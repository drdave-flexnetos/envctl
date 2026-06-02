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
    /// Dependency-graph intelligence: summary, impact/blast-radius, paths, DOT/JSON.
    Graph {
        /// Focus on one component: install closure + reset --cascade blast-radius.
        #[arg(long)]
        impact: Option<String>,
        /// Why is X needed — print the root→X dependency paths.
        #[arg(long)]
        why: Option<String>,
        /// Emit Graphviz DOT (pipe to `dot -Tsvg -o graph.svg`).
        #[arg(long)]
        dot: bool,
        /// Annotate with live detect/drift state (runs auto-detect first).
        #[arg(long)]
        live: bool,
    },
    /// Write/verify envctl.lock — a content hash of every component for reproducible
    /// installs + a CI gate. No flags = (re)write the lock; --check = verify (exit 1 on drift).
    Lock {
        /// Verify the lock matches the manifest; exit nonzero on drift (CI gate).
        #[arg(long)]
        check: bool,
    },
    /// Read-only health diagnostics: writability, toolchains, sudo, UEFI, GPU.
    Doctor,
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
        /// Interactive: clone, then drop into an agent session in the clone (for
        /// cherry-pick / port-to-rust). Pair with --build to build afterward.
        #[arg(long)]
        connect: bool,
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
        Cmd::Graph { impact, why, dot, live } => {
            use envctl_engine::graph;
            let live_report = if live {
                let (sink, _rx) = EventSink::channel();
                Some(engine.detect(&sink)?)
            } else {
                None
            };
            let reg = engine.registry();
            if dot {
                print!("{}", graph::to_dot(reg, live_report.as_ref()));
            } else if json {
                println!("{}", serde_json::to_string_pretty(&graph::to_json(reg, live_report.as_ref()))?);
            } else if let Some(id) = impact {
                print_impact(reg, &id);
            } else if let Some(id) = why {
                print_why(reg, &id);
            } else {
                print_graph_summary(reg);
            }
            Ok(())
        }
        Cmd::Lock { check } => {
            use envctl_engine::lock;
            let reg = engine.registry();
            let dir = engine.manifest_dir();
            if check {
                let locked = lock::LockFile::load(dir)?;
                let drift = lock::diff(reg, &locked);
                if json {
                    let items: Vec<_> = drift.iter().map(|(id, k)| serde_json::json!({"component": id, "drift": k})).collect();
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"locked": !drift.is_empty(), "drift": items}))?);
                } else if drift.is_empty() {
                    println!("\x1b[1;32m✓ envctl.lock matches the manifest ({} components)\x1b[0m", reg.len());
                } else {
                    println!("\x1b[1;33m✗ lock drift ({}): manifest changed without re-locking\x1b[0m", drift.len());
                    for (id, k) in &drift {
                        println!("  {:?}  {id}", k);
                    }
                    println!("  run `envctl lock` to update.");
                }
                if !drift.is_empty() {
                    std::process::exit(1);
                }
            } else {
                let mut lf = lock::generate(reg);
                lf.save(dir)?;
                println!("wrote {} ({} components)", lock::lock_path(dir).display(), reg.len());
            }
            Ok(())
        }
        Cmd::Doctor => print_doctor(&engine, json),
        // Interactive add-repo connect: handled on the MAIN thread so the agent
        // attaches to the real terminal.
        other if matches!(&other, Cmd::AddRepo { connect: true, .. }) => handle_connect(engine, other, json),
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
                force, recurse_submodules, connect: _, dry_run,
            } => {
                let spec = build_spec(AddRepoArgs {
                    git_url, id, local, git_ref, build_system, build_cmd, artifacts, strategy, bins,
                    renames, patch_cmd, ai_goal, ai_agent, ai_instruction, daemon, verify_cmd, build,
                    force, recurse_submodules,
                })
                .map_err(|e| anyhow::anyhow!(e))?;
                eng.add_repo(spec, dry_run, &sink)?.ok()
            }
            Cmd::AutoDetect { .. } | Cmd::Graph { .. } | Cmd::Lock { .. } | Cmd::Doctor => {
                unreachable!("handled in main")
            }
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

