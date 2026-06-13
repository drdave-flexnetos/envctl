# Target architecture — kasetto → envctl-agent-env

The Rust target layout is **already established by the seed** (commit `6ecb270`) + the architect plan
(`.handoff/loop/cycle/01_architect_plan.md`). The port EXTENDS this layout; it does not redesign it.

## Crate: `crates/agent-env` (package `envctl-agent-env`)

| Module | Role | kasetto source absorbed |
|--------|------|--------------------------|
| `lib.rs` | crate root, `AgentEnvError` (thiserror), `Result`, `err()`, re-exports | `src/error.rs`, `src/lib.rs` exports |
| `config.rs` | 6-key+extends schema, `Scope`, `SourceSpec`/`McpSourceSpec`/`CommandSourceSpec`, `*Field`/`*Entry`, `GitPin`, `Agent`/`AgentField`/`AGENT_PRESETS`, the 21-preset path/target methods, `McpSettingsFormat`/`CommandFormat`/`*Target`, `resolve_scope` | `src/model/{config,agent,types,mod}.rs` |
| `extend.rs` | `extract_extends`, `merge_yaml`, `load_config_recursive`/`load_config_any`, `fetch_config_text`, depth(8)+cycle guards | `src/model/extend.rs`, `src/fsops/config.rs` |
| `source.rs` | `RepoUrl`, `parse_repo_url`, host families, `derive_browse_url`/`rewrite_browse_to_raw_url`, archive-URL builders, `download_extract` (tar-slip guard), `UrlRequestAuth`, `http_client` | `src/source/{hosts,parse,remote,auth}.rs`, `src/fsops/http.rs` |
| `hash.rs` | OS-invariant SHA-256 (`hash_dir`/`hash_file`/`hash_str`) | `src/fsops/hash.rs` |
| `lock.rs` | `AgentLockFile`, `AssetEntry`, `LockMode {Plain,Update,Locked}`, `lock_check`, `LOCK_VERSION=2`; SHA-256 agent-asset lock **separate** from engine FNV-1a | `src/lock.rs`, `src/model/types.rs`, `src/commands/lock.rs` (mode logic) |
| `fsops.rs` *(new)* | `copy_dir`, `resolve_path`, `select_targets`, `resolve_destinations`, `resolve_mcp_settings_targets`, `resolve_command_targets`, `scope_root`/`relativize_dest`/`resolve_dest`, `SettingsFile` | `src/fsops/{copy,settings,mod}.rs` |
| `config_edit.rs` *(new)* | comment-preserving config mutation (`add`/`remove`/`source_edit` logic): `Section`/`Pin`/`Selector`/`SourceItem`/`RemoveOutcome` | `src/fsops/config_edit.rs` (811 lines), `src/source/edit.rs` |
| `mcp.rs` *(new)* | the **4-format additive/never-clobber MCP merge** (McpServers/VsCodeServers/OpenCode/CodexToml) — MUST preserve global broker/repowire/weave servers | `src/mcps/*` |
| `command.rs` *(new)* | the **5 command-format transforms** (MarkdownFrontmatter/MarkdownPlain/PromptMd/PromptFile/GeminiToml) | command-format logic in `src/commands/*` + `src/prompts/*` |
| `report.rs` *(new)* | sync-result value types (`Summary`/`Action`/`Report`/`InstalledSkill`/`SyncFailure`) | `src/model/types.rs` |
| `dirs.rs` / `util.rs` *(new)* | XDG dir resolution (envctl-namespaced), `now_unix` | `src/fsops/{dirs,mod}.rs` |

## Idiom / dependency map
- kasetto error channel (`Box<dyn Error>` string messages) → idiomatic `thiserror` `AgentEnvError`
  (`Message`/`Io`/`Yaml`) with `err()` helper for parity with kasetto's string-message contract.
- `serde`/`serde_yaml`/`serde_json`/`toml` reused from workspace pins; **`mimalloc` DROPPED**;
  `flate2 = {default-features=false, features=["rust_backend"]}` (miniz_oxide, no C); `tar`; `sha2`;
  `reqwest` (workspace pin, rustls+ring). One rustls, ring-only.
- `#![forbid(unsafe_code)]`, MSRV 1.80, snake_case modules.

## Boundary (do NOT port into this crate)
- **Engine wiring** (`Engine` methods/Events driving these absorbed fns) = TASK-0013.
- **CLI verbs** (`envctl agent {sync,add,remove,lock,list,clean}`) + GUI parity = TASK-0014.
- **Front-end / presentation** (`src/{ui,banner,colors}.rs`, clap `src/cli.rs`/`app.rs`,
  `update_notifier.rs`/`self_update.rs`) — envctl owns rendering (`- [≠]` rows).
- **Engine FNV-1a component lock** (`crates/engine/src/lock.rs`) — untouched; agent-asset SHA-256
  lock is a separate type/section.
