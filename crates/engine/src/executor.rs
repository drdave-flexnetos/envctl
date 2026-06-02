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
use crate::model::{AddRepoSpec, OpResult, OpStatus, Registry, ResetGates, RunPlan, RunSummary, Wiring};
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

    let mut summary = RunSummary::default();

    // ---- Reset gates (Phase::Remove only), evaluated ONCE under frozen ctx ----
    if plan.phase == Phase::Remove {
        // (1) Untargeted whole-roster reset requires --all AND --confirm.
        if plan.targets.is_empty() && !(plan.gates.all && plan.gates.confirm) {
            let reason = "refusing whole-roster reset: pass --all --confirm".to_string();
            sink.emit(Event::GuardRefused { component: "<reset>".into(), reason: reason.clone() });
            summary.refused.push("<reset>".into());
            finish(sink, &mut summary, mkres_id("<reset>", Phase::Remove, OpStatus::Refused, &reason, plan.dry_run));
            sink.emit(Event::RunFinished { summary: summary.clone() });
            return Ok(summary);
        }
        // (2)+(3) Reverse-dependent refusal / cascade fold (explicit targets only).
        if !plan.targets.is_empty() {
            let target_set: HashSet<String> = order.iter().map(|c| c.id.clone()).collect();
            let mut fold: HashSet<String> = HashSet::new();
            let mut refuse: HashSet<String> = HashSet::new();
            for tid in &plan.targets {
                for rdep in reg.reverse_dependents(tid) {
                    let live = run_phase(sink, rdep, Phase::Detect, runner, false, &ctx).status == OpStatus::Ok;
                    if live && !target_set.contains(&rdep.id) {
                        if plan.gates.cascade {
                            fold.insert(rdep.id.clone());
                        } else {
                            refuse.insert(tid.clone());
                        }
                    }
                }
            }
            for tid in &refuse {
                let reason = format!("refusing remove of {tid}: a live reverse-dependent is not in the set (use --cascade)");
                sink.emit(Event::GuardRefused { component: tid.clone(), reason: reason.clone() });
                summary.refused.push(tid.clone());
                finish(sink, &mut summary, mkres_id(tid, Phase::Remove, OpStatus::Refused, &reason, plan.dry_run));
            }
            // Folding extra components beyond the named targets needs --confirm on --apply.
            if !fold.is_empty() && !plan.gates.confirm && !plan.dry_run {
                let list: Vec<String> = { let mut v: Vec<String> = fold.into_iter().collect(); v.sort(); v };
                let reason = format!("refusing cascade: would also remove {} — pass --confirm", list.join(", "));
                sink.emit(Event::GuardRefused { component: "<cascade>".into(), reason: reason.clone() });
                summary.refused.push("<cascade>".into());
                finish(sink, &mut summary, mkres_id("<cascade>", Phase::Remove, OpStatus::Refused, &reason, plan.dry_run));
                sink.emit(Event::RunFinished { summary: summary.clone() });
                return Ok(summary);
            }
            if !refuse.is_empty() {
                order.retain(|c| !refuse.contains(&c.id));
            }
            if !fold.is_empty() {
                let mut keep: HashSet<String> = order.iter().map(|c| c.id.clone()).collect();
                keep.extend(fold.iter().cloned());
                order = reg.ordered().filter(|c| keep.contains(&c.id)).collect();
                order.reverse();
            }
        }
    }

    // Pre-warm sudo (+ keepalive) if this run will need it; dropped at fn end.
    let _sudo = prewarm_sudo(&order, plan.phase, plan.dry_run, sink);

    let total = order.len();
    sink.emit(Event::RunStarted {
        phase: plan.phase,
        total,
        dry_run: plan.dry_run,
    });

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
                    let mut res = mkres(comp, plan.phase, OpStatus::Skipped, "already present", false);
                    apply_wiring(comp, sink, &mut res, &mut summary);
                    finish(sink, &mut summary, res);
                    continue;
                }
            }
        }

        // Auto-fix triage (Phase::Fix): act ONLY on broken/partial components.
        if plan.phase == Phase::Fix && !plan.dry_run {
            if comp.detect.is_some()
                && run_phase(sink, comp, Phase::Detect, runner, false, &ctx).status != OpStatus::Ok
            {
                finish(sink, &mut summary, mkres(comp, Phase::Fix, OpStatus::Skipped, "not installed; use install", false));
                continue;
            }
            let healthy = comp.verify.is_none()
                || run_phase(sink, comp, Phase::Verify, runner, false, &ctx).status == OpStatus::Ok;
            if healthy && wiring_present(comp) {
                finish(sink, &mut summary, mkres(comp, Phase::Fix, OpStatus::Skipped, "already healthy", false));
                continue;
            }
            // A system-scope fix (apt/nix/cdi/alt) is destructive infra — gate it.
            if has_system_scope(&comp.wiring) && !plan.gates.confirm {
                let reason = "system-scope fix needs --confirm".to_string();
                sink.emit(Event::GuardRefused { component: comp.id.clone(), reason: reason.clone() });
                summary.refused.push(comp.id.clone());
                finish(sink, &mut summary, mkres(comp, Phase::Fix, OpStatus::Refused, &reason, false));
                continue;
            }
        }

        let mut res = run_phase(sink, comp, plan.phase, runner, plan.dry_run, &ctx);
        match res.status {
            OpStatus::Failed => {
                summary.failed.push(comp.id.clone());
                failed_ids.insert(comp.id.clone());
            }
            OpStatus::Refused => summary.refused.push(comp.id.clone()),
            OpStatus::SkippedBlocked => summary.skipped_blocked.push(comp.id.clone()),
            _ => {}
        }

        // Wiring + post-action re-verify (frozen ctx; never on dry-run; only when
        // the hook actually acted: Ok | NoHook).
        if !plan.dry_run && matches!(res.status, OpStatus::Ok | OpStatus::NoHook) {
            match plan.phase {
                Phase::Install => apply_wiring(comp, sink, &mut res, &mut summary),
                Phase::Remove => {
                    revert_wiring(comp, &plan.gates, &ctx, sink, &mut res, &mut summary);
                    // reset must leave the component ABSENT.
                    if let Some(d) = reverify_absent(comp, runner, sink, &ctx) {
                        res = d;
                        summary.incomplete.push(comp.id.clone());
                    }
                }
                Phase::Fix => {
                    apply_wiring(comp, sink, &mut res, &mut summary);
                    // auto-fix must leave the component HEALTHY.
                    if let Some(d) = reverify_healthy(comp, runner, sink, &ctx) {
                        res = d;
                        summary.incomplete.push(comp.id.clone());
                    }
                }
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

fn mkres_id(id: &str, phase: Phase, status: OpStatus, msg: &str, dry_run: bool) -> OpResult {
    OpResult {
        component: id.into(),
        phase,
        status,
        exit_code: None,
        duration_ms: 0,
        message: msg.into(),
        dry_run,
    }
}

fn wiring_empty(w: &Wiring) -> bool {
    w.path_entries.is_empty()
        && w.shell_rc.is_empty()
        && w.desktop_entries.is_empty()
        && w.systemd_user.is_empty()
        && w.apt_repos.is_empty()
        && w.nix_conf_lines.is_empty()
        && w.cdi_specs.is_empty()
        && w.alternatives.is_empty()
        && w.data_paths.is_empty()
        && w.config_paths.is_empty()
}

fn has_system_scope(w: &Wiring) -> bool {
    !w.apt_repos.is_empty()
        || !w.nix_conf_lines.is_empty()
        || !w.cdi_specs.is_empty()
        || !w.alternatives.is_empty()
}

/// True iff every shell_rc marker block this component owns is present (matches
/// detect.rs::wiring_present; suffix-agnostic so wizard-written blocks count).
fn wiring_present(comp: &Component) -> bool {
    if comp.wiring.shell_rc.is_empty() {
        return true; // nothing to reconcile
    }
    comp.wiring.shell_rc.iter().all(|blk| {
        let file = match blk.file.strip_prefix("~/") {
            Some(rest) => match std::env::var("HOME") {
                Ok(h) => format!("{h}/{rest}"),
                Err(_) => return false,
            },
            None => blk.file.clone(),
        };
        std::fs::read_to_string(&file)
            .map(|t| t.contains(&format!("BEGIN {}", blk.marker)))
            .unwrap_or(false)
    })
}

fn emit_wiring(comp: &Component, sink: &EventSink, rep: &crate::wiring::WiringReport, verb: &str) {
    for n in &rep.notes {
        sink.emit(Event::Log { component: comp.id.clone(), stream: Stream::Stdout, line: n.clone() });
    }
    for (kind, e) in &rep.failures {
        sink.emit(Event::Log {
            component: comp.id.clone(),
            stream: Stream::Stderr,
            line: format!("wiring {verb} ({kind}) failed: {e}"),
        });
    }
    if rep.notes.is_empty() && rep.failures.is_empty() {
        sink.emit(Event::Log { component: comp.id.clone(), stream: Stream::Stdout, line: format!("wiring {verb}") });
    }
}

fn apply_wiring(comp: &Component, sink: &EventSink, res: &mut OpResult, summary: &mut RunSummary) {
    if wiring_empty(&comp.wiring) {
        return;
    }
    let rep = crate::wiring::apply(&comp.wiring);
    emit_wiring(comp, sink, &rep, "applied");
    if !rep.failures.is_empty() && matches!(res.status, OpStatus::Ok | OpStatus::NoHook) {
        res.status = OpStatus::Incomplete;
        res.message = "wiring apply reported failures (see log)".into();
        summary.incomplete.push(comp.id.clone());
    }
}

fn revert_wiring(
    comp: &Component,
    gates: &ResetGates,
    ctx: &RunContext,
    sink: &EventSink,
    res: &mut OpResult,
    summary: &mut RunSummary,
) {
    if wiring_empty(&comp.wiring) {
        return;
    }
    let rep = crate::wiring::revert(&comp.wiring, gates, ctx);
    emit_wiring(comp, sink, &rep, "reverted");
    if !rep.failures.is_empty() && matches!(res.status, OpStatus::Ok | OpStatus::NoHook) {
        res.status = OpStatus::Incomplete;
        res.message = "wiring revert reported failures (see log)".into();
        summary.incomplete.push(comp.id.clone());
    }
}

/// reset/Remove postcondition: detect must now FAIL (absent). No detect hook =>
/// unverifiable => satisfied (None).
fn reverify_absent(comp: &Component, runner: &dyn HookRunner, sink: &EventSink, ctx: &RunContext) -> Option<OpResult> {
    comp.detect.as_ref()?;
    if run_phase(sink, comp, Phase::Detect, runner, false, ctx).status == OpStatus::Ok {
        Some(mkres(comp, Phase::Remove, OpStatus::Incomplete,
            "removed, but still detected (orphaned/partial remove) — re-run reset or inspect", false))
    } else {
        None
    }
}

/// auto-fix/Fix postcondition: verify must now SUCCEED (healthy). No verify hook
/// => unverifiable => satisfied (None).
fn reverify_healthy(comp: &Component, runner: &dyn HookRunner, sink: &EventSink, ctx: &RunContext) -> Option<OpResult> {
    comp.verify.as_ref()?;
    if run_phase(sink, comp, Phase::Verify, runner, false, ctx).status == OpStatus::Ok {
        None
    } else {
        Some(mkres(comp, Phase::Fix, OpStatus::Incomplete,
            "fix ran, but verify still fails — review log / escalate", false))
    }
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
    order.iter().any(|c| {
        // System-scope wiring (apt/nix/cdi/alt) runs sudo during apply/revert.
        has_system_scope(&c.wiring)
            || match c.hook(phase) {
                Some(crate::component::Hook::Command { needs_sudo, .. }) => *needs_sudo,
                Some(crate::component::Hook::ShippedScript { needs_sudo, .. }) => *needs_sudo,
                Some(crate::component::Hook::Script { needs_sudo, script, .. }) => {
                    *needs_sudo || script.contains("sudo ")
                }
                None => false,
            }
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
