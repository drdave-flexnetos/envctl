//! YAML-level `extends` config inheritance — consolidates kasetto v3.2.0
//! `src/model/extend.rs` + the recursive loader in `src/fsops/config.rs`.
//!
//! `extends` is stripped at the `serde_yaml::Value` layer before deserialization, then each
//! parent config is loaded recursively and merged. Top-level scalar fields (`destination`,
//! `scope`, `agent`) replace; `skills` / `mcps` / `commands` lists merge by an identity tuple
//! `(source, ref|branch, sub-dir)` — same identity replaces, otherwise appends.
//!
//! Two fail-closed guards bound the recursion: a **cycle guard** (a config that transitively
//! extends itself errors) and a **depth guard** ([`MAX_EXTENDS_DEPTH`]).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Value};

use crate::config::Config;
use crate::source::{
    auth_env_inline_help, auth_for_request_url, http_client, http_fetch_auth_hint,
    rewrite_browse_to_raw_url,
};
use crate::{err, Result};

/// Maximum `extends` recursion depth before the load fails closed.
pub const MAX_EXTENDS_DEPTH: u8 = 8;

/// Strip and return the `extends` field from a config Value.
/// Accepts a single string or a sequence of strings.
pub fn extract_extends(v: &mut Value) -> Vec<String> {
    let Value::Mapping(map) = v else {
        return Vec::new();
    };
    let Some(raw) = map.remove("extends") else {
        return Vec::new();
    };
    match raw {
        Value::String(s) => vec![s],
        Value::Sequence(seq) => seq
            .into_iter()
            .filter_map(|item| match item {
                Value::String(s) => Some(s),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Merge `overlay` on top of `base`. Both should be top-level config mappings.
/// Returns `overlay` unchanged if either side is not a mapping.
pub fn merge_yaml(base: Value, overlay: Value) -> Value {
    let Value::Mapping(mut out) = base else {
        return overlay;
    };
    let overlay_map = match overlay {
        Value::Mapping(m) => m,
        other => return other,
    };
    for (key, ov_val) in overlay_map {
        let key_str = key.as_str().unwrap_or("");
        let is_source_list = matches!(key_str, "skills" | "mcps" | "commands");
        match (is_source_list.then(|| out.remove(&key)).flatten(), ov_val) {
            (Some(Value::Sequence(base_seq)), Value::Sequence(ov_seq)) => {
                out.insert(key, Value::Sequence(merge_source_list(base_seq, ov_seq)));
            }
            (_, ov) => {
                out.insert(key, ov);
            }
        }
    }
    Value::Mapping(out)
}

/// Identity-aware merge for skills/mcps/commands lists.
/// Identity = `(source, ref|branch|"", sub_dir|"")`.
/// Same-identity entries are replaced wholesale by overlay; new entries appended.
fn merge_source_list(base: Vec<Value>, overlay: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = base;
    for ov in overlay {
        let ov_id = identity_of(&ov);
        if let Some(pos) = out.iter().position(|b| identity_of(b) == ov_id) {
            out[pos] = ov;
        } else {
            out.push(ov);
        }
    }
    out
}

fn identity_of(entry: &Value) -> (String, String, String) {
    let Value::Mapping(m) = entry else {
        return (String::new(), String::new(), String::new());
    };
    let source = string_field(m, "source").unwrap_or_default();
    let pin = string_field(m, "ref")
        .or_else(|| string_field(m, "branch"))
        .unwrap_or_default();
    let sub_dir = string_field(m, "sub-dir")
        .or_else(|| string_field(m, "sub_dir"))
        .unwrap_or_default();
    (source, pin, sub_dir)
}

fn string_field(m: &Mapping, key: &str) -> Option<String> {
    m.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Where a config came from — used to resolve relative `extends` paths and detect cycles.
struct ConfigOrigin {
    /// Canonical identifier (absolute path or full URL) for cycle detection.
    canonical_id: String,
    /// Directory used to resolve relative `extends` references in this config.
    /// `None` for HTTP origins (relative extends are an error).
    base_dir: Option<PathBuf>,
    /// Human-readable label for error messages.
    label: String,
}

/// Load a config (local path or `http(s)://` URL), resolving + merging `extends` parents
/// recursively, then deserialize into a [`Config`]. Returns the config, its base directory,
/// and a human-readable label.
pub fn load_config_any(config_path: &str) -> Result<(Config, PathBuf, String)> {
    let mut visited = HashSet::new();
    let (merged, origin) = load_value_recursive(config_path, None, &mut visited, 0)?;
    let cfg: Config = serde_yaml::from_value(merged)
        .map_err(|e| err(format!("failed to parse config {}: {e}", origin.label)))?;
    let cfg_dir = match origin.base_dir {
        Some(dir) => dir,
        None => std::env::current_dir()
            .map_err(|e| err(format!("failed to get current directory: {e}")))?,
    };
    Ok((cfg, cfg_dir, origin.label))
}

/// Recursively load + merge a config ref and its `extends` parents into one `serde_yaml::Value`.
///
/// Fail-closed: errors past [`MAX_EXTENDS_DEPTH`] (depth guard) or on a repeated
/// `canonical_id` in the current chain (cycle guard).
pub fn load_config_recursive(
    config_ref: &str,
    parent_base_dir: Option<&Path>,
    visited: &mut HashSet<String>,
    depth: u8,
) -> Result<(Value, PathBuf, String)> {
    let (value, origin) = load_value_recursive(config_ref, parent_base_dir, visited, depth)?;
    let base_dir = origin.base_dir.clone().unwrap_or_default();
    Ok((value, base_dir, origin.label))
}

fn load_value_recursive(
    config_ref: &str,
    parent_base_dir: Option<&Path>,
    visited: &mut HashSet<String>,
    depth: u8,
) -> Result<(Value, ConfigOrigin)> {
    if depth > MAX_EXTENDS_DEPTH {
        return Err(err(format!(
            "extends depth limit exceeded ({MAX_EXTENDS_DEPTH}) at {config_ref}"
        )));
    }

    let (text, origin) = fetch_config_text(config_ref, parent_base_dir)?;
    if !visited.insert(origin.canonical_id.clone()) {
        return Err(err(format!(
            "circular extends detected involving {}",
            origin.label
        )));
    }

    let mut value: Value = serde_yaml::from_str(&text)
        .map_err(|e| err(format!("failed to parse config {}: {e}", origin.label)))?;
    let parents = extract_extends(&mut value);

    let mut merged: Value = Value::Mapping(Default::default());
    for parent_ref in &parents {
        let mut parent_visited = visited.clone();
        let (parent_value, _parent_origin) = load_value_recursive(
            parent_ref,
            origin.base_dir.as_deref(),
            &mut parent_visited,
            depth + 1,
        )
        .map_err(|e| {
            err(format!(
                "failed to load extended config '{parent_ref}' (extended from {}): {e}",
                origin.label
            ))
        })?;
        merged = merge_yaml(merged, parent_value);
    }
    let final_value = merge_yaml(merged, value);

    visited.remove(&origin.canonical_id);
    Ok((final_value, origin))
}

fn fetch_config_text(
    config_ref: &str,
    parent_base_dir: Option<&Path>,
) -> Result<(String, ConfigOrigin)> {
    if config_ref.starts_with("http://") || config_ref.starts_with("https://") {
        let fetch_url =
            rewrite_browse_to_raw_url(config_ref).unwrap_or_else(|| config_ref.to_string());
        let auth = auth_for_request_url(&fetch_url);
        let request = auth.apply(http_client()?.get(&fetch_url));
        let response = request
            .send()
            .map_err(|e| err(format!("failed to fetch remote config: {config_ref}: {e}")))?;
        let status = response.status().as_u16();
        let text = response.text().map_err(|e| {
            err(format!(
                "failed to read remote config body for {config_ref}: {e}"
            ))
        })?;
        if !(200..300).contains(&status) {
            return Err(err(format!(
                "remote config returned HTTP {status} for {config_ref}{}",
                http_fetch_auth_hint(config_ref, status)
            )));
        }
        if text.trim_start().starts_with("<!DOCTYPE") || text.trim_start().starts_with("<html") {
            return Err(err(format!(
                "remote config at {config_ref} returned a login/HTML page instead of YAML - {}",
                auth_env_inline_help(config_ref)
            )));
        }
        return Ok((
            text,
            ConfigOrigin {
                canonical_id: fetch_url,
                base_dir: None,
                label: config_ref.to_string(),
            },
        ));
    }

    let path = PathBuf::from(config_ref);
    let resolved = if path.is_absolute() {
        path
    } else if let Some(base) = parent_base_dir {
        base.join(path)
    } else {
        path
    };
    let cfg_abs = fs::canonicalize(&resolved).map_err(|_| {
        err(format!(
            "config not found: {} (resolved to {})",
            config_ref,
            resolved.display()
        ))
    })?;
    let cfg_text = fs::read_to_string(&cfg_abs)?;
    let cfg_dir = cfg_abs
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| err("invalid config path"))?;
    let label = cfg_abs.to_string_lossy().to_string();
    Ok((
        cfg_text,
        ConfigOrigin {
            canonical_id: label.clone(),
            base_dir: Some(cfg_dir),
            label,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Scope, SkillsField};

    fn yaml(s: &str) -> Value {
        serde_yaml::from_str(s).expect("parse yaml")
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&d).unwrap();
        d
    }

    // --- extract_extends ---

    #[test]
    fn extract_extends_string_and_removes_field() {
        let mut v = yaml("extends: ../base.yaml\nskills: []\n");
        assert_eq!(extract_extends(&mut v), vec!["../base.yaml".to_string()]);
        assert!(
            matches!(&v, Value::Mapping(m) if !m.contains_key(Value::String("extends".into())))
        );
    }

    #[test]
    fn extract_extends_list() {
        let mut v = yaml("extends:\n  - a.yaml\n  - https://x/b.yaml\nskills: []\n");
        assert_eq!(extract_extends(&mut v), vec!["a.yaml", "https://x/b.yaml"]);
    }

    #[test]
    fn extract_extends_absent() {
        let mut v = yaml("skills: []\n");
        assert!(extract_extends(&mut v).is_empty());
    }

    // --- merge_yaml ---

    #[test]
    fn merge_replaces_scalars_keeps_base_only_keys() {
        let base = yaml("destination: ./skills\nscope: global\nagent: cursor\nskills: []\n");
        let overlay = yaml("scope: project\nskills: []\n");
        let merged = merge_yaml(base, overlay);
        assert_eq!(merged.get("scope").and_then(Value::as_str), Some("project"));
        assert_eq!(merged.get("agent").and_then(Value::as_str), Some("cursor"));
        assert_eq!(
            merged.get("destination").and_then(Value::as_str),
            Some("./skills")
        );
    }

    #[test]
    fn merge_appends_distinct_and_overrides_same_identity() {
        let base = yaml("skills:\n  - source: https://x/a\n    ref: v1\n    skills: \"*\"\n");
        let overlay =
            yaml("skills:\n  - source: https://x/a\n    ref: v1\n    skills:\n      - one\n  - source: https://x/b\n    skills: \"*\"\n");
        let merged = merge_yaml(base, overlay);
        let Value::Sequence(seq) = merged.get("skills").unwrap() else {
            panic!("expected sequence")
        };
        assert_eq!(seq.len(), 2);
        // same-identity (a@v1) replaced by overlay's narrower list
        assert!(matches!(seq[0].get("skills").unwrap(), Value::Sequence(_)));
    }

    #[test]
    fn merge_keeps_distinct_refs_and_sub_dirs_separate() {
        let base = yaml("skills:\n  - source: https://x/a\n    ref: v1\n    skills: \"*\"\n");
        let overlay = yaml("skills:\n  - source: https://x/a\n    ref: v2\n    skills: \"*\"\n");
        let merged = merge_yaml(base, overlay);
        let Value::Sequence(seq) = merged.get("skills").unwrap() else {
            panic!("expected sequence")
        };
        assert_eq!(seq.len(), 2);
    }

    #[test]
    fn merge_mcps_and_commands_use_same_rules() {
        let base = yaml("mcps:\n  - source: https://x/a\n    ref: v1\n    mcps: \"*\"\n");
        let overlay =
            yaml("mcps:\n  - source: https://x/a\n    ref: v1\n    mcps:\n      - github\n");
        let merged = merge_yaml(base, overlay);
        let Value::Sequence(seq) = merged.get("mcps").unwrap() else {
            panic!("expected sequence")
        };
        assert_eq!(seq.len(), 1);
    }

    // --- recursive loader: extends + cycle + depth guards ---

    #[test]
    fn load_config_any_resolves_extends_relative_to_parent() {
        let root = temp_dir("agent-env-extends-rel");
        let base = root.join("base.yaml");
        let child = root.join("child.yaml");
        fs::write(
            &base,
            "agent: cursor\nscope: global\nskills:\n  - source: https://x/a\n    ref: v1\n    skills: \"*\"\n",
        )
        .unwrap();
        fs::write(
            &child,
            "extends: ./base.yaml\nscope: project\nskills:\n  - source: https://x/b\n    skills: \"*\"\n",
        )
        .unwrap();

        let (cfg, _, _) = load_config_any(child.to_str().unwrap()).expect("load");
        assert_eq!(cfg.scope, Some(Scope::Project));
        assert_eq!(cfg.skills.len(), 2);
        assert!(cfg
            .skills
            .iter()
            .any(|s| s.source == "https://x/a" && s.git_ref.as_deref() == Some("v1")));
        assert!(cfg.skills.iter().any(|s| s.source == "https://x/b"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_config_any_chains_extends() {
        let root = temp_dir("agent-env-extends-chain");
        let a = root.join("a.yaml");
        let b = root.join("b.yaml");
        let c = root.join("c.yaml");
        fs::write(&a, "agent: cursor\nscope: global\nskills: []\n").unwrap();
        fs::write(&b, "extends: ./a.yaml\nskills: []\n").unwrap();
        fs::write(&c, "extends: ./b.yaml\nscope: project\nskills: []\n").unwrap();

        let (cfg, _, _) = load_config_any(c.to_str().unwrap()).expect("load");
        assert_eq!(cfg.scope, Some(Scope::Project));
        assert_eq!(cfg.agents().len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_config_any_overrides_same_identity_in_extends() {
        let root = temp_dir("agent-env-extends-override");
        let base = root.join("base.yaml");
        let child = root.join("child.yaml");
        fs::write(
            &base,
            "agent: cursor\nskills:\n  - source: https://x/a\n    ref: v1\n    skills: \"*\"\n",
        )
        .unwrap();
        fs::write(
            &child,
            "extends: ./base.yaml\nskills:\n  - source: https://x/a\n    ref: v1\n    skills:\n      - one\n",
        )
        .unwrap();

        let (cfg, _, _) = load_config_any(child.to_str().unwrap()).expect("load");
        assert_eq!(cfg.skills.len(), 1);
        assert!(matches!(&cfg.skills[0].skills, SkillsField::List(items) if items.len() == 1));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_config_any_detects_cycles() {
        let root = temp_dir("agent-env-extends-cycle");
        let a = root.join("a.yaml");
        let b = root.join("b.yaml");
        fs::write(&a, "extends: ./b.yaml\nskills: []\n").unwrap();
        fs::write(&b, "extends: ./a.yaml\nskills: []\n").unwrap();

        let result = load_config_any(a.to_str().unwrap());
        assert!(result.is_err(), "expected cycle error");
        let msg = format!("{}", result.err().unwrap());
        assert!(msg.contains("circular"), "got: {msg}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_config_any_enforces_depth_guard() {
        // Build a linear chain longer than MAX_EXTENDS_DEPTH (each extends the next).
        let root = temp_dir("agent-env-extends-depth");
        let count: u8 = MAX_EXTENDS_DEPTH + 3;
        for i in 0..count {
            let path = root.join(format!("c{i}.yaml"));
            if i + 1 < count {
                fs::write(&path, format!("extends: ./c{}.yaml\nskills: []\n", i + 1)).unwrap();
            } else {
                fs::write(&path, "skills: []\n").unwrap();
            }
        }
        let result = load_config_any(root.join("c0.yaml").to_str().unwrap());
        assert!(result.is_err(), "expected depth-limit error");
        let msg = format!("{}", result.err().unwrap());
        assert!(msg.contains("depth limit exceeded"), "got: {msg}");
        let _ = fs::remove_dir_all(&root);
    }
}
