//! The agent-asset lock — SHA-256 content lock for provisioned skills/MCPs/commands.
//! Consolidates kasetto v3.2.0 `src/lock.rs` + the mode/drift logic in
//! `src/commands/lock.rs`.
//!
//! This is a **separate type** from the engine's FNV-1a component lock
//! (`crates/engine/src/lock.rs`); the two never share code. This lock is designed to be
//! embedded into `envctl.lock` under its own keyed section ([`AGENT_ASSETS_KEY`]),
//! leaving the FNV-1a component section untouched (TASK-0017 does the embedding;
//! TASK-0012 ships it standalone).
//!
//! ## 3 modes ([`LockMode`])
//! - [`LockMode::Plain`] — verify + fetch as needed, write/refresh the lock.
//! - [`LockMode::Update`] — re-resolve the named packages' refs and rewrite the lock.
//! - [`LockMode::Locked`] — verify the lock is satisfied with **zero network fetch**;
//!   fail-closed if it isn't.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Scope;
use crate::{err, Result};

/// Schema version of the agent-asset lock section. Matches kasetto's portable v2 format:
/// scope-relative `destination` paths, no machine-/run-specific fields.
pub const LOCK_VERSION: u8 = 2;

/// Top-level key under which this lock serializes when embedded into `envctl.lock`
/// (TASK-0017). The FNV-1a component lock lives under its own, separate section.
pub const AGENT_ASSETS_KEY: &str = "agent_assets";

/// Default standalone filename for the agent-asset lock (when not embedded).
pub const LOCK_FILENAME: &str = "agent-env.lock";

/// A tracked skill entry (the verbatim-copied skill tree, SHA-256 hashed).
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct AgentLockEntry {
    /// Install path relative to the scope root (portable across machines); legacy locks
    /// may store an absolute path here, which is still honored.
    pub destination: String,
    pub hash: String,
    pub skill: String,
    #[serde(default)]
    pub description: String,
    pub source: String,
    pub source_revision: String,
    /// Scope this entry was installed under (present for locks written by newer envctl).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
}

/// A tracked non-skill asset (command or MCP) recorded in the lock.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct AssetEntry {
    pub kind: String,
    pub name: String,
    pub hash: String,
    pub source: String,
    /// For commands: install paths relative to the scope root (CSV).
    /// For MCPs: the merged server names (CSV).
    pub destination: String,
    /// Resolved git revision label (e.g. `ref:v1.0`, `branch:main`, `local`). Defaulted to
    /// empty for backwards compatibility with v2 locks written before this field existed;
    /// drift checks skip the revision comparison when this is empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_revision: String,
}

/// Portable, commit-friendly manifest of installed agent assets (skills + commands/MCPs).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct AgentLockFile {
    #[serde(default = "default_version")]
    pub version: u8,
    #[serde(default)]
    pub skills: BTreeMap<String, AgentLockEntry>,
    #[serde(default)]
    pub assets: BTreeMap<String, AssetEntry>,
}

fn default_version() -> u8 {
    LOCK_VERSION
}

impl Default for AgentLockFile {
    fn default() -> Self {
        Self {
            version: LOCK_VERSION,
            skills: BTreeMap::new(),
            assets: BTreeMap::new(),
        }
    }
}

/// The mode a lock operation runs in (kasetto's `sync`/`lock` mode logic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockMode {
    /// Verify + fetch as needed, write/refresh the lock (default).
    Plain,
    /// Re-resolve the named packages' refs and rewrite the lock (`--update <name>...`);
    /// empty means re-resolve every source.
    Update(Vec<String>),
    /// Verify the lock is satisfied with ZERO network fetch; fail-closed if unsatisfied.
    Locked,
}

impl LockMode {
    /// Whether this mode permits any network fetch. `Locked` is the only zero-network mode.
    pub fn allows_fetch(&self) -> bool {
        !matches!(self, LockMode::Locked)
    }

