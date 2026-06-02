//! The best-effort run loop. Maps a `RunPlan` to an ordered target list (topo
//! forward for install/fix, reverse for remove), runs each phase, accumulates a
//! `RunSummary`, and emits `RunStarted`/`StepStarted`/`StepFinished`/`RunFinished`
//! so the CLI and GUI render identically. Also `add_repo()` — the hardened
//! drop-in writer.
//!
//! Best-effort = one component failing never aborts the run (mirrors the wizard's
//! `run()`), but a failed component's dependents are `SkippedBlocked`, and an
//! `install` whose `detect` already passes is `Skipped` (idempotent).
use crate::component::{Component, HookRunner, Phase};
use crate::error::{run_phase, RunContext};
use crate::event::{Event, EventSink, Stream};
use crate::model::{AddRepoSpec, OpResult, OpStatus, Registry, RunPlan, RunSummary};
use std::collections::HashSet;
use std::path::Path;

/// Resolve run-wide identities ONCE (no TOCTOU). GPU presence via the PCI floor
/// (driver-independent); live-root UUID via findmnt+blkid for the guard engine.
fn resolve_context() -> RunContext {
    RunContext {
        gpu_present: crate::detect::pci_nvidia_count() > 0,
        live_root_uuid: crate::guard::resolve_live_root_uuid(),
    }
}

pub fn run(
    reg: &Registry,
    runner: &dyn HookRunner,
    plan: RunPlan,
    sink: &EventSink,
) -> anyhow::Result<RunSummary> {
    let ctx = resolve_context();

    // Forward order, or reverse for Remove (tear down dependents first).
    let mut order: Vec<&Component> = if plan.targets.is_empty() {
        reg.ordered().collect()
    } else {
        let mut v: Vec<&Component> = Vec::new();
        for id in &plan.targets {
            for c in reg.closure(id)? {
                if !v.iter().any(|x| x.id == c.id) {
                    v.push(c);
                }
            }
        }
        v
    };
    if plan.phase == Phase::Remove {
        order.reverse();
    }

    // Pre-warm sudo (+ keepalive) if this run will need it; dropped at fn end.
    let _sudo = prewarm_sudo(&order, plan.phase, plan.dry_run, sink);

    let total = order.len();
    sink.emit(Event::RunStarted {
        phase: plan.phase,
        total,
        dry_run: plan.dry_run,
    });

    let mut summary = RunSummary::default();
    let mut failed_ids: HashSet<String> = HashSet::new();

    for (i, comp) in order.iter().enumerate() {
        sink.emit(Event::StepStarted {
            component: comp.id.clone(),
            phase: plan.phase,
            index: i,
            total,
        });

        // Dependency gate (forward phases): a dependency that already failed
        // this run blocks its dependents instead of running them on rubble.
        if matches!(plan.phase, Phase::Install | Phase::Fix)
            && comp.requires.iter().any(|d| failed_ids.contains(d))
        {
            let res = mkres(comp, plan.phase, OpStatus::SkippedBlocked, "dependency failed", plan.dry_run);
            summary.skipped_blocked.push(comp.id.clone());
            finish(sink, &mut summary, res);
            continue;
        }

        // Idempotent install: skip-if-already-detected (never re-run curl|bash),
        // but still reconcile its declarative wiring (idempotent) so an already-
        // installed tool with a missing PATH/rc block gets fixed.
        if plan.phase == Phase::Install && !plan.dry_run {
            if let Some(h) = comp.detect.as_ref() {
                if runner.run(&comp.id, Phase::Detect, h, false, sink).status == OpStatus::Ok {
                    apply_wiring(comp, sink);
                    let res = mkres(comp, plan.phase, OpStatus::Skipped, "already present", false);
                    finish(sink, &mut summary, res);
                    continue;
                }
            }
        }

        let res = run_phase(sink, comp, plan.phase, runner, plan.dry_run, &ctx);
        match res.status {
            OpStatus::Failed => {
                summary.failed.push(comp.id.clone());
                failed_ids.insert(comp.id.clone());
            }
            OpStatus::Refused => summary.refused.push(comp.id.clone()),
            OpStatus::SkippedBlocked => summary.skipped_blocked.push(comp.id.clone()),
            _ => {}
        }

        // Wiring: apply after a successful install, revert after a successful
        // remove (never on dry-run; never if the hook itself failed/was refused).
        if !plan.dry_run && matches!(res.status, OpStatus::Ok | OpStatus::NoHook) {
            match plan.phase {
                Phase::Install => apply_wiring(comp, sink),
                Phase::Remove => revert_wiring(comp, sink),
                _ => {}
            }
        }

        finish(sink, &mut summary, res);
    }

    sink.emit(Event::RunFinished {
        summary: summary.clone(),
    });
    Ok(summary)
}

