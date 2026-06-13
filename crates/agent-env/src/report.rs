//! Sync-result value types — ported verbatim from kasetto v3.2.0 `src/model/types.rs`
//! (ledger M-25). These are the typed outputs of an agent-asset `sync` / `list` run:
//! the per-run [`Summary`] counters, the per-skill [`Action`] log, the aggregate
//! [`Report`], the `list`-view [`InstalledSkill`] row, and the [`SyncFailure`] record.
//!
//! NOTE (TASK-0012 scope): only the **types + their serde** are ported here. The sync
//! ENGINE that fills them (`commands/sync`, `commands/list`) is TASK-0013. The state/lock
//! types `State`, `SkillEntry`, `LOCK_VERSION` already live in [`crate::lock`] and are not
//! re-defined here (kasetto co-locates them in `types.rs`; the seed split the lock surface).

use serde::Serialize;

use crate::config::Scope;

/// Per-run outcome counters (ledger M-25).
#[derive(Debug, Serialize, Default)]
pub struct Summary {
    /// Skills newly installed this run.
    pub installed: usize,
    /// Skills whose content changed and were re-written.
    pub updated: usize,
    /// Skills removed (orphaned from the config).
    pub removed: usize,
    /// Skills already present and content-identical.
    pub unchanged: usize,
    /// Lock entries whose on-disk asset is missing/corrupt.
    pub broken: usize,
    /// Sources/skills that errored during the run.
    pub failed: usize,
}

/// A single recorded step in a sync run — the per-skill (or per-source) action log row
/// (ledger M-25).
#[derive(Debug, Serialize)]
pub struct Action {
    /// The source this action pertains to (`None` for source-less actions).
    pub source: Option<String>,
    /// The skill this action pertains to (`None` for source-level actions).
    pub skill: Option<String>,
    /// The outcome label (e.g. `installed`, `updated`, `unchanged`, `removed`, `failed`).
    pub status: String,
    /// The error detail when `status` denotes a failure.
    pub error: Option<String>,
}

/// The serialized result of a whole sync run (ledger M-25).
#[derive(Debug, Serialize)]
pub struct Report {
    /// Opaque per-run identifier.
    pub run_id: String,
    /// The config file path this run was driven from.
    pub config: String,
    /// The resolved destination root for this run.
    pub destination: String,
    /// Whether this was a preview (no writes).
    pub dry_run: bool,
    /// Aggregate counters.
    pub summary: Summary,
    /// Ordered action log.
    pub actions: Vec<Action>,
}

/// A `list`-view row describing one installed skill (ledger M-25).
#[derive(Debug, Serialize, Clone)]
pub struct InstalledSkill {
    /// Stable identifier (typically `source::skill`).
    pub id: String,
    /// Scope this skill was installed under.
    pub scope: Scope,
    /// Display name.
    pub name: String,
    /// Skill description (from its profile).
    pub description: String,
    /// The source it came from.
    pub source: String,
    /// The skill slug.
    pub skill: String,
    /// The install destination path.
    pub destination: String,
    /// The recorded content hash.
    pub hash: String,
    /// The source revision label (`ref:…` / `branch:…` / `local`).
    pub source_revision: String,
    /// Absolute timestamp of last install/update.
    pub updated_at: String,
    /// Human-friendly relative age (e.g. `2h ago`).
    pub updated_ago: String,
}

/// A recorded sync failure for a source/skill (ledger M-25).
#[derive(Debug, Serialize, Clone)]
pub struct SyncFailure {
    /// The skill/source name that failed.
    pub name: String,
    /// The source it came from.
    pub source: String,
    /// The human-readable failure reason.
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_default_is_all_zero() {
        let s = Summary::default();
        assert_eq!(s.installed, 0);
        assert_eq!(s.updated, 0);
        assert_eq!(s.removed, 0);
        assert_eq!(s.unchanged, 0);
        assert_eq!(s.broken, 0);
        assert_eq!(s.failed, 0);
    }

    #[test]
    fn report_serializes_with_expected_field_names() {
        let report = Report {
            run_id: "run-1".into(),
            config: "kasetto.yaml".into(),
            destination: "/dest".into(),
            dry_run: true,
            summary: Summary {
                installed: 2,
                ..Default::default()
            },
            actions: vec![Action {
                source: Some("github.com/a/b".into()),
                skill: Some("review".into()),
                status: "installed".into(),
                error: None,
            }],
        };
        let v: serde_json::Value = serde_json::to_value(&report).expect("serialize");
        assert_eq!(v["run_id"], "run-1");
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["summary"]["installed"], 2);
        assert_eq!(v["actions"][0]["status"], "installed");
        // Optional `error` still serializes as null (kasetto does not skip it).
        assert!(v["actions"][0]["error"].is_null());
    }

    #[test]
    fn installed_skill_serializes_scope() {
        let skill = InstalledSkill {
            id: "s::review".into(),
            scope: Scope::Project,
            name: "review".into(),
            description: "desc".into(),
            source: "src".into(),
            skill: "review".into(),
            destination: "/d".into(),
            hash: "deadbeef".into(),
            source_revision: "branch:main".into(),
            updated_at: "2026-06-13T00:00:00Z".into(),
            updated_ago: "now".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&skill).expect("serialize");
        assert_eq!(v["scope"], "project");
        assert_eq!(v["source_revision"], "branch:main");
    }

    #[test]
    fn sync_failure_serializes() {
        let f = SyncFailure {
            name: "n".into(),
            source: "s".into(),
            reason: "boom".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&f).expect("serialize");
        assert_eq!(v["name"], "n");
        assert_eq!(v["reason"], "boom");
    }
}