    /// Whether a source named `source_url` providing `skill` should be re-resolved under
    /// this mode. `Plain` re-resolves all; `Update(names)` only sources whose tracked
    /// skills intersect `names`; `Locked` never re-resolves.
    pub fn should_resolve(&self, source_url: &str, prev: &AgentLockFile) -> bool {
        match self {
            LockMode::Plain => true,
            LockMode::Locked => false,
            LockMode::Update(names) => {
                if names.is_empty() {
                    return true;
                }
                prev.skills
                    .values()
                    .any(|e| e.source == source_url && names.contains(&e.skill))
            }
        }
    }
}

/// One drift change between two lock snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockDrift {
    pub status: DriftStatus,
    pub id: String,
}

/// The kind of drift for a single lock entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftStatus {
    Added,
    Removed,
    Updated,
}

impl DriftStatus {
    pub fn label(self) -> &'static str {
        match self {
            DriftStatus::Added => "added",
            DriftStatus::Removed => "removed",
            DriftStatus::Updated => "updated",
        }
    }
}

impl AgentLockFile {
    pub fn get_tracked_asset(&self, kind: &str, id: &str) -> Option<(String, String)> {
        self.assets.get(id).and_then(|a| {
            if a.kind == kind {
                Some((a.hash.clone(), a.destination.clone()))
            } else {
                None
            }
        })
    }

    pub fn save_tracked_asset(&mut self, id: &str, entry: AssetEntry) {
        self.assets.insert(id.to_string(), entry);
    }

    pub fn remove_tracked_asset(&mut self, id: &str) {
        self.assets.remove(id);
    }

    pub fn list_tracked_asset_ids(&self, kind: &str) -> Vec<(&str, &str)> {
        self.assets
            .iter()
            .filter(|(_, a)| a.kind == kind)
            .map(|(id, a)| (id.as_str(), a.destination.as_str()))
            .collect()
    }

    pub fn clear_all(&mut self) {
        self.skills.clear();
        self.assets.clear();
    }

    /// Compute drift between this (previous on-disk) lock and a freshly-resolved `next` lock.
    /// Deterministic order via BTreeMap iteration. The basis of `lock --check` (CI drift,
    /// no mutation): a non-empty result means the on-disk lock is out of date.
    pub fn lock_check(&self, next: &AgentLockFile) -> Vec<LockDrift> {
        let mut out = Vec::new();
        for (id, prev) in &self.skills {
            match next.skills.get(id) {
                None => out.push(LockDrift {
                    status: DriftStatus::Removed,
                    id: id.clone(),
                }),
                Some(now)
                    if now.hash != prev.hash || now.source_revision != prev.source_revision =>
                {
                    out.push(LockDrift {
                        status: DriftStatus::Updated,
                        id: id.clone(),
                    });
                }
                _ => {}
            }
        }
        for id in next.skills.keys() {
            if !self.skills.contains_key(id) {
                out.push(LockDrift {
                    status: DriftStatus::Added,
                    id: id.clone(),
                });
            }
        }
        for (id, prev) in &self.assets {
            if let Some(now) = next.assets.get(id) {
                if now.source_revision != prev.source_revision {
                    out.push(LockDrift {
                        status: DriftStatus::Updated,
                        id: id.clone(),
                    });
                }
            }
        }
        out
    }
}

/// Resolve the standalone lock file path for the given scope.
/// `Project` → `<project_root>/<LOCK_FILENAME>`, `Global` → `<global_data_dir>/<LOCK_FILENAME>`.
pub fn lock_path(scope: Scope, project_root: &Path, global_data_dir: &Path) -> PathBuf {
    match scope {
        Scope::Project => project_root.join(LOCK_FILENAME),
        Scope::Global => global_data_dir.join(LOCK_FILENAME),
    }
}