fn mkres(comp: &Component, phase: Phase, status: OpStatus, msg: &str, dry_run: bool) -> OpResult {
    OpResult {
        component: comp.id.clone(),
        phase,
        status,
        exit_code: None,
        duration_ms: 0,
        message: msg.into(),
        dry_run,
    }
}

fn finish(sink: &EventSink, summary: &mut RunSummary, res: OpResult) {
    sink.emit(Event::StepFinished { result: res.clone() });
    summary.results.push(res);
}

fn apply_wiring(comp: &Component, sink: &EventSink) {
    if comp.wiring.shell_rc.is_empty() {
        return;
    }
    match crate::wiring::apply(&comp.wiring) {
        Ok(()) => sink.emit(Event::Log {
            component: comp.id.clone(),
            stream: Stream::Stdout,
            line: "wiring applied (shell-rc reconciled)".into(),
        }),
        Err(e) => sink.emit(Event::Log {
            component: comp.id.clone(),
            stream: Stream::Stderr,
            line: format!("wiring apply failed: {e}"),
        }),
    }
}

fn revert_wiring(comp: &Component, sink: &EventSink) {
    if comp.wiring.shell_rc.is_empty() {
        return;
    }
    let _ = crate::wiring::revert(&comp.wiring);
    sink.emit(Event::Log {
        component: comp.id.clone(),
        stream: Stream::Stdout,
        line: "wiring reverted (owned shell-rc block excised)".into(),
    });
}

