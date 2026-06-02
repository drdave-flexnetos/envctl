//! `envctl` — thin CLI over the shared engine. Subcommands map 1:1 to the five
//! verbs. Destructive verbs (reset/auto-fix) are DRY-RUN by default; pass
//! `--apply` to act. `auto-detect` is read-only and prints a real EnvReport.
use clap::{Parser, Subcommand};
use envctl_engine::{
    AddRepoSpec, Engine, EnvReport, Event, EventSink, OpStatus, Phase, RunPlan, Severity,
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
    },
    /// Auto-fix = repair broken components. DRY-RUN by default; --apply to act.
    AutoFix {
        targets: Vec<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Add a repo as a managed component (build-from-source + wire-in).
    AddRepo {
        git_url: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        build_cmd: String,
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
        other => run_action(engine, other, json),
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
                .run(RunPlan { phase: Phase::Install, targets, dry_run }, &sink)?
                .ok(),
            Cmd::Reset { targets, apply } => eng
                .run(RunPlan { phase: Phase::Remove, targets, dry_run: !apply }, &sink)?
                .ok(),
            Cmd::AutoFix { targets, apply } => eng
                .run(RunPlan { phase: Phase::Fix, targets, dry_run: !apply }, &sink)?
                .ok(),
            Cmd::AddRepo { git_url, id, build_cmd, dry_run } => eng
                .add_repo(
                    AddRepoSpec { id, git_url, git_ref: None, build_cmd, bin_dir: None, verify_cmd: None },
                    dry_run,
                    &sink,
                )?
                .ok(),
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
                    "\x1b[1;33mdone with {} failed, {} refused, {} blocked.\x1b[0m",
                    summary.failed.len(),
                    summary.refused.len(),
                    summary.skipped_blocked.len()
                );
            }
        }
        _ => {}
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
