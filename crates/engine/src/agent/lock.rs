//! `Engine::agent_lock` (C-09) — resolve + pin the agent-env config into `agent-env.lock`
//! WITHOUT installing to destinations. `--check` compares a fresh resolve against the on-disk
//! lock and returns the drift (no write); otherwise the rebuilt lock is saved.
//!
//! This lock is the SHA-256 `agent-env.lock` only. It lives at `crate::agent::lock` and never
//! touches the engine's FNV-1a component lock (`crate::lock`) — the two are wholly separate
//! (unifying them is TASK-0017, not this card).

use envctl_agent_env::driver::rebuild_lock;
use envctl_agent_env::lock::{save, AgentLockFile, LockMode};

use crate::agent::report::{AgentLockDriftItem, AgentLockOutcome, AgentVerb};
use crate::agent::{AgentCtx, AgentLockSpec};
use crate::event::{Event, EventSink};
use crate::Engine;

impl Engine {
    /// Resolve + pin the agent-env config into the agent-asset lock.
    ///
    /// `--check` (`spec.check == true`): re-resolve the sources, diff against the on-disk lock,
    /// emit `Event::AgentLockChecked`, and return the drift — **nothing is written**. With
    /// `lock_mode == Locked` the audit is zero-network: no source is re-resolved, so a satisfied
    /// lock reports empty drift without touching the network.
    ///
    /// Without `--check`: rebuild the lock from a fresh resolve and save it.
    pub fn agent_lock(
        &self,
        spec: AgentLockSpec,
        sink: &EventSink,
    ) -> anyhow::Result<AgentLockOutcome> {
        let ctx = AgentCtx::resolve(spec.config_path.as_deref(), spec.scope_override)?;

        sink.emit(Event::AgentRunStarted {
            verb: AgentVerb::Lock,
            scope: ctx.scope.into(),
            dry_run: spec.check,
            lock_mode: spec.lock_mode.label(),
        });

        let prev: AgentLockFile = envctl_agent_env::lock::load(&ctx.lock_file)?;

        // `--check` with `Locked` is a true zero-network audit: skip the re-resolve entirely
        // (no `materialize_source`), so a satisfied lock simply diffs clean against itself.
        let zero_network = spec.check && matches!(spec.lock_mode.to_library(), LockMode::Locked);

        let next = if zero_network {
            prev.clone()
        } else {
            rebuild_lock(&ctx.cfg, &ctx.cfg_dir, ctx.scope, &prev, &spec.upgrade_only)?
        };

        let skills = next.skills.len();
        let sources = ctx.cfg.skills.len();

        if spec.check {
            let drift: Vec<AgentLockDriftItem> = prev
                .lock_check(&next)
                .into_iter()
                .map(|d| AgentLockDriftItem {
                    status: d.status.label().to_string(),
                    id: d.id,
                })
                .collect();
            sink.emit(Event::AgentLockChecked {
                drift: drift.clone(),
            });
            return Ok(AgentLockOutcome {
                check: true,
                saved: false,
                skills,
                sources,
                drift,
                lock_path: None,
            });
        }

        let mut next = next;
        save(&mut next, &ctx.lock_file)?;
        Ok(AgentLockOutcome {
            check: false,
            saved: true,
            skills,
            sources,
            drift: Vec::new(),
            lock_path: Some(ctx.lock_file.to_string_lossy().to_string()),
        })
    }
}
