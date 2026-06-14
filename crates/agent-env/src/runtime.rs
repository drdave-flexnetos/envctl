//! Machine-local agent-asset **runtime state** — ported verbatim from kasetto v3.2.0
//! `src/state.rs` (ledger ST-01/ST-02).
//!
//! This is the per-machine, run-specific payload kept *out* of the committed
//! `agent-env.lock` so the lock stays portable across machines and users (mirroring how
//! `uv` keeps machine state in its cache directory, separate from `uv.lock`). Everything
//! here is regenerated on the next sync; the file is safe to delete.
//!
//! Scope note: this is a **separate** runtime payload from any engine runtime — it is the
//! agent-asset runtime state (last-run stamp, cached `Report` JSON, per-asset install
//! timestamps). The kasetto-self-named cache path (`dirs_kasetto_cache`) is renamed to the
//! envctl-neutral [`crate::dirs::dirs_agent_env_cache`]; the lock-keyed file layout is verbatim.
//!
//! Integration adaptations (no behavior change):
//! - `crate::error::{err, Result}` → [`crate::err`] / [`crate::Result`].
//! - `crate::model::{Scope, SyncFailure}` → [`crate::config::Scope`] / [`crate::report::SyncFailure`].
//! - `crate::fsops::{dirs_kasetto_cache, hash_str}` → [`crate::dirs::dirs_agent_env_cache`] /
//!   [`crate::hash::hash_str`].
//! - kasetto's `lock_path(scope, project_root)?` is a `Result` keyed only on the project root;
//!   agent-env's [`crate::lock::lock_path`] is infallible but takes the global-data dir
//!   explicitly, so the global-data dir is resolved here from [`crate::dirs::dirs_agent_env_data`]
//!   (the same XDG-derived location the lock layer writes to).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Scope;
use crate::dirs::{dirs_agent_env_cache, dirs_agent_env_data};
use crate::hash::hash_str;
use crate::lock::lock_path;
use crate::report::SyncFailure;
use crate::{err, Result};

/// Machine-local, run-specific state kept *out* of the committed `agent-env.lock`
/// so the lock stays portable across machines and users. This mirrors how `uv`
/// keeps machine state in its cache directory, separate from `uv.lock`.
///
/// Everything here is regenerated on the next sync; the file is safe to delete.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RuntimeState {
    /// Unix timestamp of the last successful sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    /// Serialized JSON of the most recent sync `Report` (used by `doctor`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_report: Option<String>,
    /// `entry id -> unix timestamp` of when this machine last installed/updated
    /// each skill. Drives the "updated N ago" display in `list`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub installed_at: BTreeMap<String, String>,
}

impl RuntimeState {
    pub fn updated_at(&self, id: &str) -> String {
        self.installed_at.get(id).cloned().unwrap_or_default()
    }

    pub fn set_updated_at(&mut self, id: &str, ts: String) {
        self.installed_at.insert(id.to_string(), ts);
    }

    pub fn forget(&mut self, id: &str) {
        self.installed_at.remove(id);
    }

    pub fn save_report_json(&mut self, report_json: &str) {
        self.latest_report = Some(report_json.to_string());
    }

