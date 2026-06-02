//! `envctl` — thin CLI over the shared engine. Subcommands map 1:1 to the five
//! verbs. Destructive verbs (reset/auto-fix) are DRY-RUN by default; pass
//! `--apply` to act. `auto-detect` is read-only and prints a real EnvReport.
use clap::{Parser, Subcommand};
use envctl_engine::{
    AddRepoSpec, AiAgent, BuildStrategy, BuildSystem, Engine, EnvReport, Event, EventSink, OpStatus,
    Phase, Refactor, RefactorGoal, RenameRule, ResetGates, RunPlan, Severity,
};

#[derive(Parser)]
#[command(
    name = "envctl",
    version,
    about = "GPU-aware, source-building environment manager for this box"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    /// Emit machine-readable NDJSON / JSON instead of the pretty view.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Read-only inventory: host, GPU (works pre-driver), tools, components.
    AutoDetect {
        #[arg(long)]
        only: Vec<String>,
    },
    /// Install components (additive + idempotent; --dry-run to preview).
    Install {
        targets: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Reset = remove + unwire. DRY-RUN by default; --apply to act.
    Reset {
        targets: Vec<String>,
        #[arg(long)]
        apply: bool,
        /// Required (with --confirm) to reset the WHOLE roster (no targets).
        #[arg(long)]
        all: bool,
        /// Acknowledge a destructive whole-roster / cascade / purge reset.
        #[arg(long)]
        confirm: bool,
        /// Also remove live reverse-dependents instead of refusing.
        #[arg(long)]
        cascade: bool,
        /// Keep config-kind paths (revert wiring + remove binaries only).
        #[arg(long)]
        keep_config: bool,
        /// Permit deletion of declared data dirs (UUID re-verified first).
        #[arg(long)]
        purge: bool,
    },
    /// Auto-fix = repair broken components. DRY-RUN by default; --apply to act.
    AutoFix {
        targets: Vec<String>,
        #[arg(long)]
        apply: bool,
        /// Confirm a system-scope fix (apt/nix/cdi/alternatives).
        #[arg(long)]
        confirm: bool,
    },
    /// Add a repo as a managed component (build-from-source + wire-in).
    /// Acquire+detect+PREVIEW by default; pass --build to actually build + install.
    AddRepo {
        /// Git URL (or use --local for a working tree).
        git_url: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        local: Option<std::path::PathBuf>,
        #[arg(long, value_name = "REF")]
        git_ref: Option<String>,
        /// Force a detector: cargo|cmake|meson|autotools|make|node|python|nix_flake|go|zig.
        #[arg(long)]
        build_system: Option<String>,
        #[arg(long)]
        build_cmd: Option<String>,
        /// Artifact glob relative to the clone. Repeatable.
        #[arg(long = "artifact")]
        artifacts: Vec<String>,
        /// Strategy: as-is | cherry-pick | rename | refactor.
        #[arg(long, default_value = "as-is")]
        strategy: String,
        /// cherry-pick: only install these binaries (by file-stem). Repeatable.
        #[arg(long = "bin")]
        bins: Vec<String>,
        /// rename: install old under new name (old=new). Repeatable.
        #[arg(long = "rename", value_parser = parse_rename)]
        renames: Vec<(String, String)>,
        /// refactor=patch: shell transform run in the clone before build.
        #[arg(long)]
        patch_cmd: Option<String>,
        /// refactor=ai goal: port-to-rust | cherry-pick-to-crate | rename-for-synergy | custom.
        #[arg(long)]
        ai_goal: Option<String>,
        /// refactor=ai: force agent — claude|codex|gemini|kimi (else auto-detect).
        #[arg(long)]
        ai_agent: Option<String>,
        /// refactor=ai: extra instruction appended to the goal prompt.
        #[arg(long)]
        ai_instruction: Option<String>,
        /// Treat as a daemon (reserved for systemd --user wiring).
        #[arg(long)]
        daemon: bool,
        #[arg(long)]
        verify_cmd: Option<String>,
        /// OPT-IN: actually run the upstream build / AI agent / install (else preview).
        #[arg(long)]
        build: bool,
        /// Back up + replace a real foreign file at an install target.
        #[arg(long)]
        force: bool,
        /// git clone --recurse-submodules (off by default).
        #[arg(long)]
        recurse_submodules: bool,
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let engine = Engine::load_default()?;
    let json = cli.json;

    match cli.cmd {
        Cmd::AutoDetect { .. } => {
            // Read-only: run on the main thread and print the returned report.
            let (sink, _rx) = EventSink::channel();
            let report = engine.detect(&sink)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_report(&report);
            }
            Ok(())
        }
        other => {
            // Usage fail-fast (exit 2) before spawning the worker. The executor
            // also enforces these authoritatively (the GUI hits that path).
            if let Cmd::Reset { targets, all, confirm, purge, .. } = &other {
                if targets.is_empty() && !(*all && *confirm) {
                    eprintln!("envctl: refusing whole-roster reset — pass --all --confirm");
                    std::process::exit(2);
                }
                if *purge && !*confirm {
                    eprintln!("envctl: refusing --purge without --confirm");
                    std::process::exit(2);
                }
            }
            run_action(engine, other, json)
        }
    }
}

