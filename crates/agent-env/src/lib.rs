//! `envctl-agent-env` — the pure-Rust agent-environment provisioning core.
//!
//! This crate is the foundational, **standalone** library of the kasetto absorption
//! (Epic C, TASK-0012): the config model + `extends` inheritance + multi-host source
//! resolver + OS-invariant SHA-256 content hashing + the agent-asset lock (with its 3
//! modes). It is ported faithfully from the live `kasetto` **v3.2.0** source with NO
//! capability downgrade (see `.handoff/decisions/ADR-0001`).
//!
//! Scope boundaries (deliberately deferred to later cards):
//! - This crate is a **library only**: non-printing, no `clap`, no UI, no `Engine`. It
//!   returns `Result<T, AgentEnvError>` and typed data; callers (TASK-0013 Engine,
//!   TASK-0014 CLI/GUI) drive it.
//! - The `Agent` enum models the full 21-preset **shape** plus the per-agent native
//!   path-mapping methods (skills/mcps/commands destinations) and their MCP/command
//!   format+target value types (see [`agent`] and [`report`]). The sync *engine* that
//!   drives them — `commands/sync`, `commands/list` — is deferred to TASK-0013.
//! - The agent-asset lock uses **SHA-256** and is a **separate** type from the engine's
//!   FNV-1a component lock (`crates/engine/src/lock.rs`) — they do not share code.
//!
//! Invariants upheld: no C in the trust boundary (pure-Rust gzip via miniz_oxide, pure-Rust
//! tar, sha2, reqwest→rustls→ring), fail-closed guards (tar-slip path-traversal refusal,
//! `--locked` zero-network), and `#![forbid(unsafe_code)]` (set via `[lints]`).

pub mod agent;
pub mod command;
pub mod config;
pub mod config_edit;
pub mod extend;
pub mod fsops;
pub mod hash;
pub mod lock;
pub mod mcp;
pub mod report;
pub mod source;

pub use agent::{
    all_command_global_targets, all_command_project_targets, all_mcp_project_targets,
    all_mcp_settings_targets, command_global_targets, command_project_targets, CommandFormat,
    CommandTarget, McpSettingsFormat, McpSettingsTarget,
};
pub use command::{apply_command, destination_path, ensure_parent_dirs, parse, render, Parsed};
pub use config::{
    git_pin_of, Agent, AgentField, CommandEntry, CommandSourceSpec, CommandsField, Config, GitPin,
    McpEntry, McpSourceSpec, McpsField, Scope, SkillTarget, SkillsField, SourceSpec, AGENT_PRESETS,
};
pub use config_edit::{
    ensure_local_config, insert_item, is_remote_source, item_exists, remove_item, remove_names,
    split_at_ref, Pin, RemoveOutcome, Section, Selector, SourceItem,
};
pub use extend::{extract_extends, load_config_recursive, merge_yaml, MAX_EXTENDS_DEPTH};
pub use fsops::SettingsFile;
pub use hash::{hash_dir, hash_file, hash_str};
pub use lock::{
    AgentLockEntry, AgentLockFile, AssetEntry, LockMode, AGENT_ASSETS_KEY, LOCK_VERSION,
};
pub use mcp::{
    merge_mcp_config, read_source_mcp_servers, remove_mcp_server, servers_present_in_settings,
};
pub use report::{Action, InstalledSkill, Report, Summary, SyncFailure};
pub use source::{
    archive_url, derive_browse_url, download_extract, parse_repo_url, rewrite_browse_to_raw_url,
    BrowseDerived, RepoUrl, UrlRequestAuth,
};

/// Result alias for the agent-env crate.
pub type Result<T> = std::result::Result<T, AgentEnvError>;

/// Typed error for the agent-env crate.
///
/// Faithful ports of kasetto's string-message `err(...)` calls land on
/// [`AgentEnvError::Message`]; structured failures use the dedicated variants.
#[derive(Debug, thiserror::Error)]
pub enum AgentEnvError {
    /// A free-form failure message (the absorbed kasetto `err(...)` channel).
    #[error("{0}")]
    Message(String),

    /// An I/O failure (file read/write, archive extraction).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A YAML (de)serialization failure for config or lock files.
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// A JSON (de)serialization failure for agent settings files.
    ///
    /// Mirrors kasetto's box-error auto-conversion of `serde_json::Error` via `?`
    /// (e.g. `SettingsFile::save`'s `serde_json::to_string_pretty`).
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Construct an [`AgentEnvError::Message`] — the string-message error channel that
/// mirrors kasetto's `err(...)` helper, so the ported control flow is line-for-line.
pub(crate) fn err(message: impl Into<String>) -> AgentEnvError {
    AgentEnvError::Message(message.into())
}
