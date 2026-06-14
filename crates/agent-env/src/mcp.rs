//! MCP pack merge / removal across agent-native config formats — ported verbatim from
//! kasetto v3.2.0 `src/mcps/{mod,merge,codex,pack}.rs` (ledger MC-01 + MC-02).
//!
//! **The no-downgrade invariant:** the merge is **ADDITIVE and NEVER-CLOBBER**. It PRESERVES
//! pre-existing MCP servers in the target settings — the global `broker`/`repowire`/`weave`
//! servers survive a merge. Existing keys are kept untouched; only new server names from the
//! pack are added; an existing same-named entry is left as-is (the pack does NOT overwrite a
//! real secret in place). All four destination formats are ported:
//!
//! - [`McpSettingsFormat::McpServers`] — `{ "mcpServers": { ... } }` (Claude, Cursor, …).
//! - [`McpSettingsFormat::VsCodeServers`] — VS Code / Copilot `{ "servers": { ... } }`.
//! - [`McpSettingsFormat::OpenCode`] — `{ "mcp": { name: { "type": local|remote, … } } }`.
//! - [`McpSettingsFormat::CodexToml`] — Codex `config.toml` `[mcp_servers.name]` tables.
//!
//! The four merge functions all live in this single module (kasetto split them across
//! `mod`/`merge`/`codex`/`pack`); the file-format submodules collapse to private helpers here.
//! Non-printing; returns [`crate::Result`]; kasetto's `err(...)` maps onto
//! [`crate::AgentEnvError::Message`](crate::AgentEnvError).

use std::fs;
use std::path::Path;

use toml::Value as Toml;

use crate::agent::{McpSettingsFormat, McpSettingsTarget};
use crate::fsops::SettingsFile;
use crate::{err, Result};

// ---------------------------------------------------------------------------
// Top-level dispatch (kasetto src/mcps/mod.rs)
// ---------------------------------------------------------------------------

/// Merge MCP server definitions from a pack JSON into an agent-native config file.
/// The pack must have a top-level `"mcpServers"` object.
pub fn merge_mcp_config(source_path: &Path, target: &McpSettingsTarget) -> Result<()> {
    match target.format {
        McpSettingsFormat::McpServers => merge_mcp_servers_object(source_path, &target.path),
        McpSettingsFormat::VsCodeServers => merge_vscode_servers_object(source_path, &target.path),
        McpSettingsFormat::OpenCode => merge_opencode_mcp_object(source_path, &target.path),
        McpSettingsFormat::CodexToml => merge_codex_config_toml(source_path, &target.path),
    }
}

/// Remove a server entry from an agent-native config file (no-op if the file is absent).
pub fn remove_mcp_server(server_name: &str, target: &McpSettingsTarget) -> Result<()> {
    if !target.path.exists() {
        return Ok(());
    }
    match target.format {
        McpSettingsFormat::CodexToml => codex_remove_server(server_name, &target.path),
        McpSettingsFormat::McpServers => {
            json_remove_top_level_key(server_name, &target.path, "mcpServers")
        }
        McpSettingsFormat::VsCodeServers => {
            json_remove_top_level_key(server_name, &target.path, "servers")
        }
        McpSettingsFormat::OpenCode => json_remove_top_level_key(server_name, &target.path, "mcp"),
    }
}

fn json_remove_top_level_key(server_name: &str, path: &Path, object_key: &str) -> Result<()> {
    let mut sf = SettingsFile::load(path)?;
    if let Some(map) = sf.data.get_mut(object_key).and_then(|v| v.as_object_mut()) {
        map.remove(server_name);
    }
    sf.save()?;
    Ok(())
}

/// Return `true` iff every named server is already present in the target settings.
pub fn servers_present_in_settings(server_names: &[String], target: &McpSettingsTarget) -> bool {
    if server_names.is_empty() {
        return true;
    }
    match target.format {
        McpSettingsFormat::CodexToml => codex_servers_present(server_names, &target.path),
        McpSettingsFormat::McpServers => {
            json_all_keys_present(server_names, &target.path, "mcpServers")
        }
        McpSettingsFormat::VsCodeServers => {
            json_all_keys_present(server_names, &target.path, "servers")
        }
        McpSettingsFormat::OpenCode => json_all_keys_present(server_names, &target.path, "mcp"),
    }
}