    /// Extract failed actions from the cached report for `doctor`.
    pub fn load_latest_failures(&self) -> Vec<SyncFailure> {
        let Some(report_json) = &self.latest_report else {
            return Vec::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(report_json) else {
            return Vec::new();
        };
        let mut failed = Vec::new();
        if let Some(actions) = value.get("actions").and_then(|v| v.as_array()) {
            for action in actions {
                let status = action.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if status != "broken" && status != "source_error" {
                    continue;
                }
                failed.push(SyncFailure {
                    name: action
                        .get("skill")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string(),
                    source: action
                        .get("source")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string(),
                    reason: action
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown reason")
                        .to_string(),
                });
            }
        }
        failed
    }
}

/// Machine-local state file path, keyed by the lock file location so project
/// and global scopes (and distinct projects) never collide. Lives in the cache
/// directory and is never committed.
pub fn runtime_state_path(scope: Scope, project_root: &Path) -> Result<PathBuf> {
    let lock = lock_path(scope, project_root, &dirs_agent_env_data()?);
    let key = hash_str(&lock.to_string_lossy());
    Ok(dirs_agent_env_cache()?
        .join("runtime")
        .join(format!("{key}.json")))
}

pub fn load_runtime_state(scope: Scope, project_root: &Path) -> Result<RuntimeState> {
    let path = runtime_state_path(scope, project_root)?;
    if !path.exists() {
        return Ok(RuntimeState::default());
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| err(format!("failed to read state file {}: {e}", path.display())))?;
    if text.trim().is_empty() {
        return Ok(RuntimeState::default());
    }
    serde_json::from_str(&text).map_err(|e| {
        err(format!(
            "failed to parse state file {}: {e}",
            path.display()
        ))
    })
}

pub fn save_runtime_state(state: &RuntimeState, scope: Scope, project_root: &Path) -> Result<()> {
    let path = runtime_state_path(scope, project_root)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| err(format!("failed to serialize state file: {e}")))?;
    fs::write(&path, json)?;
    Ok(())
}

pub fn clear_runtime_state(scope: Scope, project_root: &Path) -> Result<()> {
    let path = runtime_state_path(scope, project_root)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| {
            err(format!(
                "failed to remove state file {}: {e}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// The round-trip test sets `HOME`/`XDG_*_HOME` (consumed by `dirs_agent_env_*`), which is
    /// process-global; serialize it against the `dirs` env-poking tests via a shared lock so
    /// parallel threads never observe each other's overrides. (Race-safety requirement of the
    /// Y-gate: HOME/env-mutating tests must not corrupt sibling tests.)
    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    fn unique_root(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn round_trip_runtime_state() {
        let _g = env_lock();
        let home = unique_root("agent-env-state-home");
        let cache = unique_root("agent-env-state-cache");
        let data = unique_root("agent-env-state-data");
        // Pin every XDG base the runtime path touches so the test is hermetic.
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CACHE_HOME", &cache);
        std::env::set_var("XDG_DATA_HOME", &data);
        let root = unique_root("agent-env-state-proj");
        fs::create_dir_all(&root).unwrap();

        let mut state = RuntimeState {
            last_run: Some("123".into()),
            ..Default::default()
        };
        state.set_updated_at("src::a", "100".into());
        state.save_report_json(r#"{"actions":[]}"#);

        save_runtime_state(&state, Scope::Project, &root).unwrap();
        let loaded = load_runtime_state(Scope::Project, &root).unwrap();

        assert_eq!(loaded.last_run.as_deref(), Some("123"));
        assert_eq!(loaded.updated_at("src::a"), "100");
        assert_eq!(loaded.updated_at("missing"), "");

        clear_runtime_state(Scope::Project, &root).unwrap();
        assert!(load_runtime_state(Scope::Project, &root)
            .unwrap()
            .last_run
            .is_none());

        std::env::remove_var("XDG_CACHE_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&cache);
        let _ = fs::remove_dir_all(&data);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn forget_drops_install_stamp() {
        let mut state = RuntimeState::default();
        state.set_updated_at("src::a", "100".into());
        state.set_updated_at("src::b", "200".into());
        state.forget("src::a");
        assert_eq!(state.updated_at("src::a"), "");
        assert_eq!(state.updated_at("src::b"), "200");
    }

    #[test]
    fn load_latest_failures_extracts_failed_actions() {
        let mut state = RuntimeState::default();
        state.save_report_json(
            r#"{"actions":[
                {"status":"installed","skill":"good","source":"s"},
                {"status":"broken","skill":"bad","source":"s","error":"missing"},
                {"status":"source_error","skill":"err","source":"s2","error":"timeout"}
            ]}"#,
        );
        let failures = state.load_latest_failures();
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].name, "bad");
        assert_eq!(failures[1].reason, "timeout");
    }

    #[test]
    fn load_latest_failures_empty_without_report() {
        let state = RuntimeState::default();
        assert!(state.load_latest_failures().is_empty());
    }
}
