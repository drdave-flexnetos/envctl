//! Default config-path resolution — ported verbatim from kasetto v3.2.0 `src/lib.rs`
//! (ledger CP-01): [`default_config_path`], the testable pure core [`resolve_config_path`],
//! and the [`Preferences`] file shape.
//!
//! Resolution priority (when no explicit `--config` is given), unchanged from kasetto:
//! 1. the `$ENVCTL_AGENT_CONFIG` env var (renamed from `$KASETTO_CONFIG`);
//! 2. `./agent-env.yaml` (renamed from `./kasetto.yaml`) — local project config;
//! 3. the `source:` key in `$XDG_CONFIG_HOME/agent-env/config.yaml` (the preferences file);
//! 4. `$XDG_CONFIG_HOME/agent-env/agent-env.yaml` (the global config, renamed from `kasetto.yaml`);
//! 5. fall back to `./agent-env.yaml`.
//!
//! ## kasetto → envctl renames (the absorbed tool's OWN identity — SHOULD be renamed)
//! These names are kasetto's product self-identity, not agent-native paths, so they are
//! renamed to envctl-neutral equivalents:
//! - env var `KASETTO_CONFIG` → `ENVCTL_AGENT_CONFIG`
//! - local config filename `kasetto.yaml` → `agent-env.yaml`
//! - global config filename `kasetto.yaml` → `agent-env.yaml`
//! - per-product config dir `kasetto/` → `agent-env/` (via [`crate::dirs::dirs_agent_env_config`])
//!
//! The preferences filename (`config.yaml`) is a generic leaf under the product dir and is kept
//! as-is. kasetto's `crate::fsops::dirs_kasetto_config` → [`crate::dirs::dirs_agent_env_config`].

use crate::dirs::dirs_agent_env_config;

/// Default config file in the current directory when `--config` is omitted.
pub const DEFAULT_CONFIG_FILENAME: &str = "agent-env.yaml";
/// Default config file under the agent-env XDG config directory (`init --global` writes here).
pub const DEFAULT_GLOBAL_CONFIG_FILENAME: &str = "agent-env.yaml";
/// agent-env preferences file under the XDG config directory.
/// May contain a `source:` key pointing to a remote or absolute config path.
pub const PREFERENCES_FILENAME: &str = "config.yaml";
/// Environment variable that, when set and non-empty, overrides every other config-path source.
pub const CONFIG_ENV_VAR: &str = "ENVCTL_AGENT_CONFIG";

/// The preferences-file shape: an optional `source:` pointing to a remote or absolute config.
#[derive(serde::Deserialize)]
pub struct Preferences {
    pub source: Option<String>,
}

/// Resolve the default config path used when `--config` is omitted.
///
/// Priority:
/// 1. `$ENVCTL_AGENT_CONFIG` env var
/// 2. `./agent-env.yaml` (local project config)
/// 3. `source:` key in `$XDG_CONFIG_HOME/agent-env/config.yaml` (preferences file)
/// 4. `$XDG_CONFIG_HOME/agent-env/agent-env.yaml` (global config)
/// 5. `./agent-env.yaml` fallback
pub fn default_config_path() -> String {
    let env_var = std::env::var(CONFIG_ENV_VAR).ok().filter(|v| !v.is_empty());
    let prefs_path = dirs_agent_env_config()
        .ok()
        .map(|d| d.join(PREFERENCES_FILENAME));
    let local_exists = std::path::Path::new(DEFAULT_CONFIG_FILENAME).exists();
    let global_path = dirs_agent_env_config()
        .ok()
        .map(|d| d.join(DEFAULT_GLOBAL_CONFIG_FILENAME));
    resolve_config_path(
        env_var,
        prefs_path.as_deref(),
        local_exists,
        global_path.as_deref(),
    )
}