fn json_all_keys_present(server_names: &[String], path: &Path, root_key: &str) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let Some(map) = val.get(root_key).and_then(|v| v.as_object()) else {
        return false;
    };
    server_names.iter().all(|name| map.contains_key(name))
}

// ---------------------------------------------------------------------------
// Pack reader (kasetto src/mcps/pack.rs)
// ---------------------------------------------------------------------------

/// Read `mcpServers` definitions from a pack JSON file.
pub fn read_source_mcp_servers(
    source_path: &Path,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let source_text = fs::read_to_string(source_path)?;
    let source: serde_json::Value = serde_json::from_str(&source_text)
        .map_err(|e| err(format!("invalid MCP JSON {}: {e}", source_path.display())))?;
    Ok(source
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// JSON-format merges (kasetto src/mcps/merge.rs)
// ---------------------------------------------------------------------------

/// Shared scaffolding: read source pack, load target settings, ensure the
/// root object key exists, apply `transform` to each new entry, save.
fn merge_into_json_key(
    source_path: &Path,
    target_path: &Path,
    root_key: &str,
    transform: fn(&str, serde_json::Value) -> Result<serde_json::Value>,
) -> Result<()> {
    let src_map = read_source_mcp_servers(source_path)?;
    let mut sf = SettingsFile::load(target_path)?;
    let target_obj = sf
        .data
        .as_object_mut()
        .ok_or_else(|| err("settings file is not a JSON object"))?;
    let section = target_obj
        .entry(root_key)
        .or_insert_with(|| serde_json::json!({}));

    if let Some(dst_map) = section.as_object_mut() {
        for (key, value) in src_map {
            if !dst_map.contains_key(&key) {
                dst_map.insert(key.clone(), transform(&key, value)?);
            }
        }
    }
    sf.save()?;
    Ok(())
}

fn merge_mcp_servers_object(source_path: &Path, target_path: &Path) -> Result<()> {
    merge_into_json_key(source_path, target_path, "mcpServers", |_name, v| Ok(v))
}

fn merge_vscode_servers_object(source_path: &Path, target_path: &Path) -> Result<()> {
    merge_into_json_key(source_path, target_path, "servers", |_name, v| {
        Ok(normalize_vscode_server(v))
    })
}

fn merge_opencode_mcp_object(source_path: &Path, target_path: &Path) -> Result<()> {
    merge_into_json_key(source_path, target_path, "mcp", |name, v| {
        mcp_entry_to_opencode(name, &v)
    })
}

fn normalize_vscode_server(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = value.as_object_mut() {
        if !obj.contains_key("type") {
            if obj.contains_key("command") {
                obj.insert("type".into(), serde_json::json!("stdio"));
            } else if obj.contains_key("url") {
                obj.insert("type".into(), serde_json::json!("http"));
            }
        }
    }
    value
}

fn mcp_entry_to_opencode(name: &str, v: &serde_json::Value) -> Result<serde_json::Value> {
    let Some(obj) = v.as_object() else {
        return Err(err(format!(
            "MCP server {name} must be a JSON object for OpenCode merge"
        )));
    };

    if let Some(url) = obj
        .get("url")
        .and_then(|u| u.as_str())
        .or_else(|| obj.get("serverUrl").and_then(|u| u.as_str()))
    {
        let mut out = serde_json::Map::new();
        out.insert("type".into(), serde_json::json!("remote"));
        out.insert("url".into(), serde_json::json!(url));
        out.insert("enabled".into(), serde_json::json!(true));
        if let Some(h) = obj.get("headers").and_then(|x| x.as_object()) {
            out.insert("headers".into(), serde_json::Value::Object(h.clone()));
        }
        return Ok(serde_json::Value::Object(out));
    }

    let cmd = obj.get("command").and_then(|c| c.as_str()).ok_or_else(|| {
        err(format!(
            "MCP server {name} needs `command` or `url` for OpenCode"
        ))
    })?;

    let mut cmd_arr = vec![serde_json::json!(cmd)];
    if let Some(args) = obj.get("args").and_then(|a| a.as_array()) {
        cmd_arr.extend(args.iter().cloned());
    }

    let mut out = serde_json::Map::new();
    out.insert("type".into(), serde_json::json!("local"));
    out.insert("command".into(), serde_json::Value::Array(cmd_arr));
    out.insert("enabled".into(), serde_json::json!(true));
    if let Some(env) = obj.get("env").and_then(|e| e.as_object()) {
        out.insert("environment".into(), serde_json::Value::Object(env.clone()));
    }
    Ok(serde_json::Value::Object(out))
}

// ---------------------------------------------------------------------------
// CodexToml format (kasetto src/mcps/codex.rs)
// ---------------------------------------------------------------------------

fn merge_codex_config_toml(source_path: &Path, target_path: &Path) -> Result<()> {
    let src_map = read_source_mcp_servers(source_path)?;
    let mut root = load_or_empty_toml(target_path)?;
    let root_tbl = root
        .as_table_mut()
        .ok_or_else(|| err("Codex config root must be a TOML table"))?;
    let mcp_entry = root_tbl
        .entry("mcp_servers")
        .or_insert_with(|| Toml::Table(Default::default()));
    let mcp_tbl = mcp_entry
        .as_table_mut()
        .ok_or_else(|| err("Codex mcp_servers must be a TOML table"))?;

    for (name, cfg) in src_map {
        if mcp_tbl.contains_key(&name) {
            continue;
        }
        let table = json_mcp_server_to_codex_toml_table(&cfg)?;
        mcp_tbl.insert(name, Toml::Table(table));
    }

    write_codex_toml(target_path, &root)
}

fn codex_remove_server(server_name: &str, target_path: &Path) -> Result<()> {
    let mut root = load_or_empty_toml(target_path)?;
    if let Some(mcp) = root
        .as_table_mut()
        .and_then(|t| t.get_mut("mcp_servers"))
        .and_then(|m| m.as_table_mut())
    {
        mcp.remove(server_name);
    }
    write_codex_toml(target_path, &root)
}

fn codex_servers_present(server_names: &[String], target_path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(target_path) else {
        return false;
    };
    let Ok(val) = text.parse::<Toml>() else {
        return false;
    };
    let Some(map) = val.get("mcp_servers").and_then(|v| v.as_table()) else {
        return false;
    };
    server_names.iter().all(|name| map.contains_key(name))
}

fn load_or_empty_toml(target_path: &Path) -> Result<Toml> {
    if !target_path.exists() {
        return Ok(Toml::Table(Default::default()));
    }
    let text = fs::read_to_string(target_path)?;
    text.parse::<Toml>().map_err(|e| {
        err(format!(
            "invalid Codex config TOML {}: {e}",
            target_path.display()
        ))
    })
}

fn write_codex_toml(target_path: &Path, root: &Toml) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let out = toml::to_string_pretty(root)
        .map_err(|e| err(format!("failed to serialize Codex config.toml: {e}")))?;
    fs::write(target_path, out)?;
    Ok(())
}

fn json_mcp_server_to_codex_toml_table(
    v: &serde_json::Value,
) -> Result<toml::map::Map<String, Toml>> {
    let obj = v
        .as_object()
        .ok_or_else(|| err("each mcpServers entry must be a JSON object for Codex"))?;
    let mut out = toml::map::Map::new();

    let url = obj
        .get("url")
        .and_then(|u| u.as_str())
        .or_else(|| obj.get("serverUrl").and_then(|u| u.as_str()));

    let ty = obj.get("type").and_then(|t| t.as_str());
    let is_remote =
        url.is_some() || matches!(ty, Some("http" | "https" | "sse" | "streamable-http"));

    if is_remote {
        let Some(url) = url else {
            return Err(err(
                "remote MCP entry for Codex needs a string `url` (or `serverUrl`)",
            ));
        };
        out.insert("url".into(), Toml::String(url.to_string()));

        if let Some(h) = obj.get("headers").and_then(|x| x.as_object()) {
            let mut ht = toml::map::Map::new();
            for (k, v) in h {
                if let Some(s) = v.as_str() {
                    ht.insert(k.clone(), Toml::String(s.to_string()));
                }
            }
            if !ht.is_empty() {
                out.insert("http_headers".into(), Toml::Table(ht));
            }
        }
        return Ok(out);
    }

    let cmd = obj
        .get("command")
        .and_then(|c| c.as_str())
        .ok_or_else(|| err("Codex stdio MCP needs a string `command` (or use `url` for remote)"))?;
    out.insert("command".into(), Toml::String(cmd.to_string()));

    if let Some(args) = obj.get("args").and_then(|a| a.as_array()) {
        let arr: Vec<Toml> = args
            .iter()
            .map(|x| {
                Toml::String(match x {
                    serde_json::Value::String(s) => s.clone(),
                    _ => x.to_string(),
                })
            })
            .collect();
        if !arr.is_empty() {
            out.insert("args".into(), Toml::Array(arr));
        }
    }

    if let Some(env) = obj.get("env").and_then(|e| e.as_object()) {
        let mut et = toml::map::Map::new();
        for (k, v) in env {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                // Non-string values (numbers, bools) are rendered via Display.
                _ => v.to_string(),
            };
            et.insert(k.clone(), Toml::String(s));
        }
        if !et.is_empty() {
            out.insert("env".into(), Toml::Table(et));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use toml::Value as TomlVal;

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    fn mcp_target(path: PathBuf) -> McpSettingsTarget {
        McpSettingsTarget {
            path,
            format: McpSettingsFormat::McpServers,
        }
    }

    // --- ported verbatim from kasetto v3.2.0 src/mcps/mod.rs tests (parity evidence) ---

    #[test]
    fn merge_mcp_config_creates_target_from_scratch() {
        let dir = temp_dir("agent-env-mcps-create");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("settings.json");

        fs::write(
            &source,
            r#"{"mcpServers":{"git-tools":{"command":"git-mcp"}}}"#,
        )
        .unwrap();

        merge_mcp_config(&source, &mcp_target(target.clone())).expect("merge");

        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(val["mcpServers"]["git-tools"]["command"], "git-mcp");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_mcp_config_preserves_existing_servers() {
        let dir = temp_dir("agent-env-mcps-merge");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("settings.json");

        fs::write(
            &target,
            r#"{"mcpServers":{"existing":{"command":"keep-me"}}}"#,
        )
        .unwrap();
        fs::write(
            &source,
            r#"{"mcpServers":{"new-server":{"command":"new-cmd"}}}"#,
        )
        .unwrap();

        merge_mcp_config(&source, &mcp_target(target.clone())).expect("merge");

        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(val["mcpServers"]["existing"]["command"], "keep-me");
        assert_eq!(val["mcpServers"]["new-server"]["command"], "new-cmd");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_mcp_config_does_not_overwrite_existing_key() {
        let dir = temp_dir("agent-env-mcps-no-overwrite");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("settings.json");

        fs::write(
            &target,
            r#"{"mcpServers":{"airflow":{"command":"uvx","env":{"AIRFLOW_PASSWORD":"real-secret"}}}}"#,
        )
        .unwrap();
        fs::write(
            &source,
            r#"{"mcpServers":{"airflow":{"command":"uvx","env":{"AIRFLOW_PASSWORD":"__FROM_SOURCE_PACK__"}}}}"#,
        )
        .unwrap();

        merge_mcp_config(&source, &mcp_target(target.clone())).expect("merge");

        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(
            val["mcpServers"]["airflow"]["env"]["AIRFLOW_PASSWORD"],
            "real-secret"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_codex_writes_config_toml() {
        let dir = temp_dir("agent-env-mcps-codex");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("config.toml");
        fs::write(
            &source,
            r#"{"mcpServers":{"demo":{"command":"uvx","args":["p"],"env":{"K":"v"}}}}"#,
        )
        .unwrap();
        let tgt = McpSettingsTarget {
            path: target.clone(),
            format: McpSettingsFormat::CodexToml,
        };
        merge_mcp_config(&source, &tgt).expect("merge");
        let parsed: TomlVal = fs::read_to_string(&target).unwrap().parse().unwrap();
        let mcp = parsed.get("mcp_servers").unwrap().as_table().unwrap();
        assert_eq!(mcp["demo"]["command"].as_str().unwrap(), "uvx");
        let args = mcp["demo"]["args"].as_array().unwrap();
        assert_eq!(args[0].as_str().unwrap(), "p");
        assert_eq!(mcp["demo"]["env"]["K"].as_str().unwrap(), "v");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_codex_preserves_unrelated_toml_keys() {
        let dir = temp_dir("agent-env-mcps-codex-merge");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("config.toml");
        fs::write(&target, "model = \"gpt-5.1\"\n").unwrap();
        fs::write(
            &source,
            r#"{"mcpServers":{"new":{"command":"npx","args":["-y","x"]}}}"#,
        )
        .unwrap();
        let tgt = McpSettingsTarget {
            path: target.clone(),
            format: McpSettingsFormat::CodexToml,
        };
        merge_mcp_config(&source, &tgt).expect("merge");
        let parsed: TomlVal = fs::read_to_string(&target).unwrap().parse().unwrap();
        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()).unwrap(),
            "gpt-5.1"
        );
        assert!(parsed
            .get("mcp_servers")
            .unwrap()
            .as_table()
            .unwrap()
            .contains_key("new"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_codex_mcp_server_entry() {
        let dir = temp_dir("agent-env-mcps-codex-rm");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        fs::write(
            &path,
            r#"[mcp_servers.a]
command = "a"
[mcp_servers.b]
command = "b"
"#,
        )
        .unwrap();
        let tgt = McpSettingsTarget {
            path: path.clone(),
            format: McpSettingsFormat::CodexToml,
        };
        remove_mcp_server("a", &tgt).expect("remove");
        let parsed: TomlVal = fs::read_to_string(&path).unwrap().parse().unwrap();
        let mcp = parsed["mcp_servers"].as_table().unwrap();
        assert!(!mcp.contains_key("a"));
        assert!(mcp.contains_key("b"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_vscode_adds_stdio_type() {
        let dir = temp_dir("agent-env-mcps-vscode");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("mcp.json");
        fs::write(
            &source,
            r#"{"mcpServers":{"mem":{"command":"npx","args":["-y","@x/y"]}}}"#,
        )
        .unwrap();
        let tgt = McpSettingsTarget {
            path: target.clone(),
            format: McpSettingsFormat::VsCodeServers,
        };
        merge_mcp_config(&source, &tgt).expect("merge");
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(val["servers"]["mem"]["type"], "stdio");
        assert_eq!(val["servers"]["mem"]["command"], "npx");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_opencode_local_command() {
        let dir = temp_dir("agent-env-mcps-opencode");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("opencode.json");
        fs::write(
            &source,
            r#"{"mcpServers":{"tool":{"command":"uvx","args":["pkg"],"env":{"K":"v"}}}}"#,
        )
        .unwrap();
        let tgt = McpSettingsTarget {
            path: target.clone(),
            format: McpSettingsFormat::OpenCode,
        };
        merge_mcp_config(&source, &tgt).expect("merge");
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(val["mcp"]["tool"]["type"], "local");
        assert_eq!(val["mcp"]["tool"]["command"][0], "uvx");
        assert_eq!(val["mcp"]["tool"]["environment"]["K"], "v");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_mcp_server_deletes_entry() {
        let dir = temp_dir("agent-env-mcps-remove");
        fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");

        fs::write(
            &settings,
            r#"{"mcpServers":{"a":{"cmd":"1"},"b":{"cmd":"2"}}}"#,
        )
        .unwrap();

        remove_mcp_server("a", &mcp_target(settings.clone())).expect("remove");

        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
        assert!(val["mcpServers"]["a"].is_null());
        assert_eq!(val["mcpServers"]["b"]["cmd"], "2");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_mcp_server_noop_on_missing_file() {
        let path = temp_dir("agent-env-mcps-noop").join("nonexistent.json");
        remove_mcp_server(
            "some-server",
            &McpSettingsTarget {
                path,
                format: McpSettingsFormat::McpServers,
            },
        )
        .unwrap();
    }

    #[test]
    fn servers_present_all_exist() {
        let dir = temp_dir("agent-env-mcps-present");
        fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        fs::write(
            &settings,
            r#"{"mcpServers":{"airflow":{"cmd":"a"},"git":{"cmd":"g"}}}"#,
        )
        .unwrap();

        assert!(servers_present_in_settings(
            &["airflow".into(), "git".into()],
            &mcp_target(settings)
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn servers_present_missing_server() {
        let dir = temp_dir("agent-env-mcps-missing");
        fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        fs::write(&settings, r#"{"mcpServers":{"git":{"cmd":"g"}}}"#).unwrap();

        assert!(!servers_present_in_settings(
            &["airflow".into(), "git".into()],
            &mcp_target(settings)
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn servers_present_missing_file() {
        let path = temp_dir("agent-env-mcps-nofile").join("nope.json");
        assert!(!servers_present_in_settings(
            &["airflow".into()],
            &mcp_target(path)
        ));
    }

    #[test]
    fn servers_present_empty_list() {
        let path = temp_dir("agent-env-mcps-empty").join("nope.json");
        assert!(servers_present_in_settings(&[], &mcp_target(path)));
    }

    // --- never-clobber invariant (MC-01/MC-02 no-downgrade): the global broker/repowire/
    //     weave servers MUST survive a merge, and the new pack server is added. ---

    #[test]
    fn merge_preserves_broker_repowire_weave_and_adds_new() {
        let dir = temp_dir("agent-env-mcps-never-clobber");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("settings.json");

        // Target already holds the three global mesh servers with real values.
        fs::write(
            &target,
            r#"{"mcpServers":{
                "broker":{"command":"broker-mcp","env":{"TOKEN":"real-broker-token"}},
                "repowire":{"command":"repowire-mcp","args":["--serve"]},
                "weave":{"url":"https://weave.local","headers":{"Authorization":"real-weave-auth"}}
            }}"#,
        )
        .unwrap();
        // Pack tries to add a new server AND re-supply broker with a placeholder token.
        fs::write(
            &source,
            r#"{"mcpServers":{
                "broker":{"command":"broker-mcp","env":{"TOKEN":"__PLACEHOLDER__"}},
                "new-tool":{"command":"new-mcp"}
            }}"#,
        )
        .unwrap();

        merge_mcp_config(&source, &mcp_target(target.clone())).expect("merge");

        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        // All three pre-existing global servers survive, with their real values intact.
        assert_eq!(
            val["mcpServers"]["broker"]["env"]["TOKEN"],
            "real-broker-token"
        );
        assert_eq!(val["mcpServers"]["repowire"]["command"], "repowire-mcp");
        assert_eq!(
            val["mcpServers"]["weave"]["headers"]["Authorization"],
            "real-weave-auth"
        );
        // The new server from the pack is added.
        assert_eq!(val["mcpServers"]["new-tool"]["command"], "new-mcp");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_codex_never_clobbers_existing_mesh_servers() {
        let dir = temp_dir("agent-env-mcps-codex-never-clobber");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("config.toml");

        fs::write(
            &target,
            r#"[mcp_servers.broker]
command = "broker-mcp"
[mcp_servers.broker.env]
TOKEN = "real-broker-token"
[mcp_servers.repowire]
command = "repowire-mcp"
[mcp_servers.weave]
url = "https://weave.local"
"#,
        )
        .unwrap();
        fs::write(
            &source,
            r#"{"mcpServers":{
                "broker":{"command":"broker-mcp","env":{"TOKEN":"__PLACEHOLDER__"}},
                "new-tool":{"command":"new-mcp"}
            }}"#,
        )
        .unwrap();

        let tgt = McpSettingsTarget {
            path: target.clone(),
            format: McpSettingsFormat::CodexToml,
        };
        merge_mcp_config(&source, &tgt).expect("merge");

        let parsed: TomlVal = fs::read_to_string(&target).unwrap().parse().unwrap();
        let mcp = parsed["mcp_servers"].as_table().unwrap();
        // Pre-existing entries survive untouched.
        assert_eq!(
            mcp["broker"]["env"]["TOKEN"].as_str().unwrap(),
            "real-broker-token"
        );
        assert!(mcp.contains_key("repowire"));
        assert!(mcp.contains_key("weave"));
        // New server added.
        assert_eq!(mcp["new-tool"]["command"].as_str().unwrap(), "new-mcp");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_vscode_never_clobbers_existing_servers() {
        let dir = temp_dir("agent-env-mcps-vscode-never-clobber");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("mcp.json");

        fs::write(
            &target,
            r#"{"servers":{"broker":{"type":"stdio","command":"broker-real"}}}"#,
        )
        .unwrap();
        fs::write(
            &source,
            r#"{"mcpServers":{"broker":{"command":"broker-placeholder"},"new":{"command":"npx"}}}"#,
        )
        .unwrap();

        let tgt = McpSettingsTarget {
            path: target.clone(),
            format: McpSettingsFormat::VsCodeServers,
        };
        merge_mcp_config(&source, &tgt).expect("merge");

        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(val["servers"]["broker"]["command"], "broker-real");
        assert_eq!(val["servers"]["new"]["type"], "stdio");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_opencode_never_clobbers_existing_servers() {
        let dir = temp_dir("agent-env-mcps-opencode-never-clobber");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("opencode.json");

        fs::write(
            &target,
            r#"{"mcp":{"weave":{"type":"remote","url":"https://weave.real","enabled":true}}}"#,
        )
        .unwrap();
        fs::write(
            &source,
            r#"{"mcpServers":{"weave":{"url":"https://weave.placeholder"},"tool":{"command":"uvx"}}}"#,
        )
        .unwrap();

        let tgt = McpSettingsTarget {
            path: target.clone(),
            format: McpSettingsFormat::OpenCode,
        };
        merge_mcp_config(&source, &tgt).expect("merge");

        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(val["mcp"]["weave"]["url"], "https://weave.real");
        assert_eq!(val["mcp"]["tool"]["type"], "local");
        let _ = fs::remove_dir_all(&dir);
    }

    // --- read_source_mcp_servers (pack.rs) coverage ---

    #[test]
    fn read_source_returns_empty_when_no_mcp_servers_key() {
        let dir = temp_dir("agent-env-mcps-pack-empty");
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("source.json");
        fs::write(&src, r#"{"other":"data"}"#).unwrap();
        let map = read_source_mcp_servers(&src).expect("read");
        assert!(map.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_source_invalid_json_errors() {
        let dir = temp_dir("agent-env-mcps-pack-bad");
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("source.json");
        fs::write(&src, "{not json").unwrap();
        assert!(read_source_mcp_servers(&src).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_codex_remote_url_writes_http_headers() {
        let dir = temp_dir("agent-env-mcps-codex-remote");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("config.toml");
        fs::write(
            &source,
            r#"{"mcpServers":{"remote":{"url":"https://api.example","headers":{"Authorization":"Bearer x"}}}}"#,
        )
        .unwrap();
        let tgt = McpSettingsTarget {
            path: target.clone(),
            format: McpSettingsFormat::CodexToml,
        };
        merge_mcp_config(&source, &tgt).expect("merge");
        let parsed: TomlVal = fs::read_to_string(&target).unwrap().parse().unwrap();
        let entry = parsed["mcp_servers"]["remote"].as_table().unwrap();
        assert_eq!(entry["url"].as_str().unwrap(), "https://api.example");
        assert_eq!(
            entry["http_headers"]["Authorization"].as_str().unwrap(),
            "Bearer x"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_opencode_remote_url() {
        let dir = temp_dir("agent-env-mcps-opencode-remote");
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.json");
        let target = dir.join("opencode.json");
        fs::write(
            &source,
            r#"{"mcpServers":{"r":{"serverUrl":"https://x","headers":{"H":"v"}}}}"#,
        )
        .unwrap();
        let tgt = McpSettingsTarget {
            path: target.clone(),
            format: McpSettingsFormat::OpenCode,
        };
        merge_mcp_config(&source, &tgt).expect("merge");
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(val["mcp"]["r"]["type"], "remote");
        assert_eq!(val["mcp"]["r"]["url"], "https://x");
        assert_eq!(val["mcp"]["r"]["headers"]["H"], "v");
        let _ = fs::remove_dir_all(&dir);
    }
}
