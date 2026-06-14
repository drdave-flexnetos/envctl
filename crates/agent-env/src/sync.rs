//! Shared, **pure** sync helpers — ported from kasetto v3.2.0 `src/commands/sync/`
//! (ledger C-05/C-06).
//!
//! These are the reusable, non-`Engine` pieces of the sync driver that the (deferred,
//! TASK-0013) `agent_sync` engine builds on:
//!
//! - [`remove_stale`] + [`StaleEntry`] — the shared orphan-cleanup pass (ported verbatim from
//!   `src/commands/sync/mod.rs`): for each candidate not in the desired-id set it bumps
//!   `summary.removed`, pushes a `removed` / `would_remove` [`Action`], and (when not a dry run)
//!   invokes the caller's `on_remove` so each asset kind can drop its lock/state entry and tear
//!   down on-disk artifacts.
//! - the **asset-id / key conventions** ([`skill_key`], [`command_asset_id`], [`mcp_asset_id`]
//!   and their `*_action_label` siblings) — the single point of truth for how a skill / command /
//!   MCP maps to its lock-asset id, so the id format cannot drift between the lock writer and the
//!   lookup/cleanup sites. Kasetto inlines these `format!` strings across the per-kind sync
//!   modules; they are lifted here verbatim (same string shapes) so the shared helper and the
//!   future engine agree on one definition.
//!
//! Scope: the per-kind sync *drivers* (`sync/skills.rs`, `sync/commands.rs`, `sync/mcps.rs`) and
//! the `agent_sync` Engine verb are TASK-0013 — only the pure, Engine-free helpers land here.
//! `crate::model::{Action, Summary}` → [`crate::report::Action`] / [`crate::report::Summary`].

use std::collections::HashSet;

use crate::report::{Action, Summary};

/// One stale asset candidate processed by [`remove_stale`].
///
/// `action_source` matches what the original per-kind helper emitted: skills
/// preserve the locked `entry.source`; commands/MCPs emit `None`.
/// `action_skill` is the pre-formatted action label (e.g. `"alpha"`,
/// `"command:foo"`, `"mcp:github.json"`).
pub struct StaleEntry {
    pub id: String,
    pub action_source: Option<String>,
    pub action_skill: String,
}

/// Shared orphan-cleanup pass: bumps `summary.removed`, pushes a `removed` or
/// `would_remove` action, and (when not a dry run) invokes `on_remove` so each
/// caller can drop its lock/state entry plus tear down on-disk artifacts.
pub fn remove_stale<F>(
    dry_run: bool,
    summary: &mut Summary,
    actions: &mut Vec<Action>,
    desired_ids: &HashSet<String>,
    candidates: Vec<StaleEntry>,
    mut on_remove: F,
) where
    F: FnMut(&str),
{
    for entry in candidates {
        if desired_ids.contains(&entry.id) {
            continue;
        }
        let status = if dry_run { "would_remove" } else { "removed" };
        if !dry_run {
            on_remove(&entry.id);
        }
        summary.removed += 1;
        actions.push(Action {
            source: entry.action_source,
            skill: Some(entry.action_skill),
            status: status.into(),
            error: None,
        });
    }
}

/// Lock key for a skill: `<source>::<name>`. Single point of truth so the key
/// format cannot drift between the lock writer and the lookup sites.
/// (kasetto `src/commands/sync/skills.rs::skill_key`.)
pub fn skill_key(source: &str, skill: &str) -> String {
    format!("{source}::{skill}")
}

/// Lock asset id for a command: `command::<source>::<name>`.
/// (kasetto `src/commands/sync/commands.rs`, inlined `format!`.)
pub fn command_asset_id(source: &str, name: &str) -> String {
    format!("command::{source}::{name}")
}

/// Action-log label for a command: `command:<name>`.
pub fn command_action_label(name: &str) -> String {
    format!("command:{name}")
}

/// Lock asset id for an MCP: `mcp::<source>::<file_name>`.
/// (kasetto `src/commands/sync/mcps.rs`, inlined `format!`.)
pub fn mcp_asset_id(source: &str, file_name: &str) -> String {
    format!("mcp::{source}::{file_name}")
}

/// Action-log label for an MCP: `mcp:<file_name>`.
pub fn mcp_action_label(file_name: &str) -> String {
    format!("mcp:{file_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stale(id: &str, label: &str) -> StaleEntry {
        StaleEntry {
            id: id.to_string(),
            action_source: None,
            action_skill: label.to_string(),
        }
    }

    #[test]
    fn key_conventions_match_kasetto_shapes() {
        assert_eq!(
            skill_key("github.com/a/b", "review"),
            "github.com/a/b::review"
        );
        assert_eq!(
            command_asset_id("github.com/a/b", "foo"),
            "command::github.com/a/b::foo"
        );
        assert_eq!(command_action_label("foo"), "command:foo");
        assert_eq!(
            mcp_asset_id("github.com/a/b", "github.json"),
            "mcp::github.com/a/b::github.json"
        );
        assert_eq!(mcp_action_label("github.json"), "mcp:github.json");
    }

    #[test]
    fn remove_stale_removes_only_absent_assets() {
        let mut summary = Summary::default();
        let mut actions = Vec::new();
        let mut desired = HashSet::new();
        desired.insert("keep".to_string());

        let candidates = vec![stale("keep", "kept"), stale("drop", "command:drop")];
        let mut removed: Vec<String> = Vec::new();

        remove_stale(
            false,
            &mut summary,
            &mut actions,
            &desired,
            candidates,
            |id| removed.push(id.to_string()),
        );

        // Only the absent ("drop") asset is removed; the desired ("keep") one is skipped.
        assert_eq!(removed, vec!["drop".to_string()]);
        assert_eq!(summary.removed, 1);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].status, "removed");
        assert_eq!(actions[0].skill.as_deref(), Some("command:drop"));
    }

    #[test]
    fn remove_stale_dry_run_reports_without_callback() {
        let mut summary = Summary::default();
        let mut actions = Vec::new();
        let desired = HashSet::new();
        let candidates = vec![stale("drop", "mcp:x.json")];
        let mut called = false;

        remove_stale(
            true,
            &mut summary,
            &mut actions,
            &desired,
            candidates,
            |_| called = true,
        );

        // Dry run: counted + logged as would_remove, but on_remove is NOT invoked.
        assert!(!called);
        assert_eq!(summary.removed, 1);
        assert_eq!(actions[0].status, "would_remove");
    }

    #[test]
    fn remove_stale_preserves_action_source_for_skills() {
        let mut summary = Summary::default();
        let mut actions = Vec::new();
        let desired = HashSet::new();
        let candidates = vec![StaleEntry {
            id: "src::a".into(),
            action_source: Some("src".into()),
            action_skill: "a".into(),
        }];

        remove_stale(
            false,
            &mut summary,
            &mut actions,
            &desired,
            candidates,
            |_| {},
        );

        assert_eq!(actions[0].source.as_deref(), Some("src"));
        assert_eq!(actions[0].skill.as_deref(), Some("a"));
    }
}
