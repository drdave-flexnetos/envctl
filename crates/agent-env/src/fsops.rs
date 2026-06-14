//! Filesystem helpers for agent settings files — ported verbatim from kasetto v3.2.0
//! `src/fsops/settings.rs` (ledger F-04).
//!
//! [`SettingsFile`] is the load → mutate → save wrapper the MCP merge engine ([`crate::mcp`])
//! drives over JSON-based agent config files. It is non-printing and returns
//! [`crate::Result`]; kasetto's box-error `?` conversions map onto [`crate::AgentEnvError`]
//! ([`AgentEnvError::Json`](crate::AgentEnvError::Json) /
//! [`AgentEnvError::Io`](crate::AgentEnvError::Io)).

use std::fs;
use std::path::{Path, PathBuf};

use crate::{err, Result};

/// Wrapper for loading, mutating, and saving agent settings JSON files.
pub struct SettingsFile {
    path: PathBuf,
    /// The parsed JSON document; callers mutate this in place before [`SettingsFile::save`].
    pub data: serde_json::Value,
}

impl SettingsFile {
    /// Load an existing JSON file or start with an empty `{}`.
    pub fn load(path: &Path) -> Result<Self> {
        let data = if path.exists() {
            let text = fs::read_to_string(path)?;
            serde_json::from_str(&text)
                .map_err(|e| err(format!("invalid settings JSON {}: {e}", path.display())))?
        } else {
            serde_json::json!({})
        };
        Ok(Self {
            path: path.to_path_buf(),
            data,
        })
    }

    /// Write pretty-printed JSON back to disk, creating parent dirs if needed.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(&self.data)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn load_missing_file_starts_empty_object() {
        let path = temp_dir("agent-env-settings-missing").join("settings.json");
        let sf = SettingsFile::load(&path).expect("load");
        assert!(sf.data.is_object());
        assert_eq!(sf.data.as_object().unwrap().len(), 0);
    }

    #[test]
    fn load_existing_file_parses_data() {
        let dir = temp_dir("agent-env-settings-load");
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"a":{"b":1}}"#).unwrap();
        let sf = SettingsFile::load(&path).expect("load");
        assert_eq!(sf.data["a"]["b"], 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_invalid_json_errors() {
        let dir = temp_dir("agent-env-settings-bad");
        let path = dir.join("settings.json");
        fs::write(&path, "{not json").unwrap();
        let res = SettingsFile::load(&path);
        assert!(res.is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_writes_pretty_json_and_creates_parents() {
        let dir = temp_dir("agent-env-settings-save");
        let path = dir.join("nested").join("settings.json");
        let mut sf = SettingsFile::load(&path).expect("load");
        sf.data = serde_json::json!({"x": 42});
        sf.save().expect("save");
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["x"], 42);
        let _ = fs::remove_dir_all(&dir);
    }
}