/// Load the lock file from `path` (or return a default empty one if missing / empty).
pub fn load(path: &Path) -> Result<AgentLockFile> {
    if !path.exists() {
        return Ok(AgentLockFile::default());
    }
    let text = fs::read_to_string(path)
        .map_err(|e| err(format!("failed to read lock file {}: {e}", path.display())))?;
    if text.trim().is_empty() {
        return Ok(AgentLockFile::default());
    }
    let lock: AgentLockFile = serde_yaml::from_str(&text)
        .map_err(|e| err(format!("failed to parse lock file {}: {e}", path.display())))?;
    Ok(lock)
}

/// Write the lock file to `path`, creating parent directories if needed. Stamps the current
/// schema version so a migrated older lock is relabeled (legacy restamp).
pub fn save(lock: &mut AgentLockFile, path: &Path) -> Result<()> {
    lock.version = LOCK_VERSION;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let yaml = serde_yaml::to_string(lock)
        .map_err(|e| err(format!("failed to serialize lock file: {e}")))?;
    fs::write(path, yaml)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn test_asset(kind: &str, name: &str, destination: &str) -> AssetEntry {
        AssetEntry {
            kind: kind.into(),
            name: name.into(),
            hash: "h".into(),
            source: "s".into(),
            destination: destination.into(),
            source_revision: "rev".into(),
        }
    }

    fn skill_entry(dest: &str, hash: &str, rev: &str) -> AgentLockEntry {
        AgentLockEntry {
            destination: dest.into(),
            hash: hash.into(),
            skill: "skill-a".into(),
            description: "desc".into(),
            source: "src".into(),
            source_revision: rev.into(),
            scope: Some(Scope::Project),
        }
    }

    #[test]
    fn round_trip_with_skills_and_assets() {
        let dir = temp_dir("agent-env-lock-data");
        let path = lock_path(Scope::Project, &dir, &dir);

        let mut lock = AgentLockFile::default();
        lock.skills.insert(
            "src::skill-a".into(),
            skill_entry(".claude/skills/skill-a", "abc", "rev1"),
        );
        lock.save_tracked_asset(
            "mcp::src::pack.json",
            AssetEntry {
                kind: "mcp".into(),
                name: "pack.json".into(),
                hash: "h1".into(),
                source: "src".into(),
                destination: "srv1,srv2".into(),
                source_revision: "rev1".into(),
            },
        );

        save(&mut lock, &path).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(loaded.version, 2);
        assert_eq!(loaded.skills.len(), 1);
        assert_eq!(loaded.skills["src::skill-a"].hash, "abc");
        // scope-relative destination round-trips verbatim
        assert_eq!(
            loaded.skills["src::skill-a"].destination,
            ".claude/skills/skill-a"
        );
        assert_eq!(
            loaded.get_tracked_asset("mcp", "mcp::src::pack.json"),
            Some(("h1".into(), "srv1,srv2".into()))
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_returns_default_when_missing() {
        let dir = temp_dir("agent-env-lock-missing");
        let lock = load(&dir.join("nope.lock")).unwrap();
        assert_eq!(lock.version, 2);
        assert!(lock.skills.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_v1_lock_loads_and_restamps_on_save() {
        let dir = temp_dir("agent-env-lock-legacy");
        let path = dir.join(LOCK_FILENAME);

        // A v1 lock carrying fields that no longer exist plus an absolute destination.
        // Unknown fields must be ignored, absolute paths honored, version relabeled on save.
        let legacy = "version: 1\n\
last_run: '111'\n\
skills:\n\
\x20 src::a:\n\
\x20\x20\x20 destination: /abs/path/.claude/skills/a\n\
\x20\x20\x20 hash: h\n\
\x20\x20\x20 skill: a\n\
\x20\x20\x20 source: src\n\
\x20\x20\x20 source_revision: local\n\
\x20\x20\x20 updated_at: '111'\n\
assets: {}\n";
        fs::write(&path, legacy).unwrap();

        let mut loaded = load(&path).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.skills.len(), 1);
        assert_eq!(
            loaded.skills["src::a"].destination,
            "/abs/path/.claude/skills/a"
        );

        save(&mut loaded, &path).unwrap();
        let resaved = fs::read_to_string(&path).unwrap();
        assert!(resaved.starts_with("version: 2"));
        assert!(!resaved.contains("last_run"));
        assert!(!resaved.contains("updated_at"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_check_reports_added_removed_updated() {
        let mut prev = AgentLockFile::default();
        prev.skills
            .insert("k::keep".into(), skill_entry("d", "h1", "r1"));
        prev.skills
            .insert("k::gone".into(), skill_entry("d", "h2", "r1"));

        let mut next = AgentLockFile::default();
        // keep unchanged
        next.skills
            .insert("k::keep".into(), skill_entry("d", "h1", "r1"));
        // gone removed; new added
        next.skills
            .insert("k::new".into(), skill_entry("d", "h3", "r1"));

        let drift = prev.lock_check(&next);
        let removed = drift
            .iter()
            .any(|d| d.status == DriftStatus::Removed && d.id == "k::gone");
        let added = drift
            .iter()
            .any(|d| d.status == DriftStatus::Added && d.id == "k::new");
        assert!(removed && added, "drift: {drift:?}");
        // no spurious "keep" drift
        assert!(!drift.iter().any(|d| d.id == "k::keep"));
    }

    #[test]
    fn lock_check_flags_hash_and_revision_change() {
        let mut prev = AgentLockFile::default();
        prev.skills
            .insert("k::a".into(), skill_entry("d", "h1", "r1"));
        let mut next_hash = AgentLockFile::default();
        next_hash
            .skills
            .insert("k::a".into(), skill_entry("d", "h2", "r1"));
        assert_eq!(prev.lock_check(&next_hash)[0].status, DriftStatus::Updated);

        let mut next_rev = AgentLockFile::default();
        next_rev
            .skills
            .insert("k::a".into(), skill_entry("d", "h1", "r2"));
        assert_eq!(prev.lock_check(&next_rev)[0].status, DriftStatus::Updated);

        let mut same = AgentLockFile::default();
        same.skills
            .insert("k::a".into(), skill_entry("d", "h1", "r1"));
        assert!(prev.lock_check(&same).is_empty());
    }

    #[test]
    fn locked_mode_is_zero_network_and_never_resolves() {
        let mut prev = AgentLockFile::default();
        prev.skills
            .insert("src::a".into(), skill_entry("d", "h", "r"));

        assert!(!LockMode::Locked.allows_fetch());
        assert!(!LockMode::Locked.should_resolve("src", &prev));

        // Plain fetches + resolves everything.
        assert!(LockMode::Plain.allows_fetch());
        assert!(LockMode::Plain.should_resolve("src", &prev));
        assert!(LockMode::Plain.should_resolve("other", &prev));
    }

    #[test]
    fn update_mode_selective_resolve() {
        let mut prev = AgentLockFile::default();
        prev.skills.insert(
            "src::skill-a".into(),
            AgentLockEntry {
                skill: "skill-a".into(),
                source: "src".into(),
                ..Default::default()
            },
        );
        // Update of a named package re-resolves only its source; others carry over.
        let mode = LockMode::Update(vec!["skill-a".into()]);
        assert!(mode.allows_fetch());
        assert!(mode.should_resolve("src", &prev));
        assert!(!mode.should_resolve("unrelated-source", &prev));
        // Empty update list re-resolves all.
        assert!(LockMode::Update(vec![]).should_resolve("anything", &prev));
    }

    #[test]
    fn asset_helpers_filter_and_remove_by_kind() {
        let mut lock = AgentLockFile::default();
        lock.save_tracked_asset("mcp::a", test_asset("mcp", "a", "d1"));
        lock.save_tracked_asset("other::b", test_asset("other", "b", "d2"));

        let mcps = lock.list_tracked_asset_ids("mcp");
        assert_eq!(mcps, vec![("mcp::a", "d1")]);

        lock.remove_tracked_asset("mcp::a");
        assert!(lock.get_tracked_asset("mcp", "mcp::a").is_none());

        lock.clear_all();
        assert!(lock.assets.is_empty());
    }
}
