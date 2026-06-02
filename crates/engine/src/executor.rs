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
    } else if plan.phase == Phase::Remove {
        // AUDIT-FIX (blocker): Remove must NEVER expand a target to its
        // prerequisites. `closure(id)` is id + transitive PREREQS — correct for
        // install (deps must exist first) but catastrophic for remove: it would
        // make `reset claude-code-cli` also uninstall shared `bun`. The base set
        // is exactly the named targets; `--cascade` folds in reverse-DEPENDENTS
        // (never prerequisites) in the gate block below. Validate each id exists.
        let mut set: HashSet<&str> = HashSet::new();
        for id in &plan.targets {
            if reg.get(id).is_none() {
                anyhow::bail!("unknown component '{id}'");
            }
            set.insert(id.as_str());
        }
        reg.ordered().filter(|c| set.contains(c.id.as_str())).collect()
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
                    // audit fix (minor): the reverse-dependent's Detect hook is run here
                    // purely as a liveness PROBE to decide refuse/cascade — detect hooks
                    // must therefore be side-effect-free (idempotent, read-only).
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
            // Rebuild the removal set = (surviving named targets) ∪ (folded
            // reverse-dependents). NO closure: a target's prerequisites are never
            // auto-removed (blocker [8]), and a refused target is dropped so the
            // live reverse-dependent it protects survives (blocker [0] / FOCUS #0).
            if !refuse.is_empty() || !fold.is_empty() {
                let mut keep: HashSet<String> = plan
                    .targets
                    .iter()
                    .filter(|t| !refuse.contains(*t))
                    .cloned()
                    .collect();
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
            // run_phase (not runner.run) so the probe gets catch_unwind + gpu/guard
            // treatment, consistent with every other phase (audit fix).
            if comp.detect.is_some()
                && run_phase(sink, comp, Phase::Detect, runner, false, &ctx).status == OpStatus::Ok
            {
                {
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

    // Dedup the roster vecs — a component can be pushed onto a roster twice
    // (e.g. wiring-fail + reverify-fail both mark incomplete) (audit fix).
    for v in [
        &mut summary.failed,
        &mut summary.refused,
        &mut summary.skipped_blocked,
        &mut summary.incomplete,
    ] {
        v.sort();
        v.dedup();
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

/// True iff every wiring footprint this component owns is present on disk
/// (matches detect.rs::wiring_present; suffix-agnostic so wizard-written blocks
/// count). AUDIT-FIX (#4): previously only shell_rc was inspected, so a
/// component whose only footprint is system-scope wiring (path_entries/apt_repos/
/// nix_conf_lines/cdi_specs/alternatives) always reported present — its absence
/// was undetectable. Now each owned footprint is conservatively probed.
fn wiring_present(comp: &Component) -> bool {
    let w = &comp.wiring;

    let shell_rc_ok = w.shell_rc.iter().all(|blk| {
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
    });

    // path_entries are realized into the engine-owned "envctl PATH" block in
    // ~/.bashrc (see wiring::path_block); probe for that marker.
    let path_ok = w.path_entries.is_empty() || {
        match std::env::var("HOME") {
            Ok(h) => std::fs::read_to_string(format!("{h}/.bashrc"))
                .map(|t| t.contains("BEGIN envctl PATH"))
                .unwrap_or(false),
            Err(_) => false,
        }
    };

    // System-scope footprints: each is present iff its on-disk target exists
    // (mirrors wiring.rs apply targets: SOURCES_D/<list_file>, NIX_CONF line,
    // cdi output file, alternative link).
    let apt_ok = w.apt_repos.iter().all(|r| {
        std::path::Path::new(&format!("/etc/apt/sources.list.d/{}", r.list_file)).exists()
    });
    let nix_ok = w.nix_conf_lines.is_empty() || {
        std::fs::read_to_string("/etc/nix/nix.custom.conf")
            .map(|t| w.nix_conf_lines.iter().all(|l| t.contains(&l.line)))
            .unwrap_or(false)
    };
    let cdi_ok = w.cdi_specs.iter().all(|c| std::path::Path::new(&c.output).exists());
    let alt_ok = w.alternatives.iter().all(|a| std::path::Path::new(&a.link).exists());

    shell_rc_ok && path_ok && apt_ok && nix_ok && cdi_ok && alt_ok
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
    let id = spec.id.trim().to_string();
    validate_add_repo_spec(&spec)?; // shared gate: id slug + ''' + leading-dash guard
    if reg.get(&id).is_some() {
        anyhow::bail!("component id '{id}' already exists — refusing to shadow it (pick another --id)");
    }

    // Run the staged pipeline (acquire → [transform] → detect → build → locate →
    // shape). It refuses as root, gates real work behind spec.allow_build, and
    // streams every stage. Returns the partial summary + outcome.
    let repos_root = repos_root();
    let (mut summary, outcome) = crate::addrepo::run_pipeline(&spec, &repos_root, dry_run, sink)?;
    let Some(outcome) = outcome else {
        return Ok(summary); // pipeline short-circuited (root-refusal / a stage failed)
    };

    let bsys = crate::detect_build::system_tag(outcome.build_plan.system).to_string();
    let installed: Vec<String> = outcome
        .installs
        .iter()
        .map(|(n, _)| local_bin_target(&spec, n))
        .collect();
    let rspec = build_register_spec(&id, &spec, &outcome, &bsys, &installed);

    // PREVIEW path: no --build (or --dry-run) → show the drop-in, write nothing.
    if !spec.allow_build || dry_run {
        let toml = crate::register::synth_dropin(&rspec);
        sink.emit(Event::Log {
            component: id.clone(),
            stream: Stream::Stdout,
            line: format!("[preview] would register components.d/{id}.toml:\n{toml}"),
        });
        return Ok(summary);
    }

    // Re-check id-collision against a FRESH registry (close the long-pipeline
    // TOCTOU) BEFORE installing — so a concurrent registration can't leave
    // orphaned ~/.local/bin symlinks + a PATH block behind on the bail path (audit fix).
    if let Ok(fresh) = Registry::load(manifest_dir) {
        if fresh.get(&id).is_some() {
            anyhow::bail!("component id '{id}' was registered concurrently — refusing to overwrite");
        }
    }

    // INSTALL + WIRE-IN (symlink, refuse-shadow, refuse-unmanaged-unless-force).
    let iplan = build_install_plan(&id, &spec, &outcome);
    let ireport = crate::install::install_and_wire(&iplan, spec.force, false, sink);
    // AUDIT-FIX (#24): a half-installed add-repo must NOT persist a drop-in. If
    // install_and_wire reported failures — or produced no installed paths when
    // installs were expected — the symlinks/targets we'd record never landed, so
    // writing components.d/<id>.toml would create permanent drift (and a later
    // `reset <id>` would try to unwire links that never existed). Bail BEFORE
    // write_dropin so a failed install leaves nothing registered.
    let installs_expected = !iplan.artifacts.is_empty();
    if !ireport.failures.is_empty() || (installs_expected && ireport.installed_paths.is_empty()) {
        summary.failed.push(format!("{id}/install"));
        sink.emit(Event::Log {
            component: id.clone(),
            stream: Stream::Stderr,
            line: format!("install failed for '{id}' — not registering a drop-in (no half-installed component persisted)"),
        });
        sink.emit(Event::RunFinished { summary: summary.clone() });
        return Ok(summary);
    }

    let final_targets = if ireport.installed_paths.is_empty() { installed.clone() } else { ireport.installed_paths.clone() };
    let rspec = RegisterSpec { installed_targets: final_targets, ..rspec };
    let toml = crate::register::synth_dropin(&rspec);
    write_dropin(manifest_dir, &id, &toml, sink)?;

    sink.emit(Event::Log {
        component: id.clone(),
        stream: Stream::Stdout,
        line: format!("registered '{id}' (build-from-source). Manage with: envctl auto-detect / install {id} / reset {id} --apply"),
    });
    sink.emit(Event::RunFinished { summary: summary.clone() });
    Ok(summary)
}

use crate::install::{ArtifactPlan, InstallPlan};
use crate::model::{BuildStrategy, Refactor};
use crate::register::RegisterSpec;

/// The SINGLE add-repo gate, shared by every entry point (`executor::add_repo`
/// AND `addrepo::connect_agent`). Validates the id slug (no `/`, `..`, leading
/// dash, ≤64 chars) and every user-supplied string (leading-dash option
/// injection, `'''` manifest break, ref shape). Call this BEFORE any path join
/// or git invocation. (AUDIT-FIX blocker: the `--connect` path used to skip both
/// of these, allowing `--id ../../etc/x` traversal and git option-injection.)
pub(crate) fn validate_add_repo_spec(spec: &AddRepoSpec) -> anyhow::Result<()> {
    let id = spec.id.trim();
    if !is_valid_slug(id) {
        anyhow::bail!("invalid component id '{id}': start [a-z0-9], then [a-z0-9._-] (no spaces/slashes/..)");
    }
    validate_spec_strings(spec)
}

pub(crate) fn validate_spec_strings(spec: &AddRepoSpec) -> anyhow::Result<()> {
    let mut strs: Vec<(&str, String)> = vec![
        ("git_url", spec.git_url.clone()),
        ("build_cmd", spec.build_cmd.clone()),
    ];
    if let Some(r) = &spec.git_ref {
        strs.push(("git_ref", r.clone()));
    }
    // audit fix (minor): verify_cmd is a user string too — guard it for '''/charset
    // so the register docstring's "every user string is guarded" claim holds.
    if let Some(v) = &spec.verify_cmd {
        strs.push(("verify_cmd", v.clone()));
    }
    for g in &spec.artifacts {
        strs.push(("artifact", g.clone()));
    }
    match &spec.strategy {
        BuildStrategy::Refactor { refactor: Refactor::Patch { command } } => strs.push(("patch_cmd", command.clone())),
        BuildStrategy::Refactor { refactor: Refactor::Ai { instruction: Some(i), .. } } => strs.push(("ai_instruction", i.clone())),
        BuildStrategy::Rename { renames } => {
            for r in renames {
                if !is_valid_slug(&r.to) {
                    anyhow::bail!("--rename target '{}' is not a valid install name", r.to);
                }
                strs.push(("rename_from", r.from.clone()));
            }
        }
        BuildStrategy::CherryPick { bins } => {
            for b in bins {
                strs.push(("bin", b.clone()));
            }
        }
        _ => {}
    }
    for (label, val) in strs {
        if val.contains("'''") {
            anyhow::bail!("{label} may not contain ''' (would break the generated manifest)");
        }
    }
    // AUDIT-FIX (major): reject leading-dash values — git treats them as options
    // (e.g. git_url `--upload-pack=…` => arbitrary exec) — and validate the ref shape.
    if spec.git_url.starts_with('-') {
        anyhow::bail!("git_url may not start with '-'");
    }
    if let Some(lp) = &spec.local_path {
        if lp.to_string_lossy().starts_with('-') {
            anyhow::bail!("--local path may not start with '-'");
        }
    }
    if let Some(r) = &spec.git_ref {
        if r.starts_with('-') || !r.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/')) {
            anyhow::bail!("invalid --git-ref '{r}' (use [A-Za-z0-9._/-], no leading '-')");
        }
    }
    Ok(())
}

fn repos_root() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    std::path::PathBuf::from(home).join(".local/share/envctl/repos")
}

fn local_bin_target(_spec: &AddRepoSpec, name: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    format!("{home}/.local/bin/{name}")
}

fn build_install_plan(id: &str, _spec: &AddRepoSpec, outcome: &crate::addrepo::PipelineOutcome) -> InstallPlan {
    let artifacts = outcome
        .installs
        .iter()
        .map(|(name, src)| {
            let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            ArtifactPlan { source: src.clone(), install_name: name.clone(), renamed: name != stem }
        })
        .collect();
    InstallPlan { id: id.into(), slug: id.into(), artifacts, extra_path_entries: vec![] }
}

fn build_register_spec(
    id: &str,
    spec: &AddRepoSpec,
    outcome: &crate::addrepo::PipelineOutcome,
    build_system: &str,
    installed: &[String],
) -> RegisterSpec {
    let strategy_tag = match &spec.strategy {
        BuildStrategy::AsIs => "as-is",
        BuildStrategy::CherryPick { .. } => "cherry-pick",
        BuildStrategy::Rename { .. } => "rename",
        BuildStrategy::Refactor { refactor: Refactor::Patch { .. } } => "refactor:patch",
        BuildStrategy::Refactor { refactor: Refactor::Ai { .. } } => "refactor:ai",
    }
    .to_string();
    let transform = match &spec.strategy {
        BuildStrategy::Refactor { refactor: Refactor::Patch { command } } => Some(command.clone()),
        BuildStrategy::Refactor { refactor: Refactor::Ai { goal, instruction, .. } } => {
            Some(format!("ai goal={goal:?} {}", instruction.clone().unwrap_or_default()))
        }
        _ => None,
    };
    let relinks: Vec<(String, String)> = outcome
        .installs
        .iter()
        .map(|(name, src)| {
            let rel = src.strip_prefix(&outcome.clone_dir).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| src.to_string_lossy().into_owned());
            (name.clone(), rel)
        })
        .collect();
    let primary = outcome.installs.first().map(|(n, _)| n.clone());
    RegisterSpec {
        id: id.into(),
        slug: id.into(),
        display_name: format!("{id} (add-repo)"),
        source: spec.git_url.clone(),
        git_ref: spec.git_ref.clone(),
        resolved_sha: outcome.resolved_sha.clone().unwrap_or_default(),
        strategy_tag,
        build_system: build_system.into(),
        build_cmd: outcome.build_plan.build_cmd.clone(),
        transform,
        primary_bin: primary,
        verify_cmd: spec.verify_cmd.clone(),
        relinks,
        installed_targets: installed.to_vec(),
    }
}

fn write_dropin(manifest_dir: &Path, id: &str, toml_text: &str, sink: &EventSink) -> anyhow::Result<()> {
    let dir = manifest_dir.join("components.d");
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{id}.toml"));
    if target.exists() {
        // audit fix (minor): nanosecond epoch + uniqueness loop so two backups taken
        // within the same instant don't clobber each other (matches install.rs).
        let mut bak = dir.join(format!("{id}.toml.bak.{}", now_epoch()));
        let mut n = 0u32;
        while bak.symlink_metadata().is_ok() {
            n += 1;
            bak = dir.join(format!("{id}.toml.bak.{}.{n}", now_epoch()));
        }
        std::fs::copy(&target, &bak)?;
        sink.emit(Event::Log { component: id.into(), stream: Stream::Stdout, line: format!("backed up existing drop-in -> {}", bak.display()) });
    }
    let tmp = dir.join(format!(".{id}.toml.tmp"));
    std::fs::write(&tmp, toml_text)?;
    std::fs::rename(&tmp, &target)?;
    Ok(())
}

pub(crate) fn is_valid_slug(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

// audit fix (minor): nanosecond resolution so two same-second backups produce
// distinct `.bak.<n>` suffixes instead of colliding (matches install.rs/wiring.rs).
fn now_epoch() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