/// Pure resolution core (no env/cwd access) — the testable heart of [`default_config_path`].
pub fn resolve_config_path(
    env_var: Option<String>,
    prefs_path: Option<&std::path::Path>,
    local_exists: bool,
    global_path: Option<&std::path::Path>,
) -> String {
    if let Some(v) = env_var {
        return v;
    }

    if local_exists {
        return DEFAULT_CONFIG_FILENAME.to_string();
    }

    if let Some(path) = prefs_path {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(prefs) = serde_yaml::from_str::<Preferences>(&text) {
                if let Some(cfg) = prefs.source {
                    return cfg;
                }
            }
        }
    }

    if let Some(global) = global_path {
        if global.exists() {
            return global.to_string_lossy().to_string();
        }
    }

    DEFAULT_CONFIG_FILENAME.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Serializes the one test that pokes the process-global `ENVCTL_AGENT_CONFIG` env var
    /// against the `dirs`/`runtime` env tests (Y-gate race-safety).
    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn env_var_takes_highest_priority() {
        let result = resolve_config_path(
            Some("https://example.com/team.yaml".into()),
            None,
            true,
            None,
        );
        assert_eq!(result, "https://example.com/team.yaml");
    }

    #[test]
    fn preferences_file_source_used_when_no_env_var() {
        let dir = temp_dir("agent-env-prefs");
        fs::create_dir_all(&dir).unwrap();
        let prefs = dir.join("config.yaml");
        fs::write(&prefs, "source: https://example.com/remote.yaml\n").unwrap();

        let result = resolve_config_path(None, Some(&prefs), false, None);
        assert_eq!(result, "https://example.com/remote.yaml");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_var_beats_preferences_file() {
        let dir = temp_dir("agent-env-prefs-priority");
        fs::create_dir_all(&dir).unwrap();
        let prefs = dir.join("config.yaml");
        fs::write(&prefs, "source: https://example.com/prefs.yaml\n").unwrap();

        let result = resolve_config_path(
            Some("https://example.com/env.yaml".into()),
            Some(&prefs),
            false,
            None,
        );
        assert_eq!(result, "https://example.com/env.yaml");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_agent_env_yaml_beats_preferences_source() {
        let dir = temp_dir("agent-env-local-beats-prefs");
        fs::create_dir_all(&dir).unwrap();
        let prefs = dir.join("config.yaml");
        fs::write(&prefs, "source: https://example.com/prefs.yaml\n").unwrap();

        let result = resolve_config_path(None, Some(&prefs), true, None);
        assert_eq!(result, DEFAULT_CONFIG_FILENAME);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_agent_env_yaml_used_when_no_env_or_prefs() {
        let result = resolve_config_path(None, None, true, None);
        assert_eq!(result, DEFAULT_CONFIG_FILENAME);
    }

    #[test]
    fn global_config_used_when_local_absent() {
        let dir = temp_dir("agent-env-global");
        fs::create_dir_all(&dir).unwrap();
        let global = dir.join("agent-env.yaml");
        fs::write(&global, "agent: claude-code\nskills: []\n").unwrap();

        let result = resolve_config_path(None, None, false, Some(&global));
        assert_eq!(result, global.to_string_lossy());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_to_local_filename_when_nothing_exists() {
        let result = resolve_config_path(None, None, false, None);
        assert_eq!(result, DEFAULT_CONFIG_FILENAME);
    }

    #[test]
    fn missing_prefs_file_is_skipped_silently() {
        let dir = temp_dir("agent-env-no-prefs");
        let missing = dir.join("config.yaml");

        let result = resolve_config_path(None, Some(&missing), true, None);
        assert_eq!(result, DEFAULT_CONFIG_FILENAME);
    }

    #[test]
    fn prefs_file_without_source_key_is_skipped() {
        let dir = temp_dir("agent-env-prefs-no-source");
        fs::create_dir_all(&dir).unwrap();
        let prefs = dir.join("config.yaml");
        fs::write(&prefs, "some_other_key: value\n").unwrap();

        let result = resolve_config_path(None, Some(&prefs), true, None);
        assert_eq!(result, DEFAULT_CONFIG_FILENAME);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_config_path_honors_renamed_env_override() {
        let _g = env_lock();
        let prev = std::env::var(CONFIG_ENV_VAR).ok();
        std::env::set_var(CONFIG_ENV_VAR, "https://example.com/from-env.yaml");
        assert_eq!(default_config_path(), "https://example.com/from-env.yaml");
        match prev {
            Some(v) => std::env::set_var(CONFIG_ENV_VAR, v),
            None => std::env::remove_var(CONFIG_ENV_VAR),
        }
    }
}
