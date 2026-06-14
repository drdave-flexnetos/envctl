//! Typed engine returns for the agent-asset verbs — the parity contract both the CLI and
//! the GUI render. These wrap the pure `agent_env::report::*` value types (Summary/Action/
//! InstalledSkill/AssetRow) with the engine-level run metadata (run id, config label,
//! destination, scope, dry-run) and the per-verb outcomes (lock drift, edit actions, clean
//! counts). All are `Serialize` so a `--json` front-end serializes them directly — no engine
//! method needs a json flag.

use serde::{Deserialize, Serialize};

use envctl_agent_env::driver::AssetRow;
use envctl_agent_env::report::{Action, InstalledSkill, Summary};

use super::AgentScope;

/// Which agent-asset verb a run belongs to (drives `Event::AgentRunStarted`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentVerb {
    Sync,
    Add,
    Remove,
    Lock,
    List,
    Clean,
}

/// The result of a `sync` / `clean` run: the per-run counters + ordered action log plus the
/// run metadata. `summary.failed > 0` is the engine-level "this run had failures" signal that
/// a front-end maps to a non-zero exit code (the engine never `process::exit`s).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentReport {
    /// Opaque per-run identifier.
    pub run_id: String,
    /// The config file path this run was driven from.
    pub config: String,
    /// The resolved destination root for this run.
    pub destination: String,
    /// The scope this run resolved to.
    pub scope: AgentScope,
    /// Whether this was a preview (no writes).
    pub dry_run: bool,
    /// Aggregate counters.
    pub summary: Summary,
    /// Ordered action log.
    pub actions: Vec<Action>,
}

/// The merged `list` view: installed skills + MCP servers + commands, plus whether the two
/// scopes were merged (no `--scope` override).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentList {
    pub skills: Vec<InstalledSkill>,
    pub mcps: Vec<AssetRow>,
    pub commands: Vec<AssetRow>,
    pub merged_scopes: bool,
}

/// One drift change reported by `lock --check`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentLockDriftItem {
    /// `added` / `removed` / `updated`.
    pub status: String,
    /// The lock-asset id.
    pub id: String,
}

/// The result of `agent_lock`. In `--check` mode `drift` lists pending changes and nothing
/// is written (`saved = false`); otherwise the lock was rewritten (`saved = true`, `drift`
/// empty) and `lock_path` names the file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentLockOutcome {
    pub check: bool,
    pub saved: bool,
    pub skills: usize,
    pub sources: usize,
    pub drift: Vec<AgentLockDriftItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_path: Option<String>,
}

/// The result of `add` / `remove`: the action verb, the affected source/targets, the touched
/// sections, and (when `add`/`remove` syncs after) the embedded follow-up sync report.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentEditOutcome {
    /// `added` / `would_add` / `removed` / `would_remove`.
    pub action: String,
    /// The source the edit targeted.
    pub source: String,
    /// The targets dropped/added (for remove: per-section names; for add: the source).
    pub items: Vec<AgentEditItem>,
    pub dry_run: bool,
    /// The follow-up sync report, when the edit synced after applying (None for `--no-sync`
    /// or preview).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<AgentReport>,
}

/// One touched section in an [`AgentEditOutcome`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentEditItem {
    pub target: String,
    pub section: String,
}