/// Mutating verbs: run on a worker thread, drain+print events on the main thread
/// (the same shape the GUI uses), exit nonzero iff something failed/was refused.
fn run_action(engine: Engine, cmd: Cmd, json: bool) -> anyhow::Result<()> {
    let (sink, rx) = EventSink::channel();
    let eng = engine.clone();
    let handle = std::thread::spawn(move || -> anyhow::Result<bool> {
        let ok = match cmd {
            Cmd::Install { targets, dry_run } => eng
                .run(RunPlan::new(Phase::Install, targets, dry_run), &sink)?
                .ok(),
            Cmd::Reset { targets, apply, all, confirm, cascade, keep_config, purge } => eng
                .run(
                    RunPlan::new(Phase::Remove, targets, !apply)
                        .with_gates(ResetGates { all, confirm, cascade, keep_config, purge }),
                    &sink,
                )?
                .ok(),
            Cmd::AutoFix { targets, apply, confirm } => eng
                .run(
                    RunPlan::new(Phase::Fix, targets, !apply)
                        .with_gates(ResetGates { confirm, ..Default::default() }),
                    &sink,
                )?
                .ok(),
            Cmd::AddRepo {
                git_url, id, local, git_ref, build_system, build_cmd, artifacts, strategy, bins,
                renames, patch_cmd, ai_goal, ai_agent, ai_instruction, daemon, verify_cmd, build,
                force, recurse_submodules, dry_run,
            } => {
                let spec = build_spec(AddRepoArgs {
                    git_url, id, local, git_ref, build_system, build_cmd, artifacts, strategy, bins,
                    renames, patch_cmd, ai_goal, ai_agent, ai_instruction, daemon, verify_cmd, build,
                    force, recurse_submodules,
                })
                .map_err(|e| anyhow::anyhow!(e))?;
                eng.add_repo(spec, dry_run, &sink)?.ok()
            }
            Cmd::AutoDetect { .. } => unreachable!("handled in main"),
        };
        Ok(ok) // sink drops here -> the main-thread rx.iter() terminates cleanly
    });

    for ev in rx.iter() {
        if json {
            println!("{}", serde_json::to_string(&ev)?);
        } else {
            print_event(&ev);
        }
    }

    let ok = handle.join().map_err(|_| anyhow::anyhow!("worker panicked"))??;
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

fn print_event(ev: &Event) {
    match ev {
        Event::StepStarted { component, phase, index, total } => {
            println!("\x1b[1;36m==> [{}/{}] {component} :: {phase:?}\x1b[0m", index + 1, total)
        }
        Event::Log { line, .. } => println!("    {line}"),
        Event::StepFinished { result } => match result.status {
            OpStatus::Ok => println!("\x1b[1;32m  ✓ {} {:?}\x1b[0m", result.component, result.phase),
            OpStatus::Failed => println!(
                "\x1b[1;33m  ! FAILED {} (exit {:?})\x1b[0m",
                result.component, result.exit_code
            ),
            OpStatus::Refused => println!("\x1b[1;31m  ⛔ REFUSED {}: {}\x1b[0m", result.component, result.message),
            OpStatus::Skipped => println!("  — skip {} ({})", result.component, result.message),
            OpStatus::SkippedBlocked => println!("\x1b[1;33m  — blocked {} ({})\x1b[0m", result.component, result.message),
            OpStatus::DryRun => println!("  · would {:?} {}", result.phase, result.component),
            OpStatus::RebootRequired => println!("\x1b[1;33m  ⟳ {} needs a REBOOT to take effect\x1b[0m", result.component),
            OpStatus::Incomplete => println!("\x1b[1;31m  ✗ {} acted but post-state wrong: {}\x1b[0m", result.component, result.message),
            OpStatus::NoHook => {}
        },
        Event::GuardRefused { component, reason } => {
            println!("\x1b[1;31m  ⛔ REFUSED {component}: {reason}\x1b[0m")
        }
        Event::RunFinished { summary } => {
            if summary.ok() {
                println!("\x1b[1;32mdone.\x1b[0m");
            } else {
                println!(
                    "\x1b[1;33mdone with {} failed, {} refused, {} blocked, {} incomplete.\x1b[0m",
                    summary.failed.len(),
                    summary.refused.len(),
                    summary.skipped_blocked.len(),
                    summary.incomplete.len()
                );
            }
        }
        _ => {}
    }
}

fn parse_rename(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
        .ok_or_else(|| format!("expected old=new, got `{s}`"))
}

/// Flattened add-repo flags (keeps build_spec's signature sane).
struct AddRepoArgs {
    git_url: String,
    id: String,
    local: Option<std::path::PathBuf>,
    git_ref: Option<String>,
    build_system: Option<String>,
    build_cmd: Option<String>,
    artifacts: Vec<String>,
    strategy: String,
    bins: Vec<String>,
    renames: Vec<(String, String)>,
    patch_cmd: Option<String>,
    ai_goal: Option<String>,
    ai_agent: Option<String>,
    ai_instruction: Option<String>,
    daemon: bool,
    verify_cmd: Option<String>,
    build: bool,
    force: bool,
    recurse_submodules: bool,
}

fn build_spec(a: AddRepoArgs) -> Result<AddRepoSpec, String> {
    let strategy = match a.strategy.as_str() {
        "as-is" => BuildStrategy::AsIs,
        "cherry-pick" => BuildStrategy::CherryPick { bins: a.bins },
        "rename" => BuildStrategy::Rename {
            renames: a.renames.into_iter().map(|(from, to)| RenameRule { from, to }).collect(),
        },
        "refactor" => BuildStrategy::Refactor {
            refactor: if let Some(cmd) = a.patch_cmd {
                Refactor::Patch { command: cmd }
            } else {
                Refactor::Ai {
                    agent: a.ai_agent.as_deref().map(parse_agent).transpose()?,
                    goal: parse_goal(a.ai_goal.as_deref().unwrap_or("custom"))?,
                    instruction: a.ai_instruction,
                }
            },
        },
        other => return Err(format!("unknown --strategy `{other}` (as-is|cherry-pick|rename|refactor)")),
    };
    Ok(AddRepoSpec {
        id: a.id,
        git_url: a.git_url,
        local_path: a.local,
        git_ref: a.git_ref,
        build_cmd: a.build_cmd.unwrap_or_default(),
        build_system: a.build_system.as_deref().map(parse_build_system).transpose()?,
        artifacts: a.artifacts,
        strategy,
        bin_dir: None,
        daemon: a.daemon,
        verify_cmd: a.verify_cmd,
        allow_build: a.build,
        force: a.force,
        recurse_submodules: a.recurse_submodules,
    })
}

fn parse_goal(s: &str) -> Result<RefactorGoal, String> {
    match s {
        "port-to-rust" => Ok(RefactorGoal::PortToRust),
        "cherry-pick-to-crate" => Ok(RefactorGoal::CherryPickToCrate),
        "rename-for-synergy" => Ok(RefactorGoal::RenameForSynergy),
        "custom" => Ok(RefactorGoal::Custom),
        other => Err(format!("unknown --ai-goal `{other}`")),
    }
}
fn parse_agent(s: &str) -> Result<AiAgent, String> {
    match s {
        "claude" => Ok(AiAgent::Claude),
        "codex" => Ok(AiAgent::Codex),
        "gemini" => Ok(AiAgent::Gemini),
        "kimi" => Ok(AiAgent::Kimi),
        other => Err(format!("unknown --ai-agent `{other}`")),
    }
}
fn parse_build_system(s: &str) -> Result<BuildSystem, String> {
    match s {
        "auto" => Ok(BuildSystem::Auto),
        "cargo" => Ok(BuildSystem::Cargo),
        "cmake" => Ok(BuildSystem::Cmake),
        "meson" => Ok(BuildSystem::Meson),
        "autotools" => Ok(BuildSystem::Autotools),
        "make" => Ok(BuildSystem::Make),
        "node" => Ok(BuildSystem::Node),
        "python" => Ok(BuildSystem::Python),
        "nix_flake" | "nix-flake" => Ok(BuildSystem::NixFlake),
        "go" => Ok(BuildSystem::Go),
        "zig" => Ok(BuildSystem::Zig),
        other => Err(format!("unknown --build-system `{other}`")),
    }
}

fn print_report(r: &EnvReport) {
    let yn = |b: bool| if b { "yes" } else { "no" };
    println!("\x1b[1;36m── host ──\x1b[0m");
    println!("  os       {}", r.os.as_deref().unwrap_or("?"));
    println!("  kernel   {}", r.kernel.as_deref().unwrap_or("?"));
    println!(
        "  cpu      {}  ({} threads)",
        r.cpu_model.as_deref().unwrap_or("?"),
        r.cpu_threads
    );
    println!("  memory   {} MiB", r.mem_total_mb);

    println!("\x1b[1;36m── gpu ──\x1b[0m");
    println!("  nvidia GPUs (PCI floor)  {}", r.gpu_count);
    for g in &r.gpus {
        println!("    • {g}");
    }
    println!("  driver loaded   {}", yn(r.driver_loaded));
    if let Some(v) = &r.driver_version {
        println!("  driver version  {v}");
    }
    println!("  open module     {}", yn(r.open_kernel_module));
    if let Some(c) = &r.cuda_version {
        println!("  cuda (nvcc)     {c}");
    }
    if r.software_rendered {
        println!("  \x1b[1;33m⟳ software-rendered: install/REBOOT nvidia-open to light up the GPUs\x1b[0m");
    }

    if !r.tools.is_empty() {
        println!("\x1b[1;36m── tools ──\x1b[0m");
        for t in &r.tools {
            println!("  {:<12} {}", t.name, t.version.as_deref().unwrap_or("present"));
        }
    }

    println!("\x1b[1;36m── components ──\x1b[0m");
    for c in &r.components {
        let mark = if c.detected { "\x1b[1;32m✓\x1b[0m" } else { "\x1b[1;90m·\x1b[0m" };
        let health = match c.healthy {
            Some(true) => " [healthy]",
            Some(false) => " \x1b[1;33m[unhealthy]\x1b[0m",
            None => "",
        };
        let note = if c.note.is_empty() { String::new() } else { format!("  ({})", c.note) };
        let wired = if c.wiring_present { " wired" } else { "" };
        println!("  {mark} {:<16} {}{}{}{}", c.id, c.name, health, wired, note);
    }

    if r.drift.is_empty() {
        println!("\n\x1b[1;32m── no drift: environment matches the manifest ──\x1b[0m");
    } else {
        println!("\n\x1b[1;36m── drift ({}) ──\x1b[0m", r.drift.len());
        for d in &r.drift {
            let sev = match d.severity {
                Severity::High => "\x1b[1;31mhigh\x1b[0m",
                Severity::Medium => "\x1b[1;33mmed \x1b[0m",
                Severity::Low => "\x1b[1;90mlow \x1b[0m",
            };
            println!(
                "  [{sev}] {:<22} {:?}: {}\n             → {}",
                d.component, d.kind, d.detail, d.suggested_verb
            );
        }
    }
    println!("\n  generated_at {}", r.generated_at);
}
