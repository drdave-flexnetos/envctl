//! `Engine::agent_sync` — drive a full agent-asset sync (skills → commands → MCPs) against the
//! `agent-env.lock`, emitting `Event::Agent*`. Preview by default (`apply: false` → zero writes);
//! `apply: true` fetches/merges/copies and rewrites the lock + runtime. `Locked` is zero-network.

use envctl_agent_env::driver::{self, DriverCtx, SyncResult, UpdatedAt};
use envctl_agent_env::lock::{save, AgentLockFile};

use crate::agent::report::{AgentReport, AgentVerb};
use crate::agent::{AgentCtx, AgentSyncSpec};
use crate::event::{Event, EventSink};
use crate::Engine;

impl Engine {
    /// Sync the configured agent assets into their native destinations.
    ///
    /// DRY-RUN by default (`spec.apply == false`): no fetch-driven writes, no lock save, no
    /// runtime write. With `apply == true` the run materializes sources, copies skills, merges
    /// MCP configs (additive, never-clobber), writes commands, prunes orphans (only when
    /// `summary.failed == 0`), and saves the lock + runtime. `lock_mode == Locked` performs
    /// ZERO network fetches and fails closed on any source the lock can't satisfy.
    ///
    /// Returns the [`AgentReport`]; `report.summary.failed > 0` is the front-end's exit-code
    /// signal (the engine never `process::exit`s).
    pub fn agent_sync(&self, spec: AgentSyncSpec, sink: &EventSink) -> anyhow::Result<AgentReport> {
        let ctx = AgentCtx::resolve(spec.config_path.as_deref(), spec.scope_override)?;
        let report = run_sync_in_ctx(&ctx, spec.apply, &spec.lock_mode, sink)?;
        Ok(report)
    }
}

/// Shared sync execution against an already-resolved [`AgentCtx`]. Used by `agent_sync` and by
/// the `add`/`remove` self-sync (so the in-process follow-up is the identical engine path).
pub(crate) fn run_sync_in_ctx(
    ctx: &AgentCtx,
    apply: bool,
    lock_mode: &crate::agent::AgentLockMode,
    sink: &EventSink,
) -> anyhow::Result<AgentReport> {
    let library_mode = lock_mode.to_library();

    sink.emit(Event::AgentRunStarted {
        verb: AgentVerb::Sync,
        scope: ctx.scope.into(),
        dry_run: !apply,
        lock_mode: lock_mode.label(),
    });

    // Apply-only: create destination dirs up front (the kasetto `!dry_run` mkdir loop).
    if apply {
        for d in &ctx.destinations {
            std::fs::create_dir_all(d)?;
        }
    }

    let mut lock: AgentLockFile = envctl_agent_env::lock::load(&ctx.lock_file)?;
    let mut updated = UpdatedAt(crate::agent::load_updated_for(ctx.scope, &ctx.cfg_dir));

    let driver_ctx = DriverCtx::from_mode(
        &ctx.cfg,
        &ctx.cfg_dir,
        &ctx.destinations,
        ctx.scope_root.clone(),
        ctx.scope,
        apply,
        &library_mode,
    );

    let result: SyncResult = driver::sync(&driver_ctx, &mut lock, &mut updated);

    let report = assemble_report(ctx, AgentVerb::Sync, apply, result, sink);

    if apply {
        save(&mut lock, &ctx.lock_file)?;
        let report_json = serde_json::to_string(&report).ok();
        crate::agent::save_runtime_after(
            ctx.scope,
            &ctx.cfg_dir,
            updated.0,
            report_json.as_deref(),
        )?;
    }

    Ok(report)
}

/// Turn a driver [`SyncResult`] into an [`AgentReport`], emitting one `Event::AgentAction` per
/// recorded action (in order — the live tree the GUI/CLI renders) and a final
/// `Event::AgentRunFinished`.
pub(crate) fn assemble_report(
    ctx: &AgentCtx,
    _verb: AgentVerb,
    apply: bool,
    result: SyncResult,
    sink: &EventSink,
) -> AgentReport {
    for a in &result.actions {
        sink.emit(Event::AgentAction {
            source: a.source.clone(),
            asset: a.skill.clone(),
            status: a.status.clone(),
            error: a.error.clone(),
        });
    }
    let destination = ctx
        .destinations
        .first()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let report = AgentReport {
        run_id: envctl_agent_env::util::now_unix().to_string(),
        config: ctx.cfg_label.clone(),
        destination,
        scope: ctx.scope.into(),
        dry_run: !apply,
        summary: result.summary,
        actions: result.actions,
    };
    sink.emit(Event::AgentRunFinished {
        report: report.clone(),
    });
    report
}
