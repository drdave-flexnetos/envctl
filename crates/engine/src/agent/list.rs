//! `Engine::agent_list` (C-10) — the read-only inventory of installed agent assets. With no
//! `--scope` override the global + project locks are merged (kasetto's default `list` view);
//! a `--scope` filters to just that lock. Filtered by [`AgentListKind`]. Never writes.

use std::path::Path;

use envctl_agent_env::driver::load_skills_mcps_commands;
use envctl_agent_env::lock::AgentLockFile;
use envctl_agent_env::Scope;

use crate::agent::report::{AgentList, AgentVerb};
use crate::agent::{AgentListKind, AgentListSpec};
use crate::event::{Event, EventSink};
use crate::Engine;

impl Engine {
    /// List installed agent assets (skills + MCP servers + commands). Read-only.
    pub fn agent_list(&self, spec: AgentListSpec, sink: &EventSink) -> anyhow::Result<AgentList> {
        let scope_override: Option<Scope> = spec.scope_override.map(Into::into);
        let merged = scope_override.is_none();

        sink.emit(Event::AgentRunStarted {
            verb: AgentVerb::List,
            scope: spec
                .scope_override
                .unwrap_or(crate::agent::AgentScope::Project),
            dry_run: true,
            lock_mode: "plain".into(),
        });

        let project_root = std::env::current_dir().unwrap_or_default();

        let load_lock = |scope: Scope, root: &Path| -> envctl_agent_env::Result<AgentLockFile> {
            let path = crate::agent::agent_lock_path(scope, root)
                .map_err(|e| envctl_agent_env::AgentEnvError::Message(e.to_string()))?;
            envctl_agent_env::lock::load(&path)
        };
        let load_updated = |scope: Scope, root: &Path| crate::agent::load_updated_for(scope, root);

        let (mut skills, mut mcps, mut commands) =
            load_skills_mcps_commands(scope_override, &project_root, &load_lock, &load_updated)?;

        if !matches!(spec.kind, AgentListKind::All | AgentListKind::Skills) {
            skills.clear();
        }
        if !matches!(spec.kind, AgentListKind::All | AgentListKind::Mcps) {
            mcps.clear();
        }
        if !matches!(spec.kind, AgentListKind::All | AgentListKind::Commands) {
            commands.clear();
        }

        let list = AgentList {
            skills,
            mcps,
            commands,
            merged_scopes: merged,
        };
        Ok(list)
    }
}