/// Interactive add-repo: build the spec, drop the user into an agent session in
/// the clone, then (if --build) build the now-transformed tree as-is.
fn handle_connect(engine: Engine, cmd: Cmd, json: bool) -> anyhow::Result<()> {
    let Cmd::AddRepo {
        git_url, id, local, git_ref, build_system, build_cmd, artifacts, strategy, bins, renames,
        patch_cmd, ai_goal, ai_agent, ai_instruction, daemon, verify_cmd, build, force,
        recurse_submodules, connect: _, dry_run: _,
    } = cmd
    else {
        unreachable!("handle_connect only called for AddRepo");
    };
    let spec = build_spec(AddRepoArgs {
        git_url, id, local, git_ref, build_system, build_cmd, artifacts, strategy, bins, renames,
        patch_cmd, ai_goal, ai_agent, ai_instruction, daemon, verify_cmd, build, force,
        recurse_submodules,
    })
    .map_err(|e| anyhow::anyhow!(e))?;

    engine.connect_repo(&spec)?; // interactive; blocks on the terminal

    if spec.allow_build {
        // Build the transformed clone AS-IS (don't re-run the agent).
        let bspec = AddRepoSpec { strategy: BuildStrategy::AsIs, allow_build: true, ..spec };
        let (sink, rx) = EventSink::channel();
        // Audit fix: capture the summary instead of discarding it so a failed
        // post-connect build exits 1, matching run_action's contract.
        let res = engine.add_repo(bspec, false, &sink)?;
        drop(sink);
        for ev in rx.iter() {
            if json {
                println!("{}", serde_json::to_string(&ev)?);
            } else {
                print_event(&ev);
            }
        }
        if !res.ok() {
            std::process::exit(1);
        }
    } else {
        println!("\nenvctl: clone is ready. Build what you made with:");
        println!("  envctl add-repo {} --id {} --strategy as-is --build", spec.git_url, spec.id);
    }
    Ok(())
}

/// Read-only health diagnostics (kasetto-style `doctor`): writability, toolchains,
/// sudo, UEFI/Secure-Boot, GPU, and the run log. Never mutates anything.
fn print_doctor(engine: &Engine, json: bool) -> anyhow::Result<()> {
    let last_run = envctl_engine::runtime::load(engine.manifest_dir()).last_run;
    let home = std::env::var("HOME").unwrap_or_default();
    let write_ok = |p: &str| -> bool {
        let dir = std::path::Path::new(p);
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
        let t = dir.join(".envctl-doctor-probe");
        let ok = std::fs::write(&t, b"x").is_ok();
        let _ = std::fs::remove_file(&t);
        ok
    };
    let has = |bin: &str| -> Option<String> {
        let out = std::process::Command::new("bash")
            .args(["-lc", &format!("command -v {bin} && {bin} --version 2>/dev/null | head -1")])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or("present").trim().to_string())
    };
    let dirs = [
        format!("{home}/.local/bin"),
        format!("{home}/.config"),
        format!("{home}/.local/share/envctl/repos"),
        "/etc".to_string(),
    ];
    let tools = ["git", "cargo", "rustc", "claude", "nix", "podman", "nvidia-smi", "gh", "uv", "bun"];
    let sudo_cached = std::process::Command::new("sudo")
        .args(["-n", "true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let uefi = std::path::Path::new("/sys/firmware/efi").exists();
    let secure_boot = std::process::Command::new("bash")
        .args(["-lc", "od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-* 2>/dev/null | tr -s ' ' | awk '{print $NF}' | head -1"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    let driver_loaded = std::path::Path::new("/proc/driver/nvidia/version").exists();
    let run_log = std::path::Path::new(&home).join(".local/state/envctl/envctl.log");
    let log_exists = run_log.exists();

    if json {
        let dirj: Vec<_> = dirs.iter().map(|d| serde_json::json!({"path": d, "writable": write_ok(d)})).collect();
        let toolj: Vec<_> = tools.iter().map(|t| serde_json::json!({"tool": t, "version": has(t)})).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "writable": dirj, "tools": toolj, "sudo_cached": sudo_cached,
                "uefi": uefi, "secure_boot": secure_boot, "nvidia_driver_loaded": driver_loaded,
                "run_log": run_log.display().to_string(), "run_log_exists": log_exists,
                "last_run": last_run,
            }))?
        );
        return Ok(());
    }

    let yn = |b: bool| if b { "\x1b[1;32m✓\x1b[0m" } else { "\x1b[1;31m✗\x1b[0m" };
    println!("\x1b[1;36m── writability ──\x1b[0m");
    for d in &dirs {
        println!("  {}  {d}", yn(write_ok(d)));
    }
    println!("\x1b[1;36m── toolchains ──\x1b[0m");
    for t in &tools {
        match has(t) {
            Some(v) => println!("  \x1b[1;32m✓\x1b[0m {t:<11} {v}"),
            None => println!("  \x1b[1;90m·\x1b[0m {t:<11} (absent)"),
        }
    }
    println!("\x1b[1;36m── system ──\x1b[0m");
    println!("  sudo (cached)      {}", yn(sudo_cached));
    println!("  UEFI               {}", yn(uefi));
    println!("  Secure Boot        {}", match secure_boot.as_deref() { Some("1") => "\x1b[1;33mON\x1b[0m (nvidia-open needs it OFF)", Some("0") => "\x1b[1;32mOFF\x1b[0m", _ => "unknown" });
    println!("  nvidia driver      {}", if driver_loaded { "\x1b[1;32mloaded\x1b[0m" } else { "\x1b[1;33mnot loaded\x1b[0m" });
    println!("  run log            {} {}", yn(log_exists), run_log.display());
    match &last_run {
        Some(lr) => println!(
            "  last op            {} {} ({}f/{}r/{}i) at {}",
            lr.verb,
            if lr.ok { "\x1b[1;32mok\x1b[0m" } else { "\x1b[1;31mFAILED\x1b[0m" },
            lr.failed, lr.refused, lr.incomplete, lr.at
        ),
        None => println!("  last op            (none recorded)"),
    }
    if !sudo_cached {
        println!("\n  note: sudo not pre-authorized — privileged installs need `sudo -v` in a real terminal first.");
    }
    Ok(())
}