/// Pre-warm sudo once (so streamed, TTY-less hooks don't prompt) and keep the
/// credential fresh for the duration of the run. Returns a guard that stops the
/// keepalive on drop. No-op unless the run actually needs sudo.
struct SudoKeepalive {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl Drop for SudoKeepalive {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn needs_sudo(order: &[&Component], phase: Phase) -> bool {
    order.iter().any(|c| match c.hook(phase) {
        Some(crate::component::Hook::Command { needs_sudo, .. }) => *needs_sudo,
        Some(crate::component::Hook::ShippedScript { needs_sudo, .. }) => *needs_sudo,
        Some(crate::component::Hook::Script { needs_sudo, script, .. }) => {
            *needs_sudo || script.contains("sudo ")
        }
        None => false,
    })
}

fn prewarm_sudo(order: &[&Component], phase: Phase, dry_run: bool, sink: &EventSink) -> Option<SudoKeepalive> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    if dry_run || !matches!(phase, Phase::Install | Phase::Fix | Phase::Remove) {
        return None;
    }
    if !needs_sudo(order, phase) {
        return None;
    }
    // `sudo -v` inherits this process's stdio: from a real terminal it prompts
    // once; with no TTY it fails fast (and we warn) rather than hanging later.
    let ok = std::process::Command::new("sudo")
        .arg("-v")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        sink.emit(Event::Log {
            component: "sudo".into(),
            stream: Stream::Stderr,
            line: "could not pre-authorize sudo (no TTY / not a sudoer?) — privileged steps will fail fast. Run from a real terminal."
                .into(),
        });
        return None;
    }
    sink.emit(Event::Log {
        component: "sudo".into(),
        stream: Stream::Stdout,
        line: "sudo pre-authorized; keepalive running for this run".into(),
    });
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let handle = std::thread::spawn(move || {
        while !stop2.load(Ordering::Relaxed) {
            let _ = std::process::Command::new("sudo").arg("-n").arg("true").status();
            for _ in 0..50 {
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    });
    Some(SudoKeepalive { stop, handle: Some(handle) })
}

// ---------------------------------------------------------------------------
// add-repo (hardened drop-in writer). Phase-0 scope: validate + register a
// component drop-in atomically (with backup); the build itself runs on the next
// explicit `envctl install <id>`. The full 9-stage build pipeline is Phase 4.
// ---------------------------------------------------------------------------
pub fn add_repo(
    manifest_dir: &Path,
    reg: &Registry,
    _runner: &dyn HookRunner,
    spec: AddRepoSpec,
    dry_run: bool,
    sink: &EventSink,
) -> anyhow::Result<RunSummary> {
    let id = spec.id.trim();
    // 1. strict slug validation (safe as a bare TOML key AND a filename component)
    if !is_valid_slug(id) {
        anyhow::bail!(
            "invalid component id '{id}': use [a-z0-9] start, then [a-z0-9._-] (no spaces/slashes)"
        );
    }
    // 2. refuse collision with an existing/built-in component
    if reg.get(id).is_some() {
        anyhow::bail!("component id '{id}' already exists — refusing to shadow it (pick another --id)");
    }
    // 3. reject inputs that could break out of the TOML literal we emit
    for (label, val) in [("git_url", &spec.git_url), ("build_cmd", &spec.build_cmd)] {
        if val.contains("'''") {
            anyhow::bail!("{label} may not contain ''' (would break the generated manifest)");
        }
    }

    let toml_text = synth_component_toml(id, &spec);

    if dry_run {
        sink.emit(Event::Log {
            component: id.into(),
            stream: Stream::Stdout,
            line: format!("[dry-run] would write components.d/{id}.toml:\n{toml_text}"),
        });
        return Ok(RunSummary::default());
    }

    // 4. atomic temp+rename, with a timestamped backup of any existing drop-in
    let dir = manifest_dir.join("components.d");
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{id}.toml"));
    if target.exists() {
        let bak = dir.join(format!("{id}.toml.bak.{}", now_epoch()));
        std::fs::copy(&target, &bak)?;
        sink.emit(Event::Log {
            component: id.into(),
            stream: Stream::Stdout,
            line: format!("backed up existing drop-in -> {}", bak.display()),
        });
    }
    let tmp = dir.join(format!(".{id}.toml.tmp"));
    std::fs::write(&tmp, &toml_text)?;
    std::fs::rename(&tmp, &target)?;

    sink.emit(Event::Log {
        component: id.into(),
        stream: Stream::Stdout,
        line: format!(
            "registered component '{id}' -> {}\n  build it with:  envctl install {id}",
            target.display()
        ),
    });
    Ok(RunSummary::default())
}

fn is_valid_slug(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Build the drop-in. `id` is a validated slug (safe bare key). `git_url` is
/// single-quoted for bash; `build_cmd` is the user's own build command, embedded
/// in a TOML literal string (both pre-checked for `'''`).
fn synth_component_toml(id: &str, spec: &AddRepoSpec) -> String {
    let url_sq = sh_single_quote(&spec.git_url);
    format!(
        "# generated by `envctl add-repo` — edit freely or re-run add-repo --force\n\
         [[component]]\n\
         id = \"{id}\"\n\
         name = \"{id} (add-repo)\"\n\
         description = \"Built from source via envctl add-repo.\"\n\
         \n\
         [component.detect]\n\
         kind = \"command\"\n\
         command = \"bash\"\n\
         args = [\"-lc\", \"command -v {id}\"]\n\
         \n\
         [component.install]\n\
         kind = \"script\"\n\
         login_shell = true\n\
         script = '''\n\
         set -e\n\
         install -d -m 700 \"$HOME/.local/share/envctl/repos\"\n\
         SRC=\"$HOME/.local/share/envctl/repos/{id}\"\n\
         if [ -d \"$SRC/.git\" ]; then git -C \"$SRC\" pull --ff-only; else git clone {url} \"$SRC\"; fi\n\
         cd \"$SRC\"\n\
         {build}\n\
         '''\n",
        id = id,
        url = url_sq,
        build = spec.build_cmd,
    )
}

/// POSIX single-quote: wrap in '...' and replace each ' with '\'' .
fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