fn print_graph_summary(reg: &envctl_engine::Registry) {
    let g = envctl_engine::graph::analyze(reg);
    let c = "\x1b[1;36m";
    let z = "\x1b[0m";
    println!("{c}── dependency graph ──{z}");
    println!("  {} components · {} edges · {} groups", g.nodes, g.edges, g.groups.len());
    println!("  roots (no deps):     {}", g.roots.len());
    println!("  leaves (top-level):  {}", g.leaves.len());
    if !g.orphans.is_empty() {
        println!("  orphans (standalone): {}", g.orphans.join(", "));
    }
    if let Some((id, n)) = &g.max_dependents {
        println!("  most depended-on:    {id}  ({n} direct dependents)");
    }
    if let Some((id, n)) = &g.max_requires {
        println!("  most prerequisites:  {id}  ({n} requires)");
    }
    println!("{c}── critical path (longest chain) ──{z}");
    println!("  {}", g.critical_path.join("  →  "));
    println!("\n  tip: envctl graph --impact <id> · --why <id> · --dot | dot -Tsvg -o g.svg · --json --live");
}

fn print_impact(reg: &envctl_engine::Registry, id: &str) {
    match envctl_engine::graph::impact(reg, id) {
        None => eprintln!("envctl: unknown component '{id}'"),
        Some(im) => {
            println!("\x1b[1;36m── impact of '{id}' ──\x1b[0m");
            println!("  direct requires:     {}", join_or_none(&im.requires));
            println!("  direct dependents:   {}", join_or_none(&im.required_by));
            println!("\x1b[1;32m  install {id}\x1b[0m pulls in ({}):", im.install_closure.len());
            println!("    {}", im.install_closure.join("  →  "));
            println!("\x1b[1;33m  reset {id} --cascade\x1b[0m also removes ({}):", im.cascade_removes.len());
            println!("    {}", join_or_none(&im.cascade_removes));
        }
    }
}

fn print_why(reg: &envctl_engine::Registry, id: &str) {
    let paths = envctl_engine::graph::dependency_paths(reg, id);
    if paths.is_empty() {
        eprintln!("envctl: unknown component '{id}' (or it has no paths)");
        return;
    }
    println!("\x1b[1;36m── why '{id}' is needed (root → {id} paths) ──\x1b[0m");
    for p in paths {
        println!("  {}", p.join("  →  "));
    }
}

fn join_or_none(v: &[String]) -> String {
    if v.is_empty() {
        "(none)".into()
    } else {
        v.join(", ")
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
