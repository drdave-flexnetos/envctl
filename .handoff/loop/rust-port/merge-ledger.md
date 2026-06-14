# Merge ledger — kasetto v3.2.0 → envctl (destination Y) · no-downgrade-across-the-merge

Destination-side counterpart of `parity-ledger.md`. Seeded from the researcher's reuse map
(`reports/research.md`). Y = envctl worktree `/home/drdave/Desktop/meta/.worktrees/task-0012-agent-env/envctl`
@ `8006b4c` (branch `task-0012-agent-env`). The agent-asset domain lands in **`crates/agent-env`**; the
verb wiring lands in **`crates/engine`** (Engine API) + CLI/GUI. The component-grain analogs in
`crates/engine` (lock/runtime/doctor) are `- [≠]` (reuse-by-analogy, untouched).

**Row format:** `- [mark] <unit-id> · <class> · <ported-rust-symbol> · <landing> · -> <Y-target-symbol> · refs: · status`

**Status legend:** `- [ ]` ported/verified, not yet merged · `- [~]` merged into Y, re-verify unproven ·
`- [x]` merged + re-parity-verified in Y + Y green · `- [!] blocked` · `- [≠] intentional-divergence`.
**Only `- [x]` and `- [≠]` count toward merge-DONE.**

**Class key:** `port-fresh` = Y lacks it, land as new module in agent-env (or new Engine method) ·
`extend-Y` = complete a partial Y impl · `reuse-Y` = Y fully provides (verify-only) · `map-onto-substrate`.
**This merge: reuse-Y(direct)=0, extend-Y=0, port-fresh=99, reuse-Y(analogy/DO-NOT-RE-PORT)=3, front-end=13.**

---

## CROSS-CUTTING

- [~] XC-01 · port-fresh · agent-env::AgentEnvError/Result/err · merge-into agent-env::lib · -> envctl_agent_env::{AgentEnvError,Result,err} · refs: research#reuse-map · status: merged (seed 6ecb270), re-verify pending
- [~] XC-02 · port-fresh · agent-env::source::http_client · merge-into agent-env::source · -> envctl_agent_env::source::http_client · refs: research#no-direct-reuse · status: merged, re-verify pending
- [ ] XC-03 · port-fresh · agent-env::dirs::* · new agent-env::dirs (envctl-namespaced) · -> envctl_agent_env::dirs::* · refs: research#no-direct-reuse (engine has no dirs) · status: not merged
- [ ] XC-04 · port-fresh · agent-env::util::now_unix{,_str} · new agent-env::util · -> envctl_agent_env::util::now_unix · refs: research#no-direct-reuse · status: not merged

## src/model/* (config schema / agent presets / lock value types)

- [~] M-01 · port-fresh · agent-env::config::Scope · merge-into agent-env::config · -> envctl_agent_env::config::Scope · refs: · status: merged, re-verify pending
- [~] M-02 · port-fresh · agent-env::config::Config · merge-into agent-env::config · -> envctl_agent_env::config::Config · refs: · status: merged, re-verify pending
- [~] M-03 · port-fresh · agent-env::config::Config::{agents,resolved_scope} · merge-into agent-env::config · -> envctl_agent_env::config::Config · refs: · status: merged, re-verify pending
- [~] M-04 · port-fresh · agent-env::config::SourceSpec · merge-into agent-env::config · -> envctl_agent_env::config::SourceSpec · refs: · status: merged, re-verify pending
- [~] M-05 · port-fresh · agent-env::config::{GitPin,SourceSpec::git_pin} · merge-into agent-env::config · -> envctl_agent_env::config::{GitPin,git_pin_of} · refs: · status: merged, re-verify pending
- [~] M-06 · port-fresh · agent-env::config::SourceSpec::expected_revision · merge-into agent-env::config · -> envctl_agent_env::config::SourceSpec::expected_revision · refs: · status: merged, re-verify pending
- [~] M-07 · port-fresh · agent-env::config::{McpSourceSpec,CommandSourceSpec} · merge-into agent-env::config · -> envctl_agent_env::config::{McpSourceSpec,CommandSourceSpec} · refs: · status: merged, re-verify pending
- [~] M-08 · port-fresh · agent-env::config::{SkillsField,SkillTarget,McpsField,McpEntry,CommandsField,CommandEntry} · merge-into agent-env::config · -> envctl_agent_env::config::* · refs: · status: merged, re-verify pending
- [~] M-09 · port-fresh · agent-env::config::{Agent,AGENT_PRESETS} · merge-into agent-env::config · -> envctl_agent_env::config::{Agent,AGENT_PRESETS} · refs: · status: merged (enum shape), re-verify pending
- [~] M-10 · port-fresh · agent-env::config::AgentField · merge-into agent-env::config · -> envctl_agent_env::config::AgentField · refs: · status: merged, re-verify pending
- [~] M-11 · port-fresh · agent-env::config::Agent::global_path · merge-into agent-env::agent · -> envctl_agent_env::config::Agent::global_path · refs: · status: merged, re-verify pending (M-11 per-preset body)
- [~] M-12 · port-fresh · agent-env::config::Agent::project_path · merge-into agent-env::agent · -> envctl_agent_env::config::Agent::project_path · refs: · status: merged, re-verify pending
- [~] M-13 · port-fresh · agent-env::config::Agent::mcp_settings_target · merge-into agent-env::agent · -> envctl_agent_env::agent::Agent::mcp_settings_target · refs: · status: merged, re-verify pending
- [~] M-14 · port-fresh · agent-env::config::Agent::mcp_project_target · merge-into agent-env::agent · -> envctl_agent_env::agent::Agent::mcp_project_target · refs: · status: merged, re-verify pending
- [~] M-15 · port-fresh · agent-env::config::Agent::commands_global_path · merge-into agent-env::agent · -> envctl_agent_env::agent::Agent::commands_global_path · refs: · status: merged, re-verify pending
- [~] M-16 · port-fresh · agent-env::config::Agent::commands_project_path · merge-into agent-env::agent · -> envctl_agent_env::agent::Agent::commands_project_path · refs: · status: merged, re-verify pending
- [~] M-17 · port-fresh · agent-env::{all_mcp_settings_targets,all_mcp_project_targets} · merge-into agent-env::agent · -> envctl_agent_env::agent::{all_mcp_settings_targets,all_mcp_project_targets} · refs: · status: merged (agent.rs:78,89), re-verify pending
- [~] M-18 · port-fresh · agent-env::{all_command_global_targets,all_command_project_targets} · merge-into agent-env::agent · -> envctl_agent_env::agent::{all_command_global_targets,all_command_project_targets} · refs: · status: merged (agent.rs:117,122), re-verify pending
- [~] M-19 · port-fresh · agent-env::{command_global_targets,command_project_targets} · merge-into agent-env::agent · -> envctl_agent_env::agent::{command_global_targets,command_project_targets} · refs: · status: merged (agent.rs:132,137), re-verify pending
- [~] M-20 · port-fresh · agent-env::config (private helpers) · merge-into agent-env::agent · -> envctl_agent_env::agent::{dedup_targets,cmd,vscode_user_mcp_json,mcp_servers_target} · refs: · status: merged (agent.rs:98-164), re-verify pending
- [~] M-21 · port-fresh · agent-env::config::resolve_scope · merge-into agent-env::config · -> envctl_agent_env::config::resolve_scope · refs: · status: merged (CLI>cfg>Global), re-verify pending; file-read fallback = M-22
- [ ] M-22 · port-fresh · agent-env::config::resolve_scope (fallback arm) · merge-into agent-env::config · -> envctl_agent_env::config::resolve_scope (fallback) · refs: · status: not merged (deferred arm, TASK-0013)
- [~] M-23 · port-fresh · agent-env::lock::AgentLockEntry (folded) · merge-into agent-env::lock · -> envctl_agent_env::lock::AgentLockEntry · refs: research#D1 · status: merged, re-verify pending
- [~] M-24 · port-fresh · agent-env::lock (version/state) · merge-into agent-env::lock · -> envctl_agent_env::lock::{LOCK_VERSION,AgentLockFile} · refs: research#D1 · status: merged, re-verify pending
- [~] M-25 · port-fresh · agent-env::report::{Summary,Action,Report,InstalledSkill,SyncFailure} · merge-into agent-env::report · -> envctl_agent_env::report::* · refs: · status: merged (report.rs), re-verify pending
- [~] M-26 · port-fresh · agent-env::config::{McpSettingsFormat,McpSettingsTarget} · merge-into agent-env::agent · -> envctl_agent_env::agent::{McpSettingsFormat,McpSettingsTarget} · refs: research#left-behind (consumed by MC-01) · status: merged (agent.rs:28,41), re-verify pending
- [~] M-27 · port-fresh · agent-env::config::{CommandFormat,CommandTarget} · merge-into agent-env::agent · -> envctl_agent_env::agent::{CommandFormat,CommandTarget} · refs: research#left-behind (consumed by PR-01) · status: merged (agent.rs:52,67), re-verify pending

## src/fsops/* (hash / config loader / copy / settings / edit / target selection)

- [~] F-01 · port-fresh · agent-env::hash::hash_dir · merge-into agent-env::hash · -> envctl_agent_env::hash::hash_dir · refs: research#D2 (engine has only fnv1a) · status: merged (hash.rs:18), re-verify pending
- [~] F-02 · port-fresh · agent-env::hash::{hash_str,hash_file} · merge-into agent-env::hash · -> envctl_agent_env::hash::{hash_str,hash_file} · refs: research#D2 · status: merged (hash.rs:45,52), re-verify pending
- [~] X-01 · port-fresh · agent-env::extend::extract_extends · merge-into agent-env::extend · -> envctl_agent_env::extend::extract_extends · refs: research#D4 · status: merged, re-verify pending
- [~] X-02 · port-fresh · agent-env::extend::merge_yaml · merge-into agent-env::extend · -> envctl_agent_env::extend::merge_yaml · refs: research#D4 (not in engine) · status: merged (extend.rs:52), re-verify pending
- [~] X-03 · port-fresh · agent-env::extend (merge internals) · merge-into agent-env::extend · -> envctl_agent_env::extend::merge_source_list · refs: · status: merged (extend.rs:78), re-verify pending
- [~] CFG-01 · port-fresh · agent-env::extend::load_config_any · merge-into agent-env::extend · -> envctl_agent_env::extend::load_config_any · refs: research#D4 · status: merged (extend.rs:123), re-verify pending
- [~] CFG-02 · port-fresh · agent-env::extend::load_config_recursive · merge-into agent-env::extend · -> envctl_agent_env::extend::{load_config_recursive,MAX_EXTENDS_DEPTH} · refs: · status: merged, re-verify pending
- [~] CFG-03 · port-fresh · agent-env::extend::fetch_config_text · merge-into agent-env::extend · -> envctl_agent_env::extend::fetch_config_text · refs: · status: merged, re-verify pending
- [ ] F-03 · port-fresh · agent-env::fsops::copy_dir · new agent-env::fsops · -> envctl_agent_env::fsops::{copy_dir,copy_dir_contents,copy_file} · refs: research#no-direct-reuse (engine has no copy) · status: not merged
- [ ] F-04 · port-fresh · agent-env::fsops::SettingsFile · new agent-env::fsops · -> envctl_agent_env::fsops::SettingsFile · refs: research#left-behind (consumed by MC-01) · status: not merged
- [ ] F-05 · port-fresh · agent-env::fsops::resolve_path · new agent-env::fsops · -> envctl_agent_env::fsops::resolve_path · refs: · status: not merged
- [ ] F-06 · port-fresh · agent-env::fsops::select_targets · new agent-env::fsops · -> envctl_agent_env::fsops::{select_targets,BrokenSkill,TargetSelection} · refs: · status: not merged
- [ ] F-07 · port-fresh · agent-env::fsops::resolve_destinations · new agent-env::fsops · -> envctl_agent_env::fsops::resolve_destinations · refs: · status: not merged
- [ ] F-08 · port-fresh · agent-env::fsops::resolve_mcp_settings_targets · new agent-env::fsops · -> envctl_agent_env::fsops::resolve_mcp_settings_targets · refs: · status: not merged
- [ ] F-09 · port-fresh · agent-env::fsops::resolve_command_targets · new agent-env::fsops · -> envctl_agent_env::fsops::resolve_command_targets · refs: · status: not merged
- [ ] F-10 · port-fresh · agent-env::fsops::{scope_root,relativize_dest,resolve_dest} · new agent-env::fsops (agent-asset variant; engine's scope logic is component-grain, not reused) · -> envctl_agent_env::fsops::{scope_root,relativize_dest,resolve_dest} · refs: research#no-direct-reuse · status: not merged
- [ ] FE-01 · port-fresh · agent-env::config_edit::{Section,Pin,Selector,SourceItem,RemoveOutcome} · new agent-env::config_edit · -> envctl_agent_env::config_edit::* · refs: · status: not merged
- [ ] FE-02 · port-fresh · agent-env::config_edit::insert_item · new agent-env::config_edit · -> envctl_agent_env::config_edit::insert_item · refs: · status: not merged
- [ ] FE-03 · port-fresh · agent-env::config_edit::remove_item · new agent-env::config_edit · -> envctl_agent_env::config_edit::remove_item · refs: · status: not merged
- [ ] FE-04 · port-fresh · agent-env::config_edit::remove_names · new agent-env::config_edit · -> envctl_agent_env::config_edit::remove_names · refs: · status: not merged
- [ ] FE-05 · port-fresh · agent-env::config_edit::{item_exists,render_item} · new agent-env::config_edit · -> envctl_agent_env::config_edit::{item_exists,render_item} · refs: · status: not merged
- [ ] FE-06 · port-fresh · agent-env::config_edit (line primitives) · new agent-env::config_edit · -> envctl_agent_env::config_edit::{parse_items,find_top_level,...} · refs: · status: not merged

## src/source/* (multi-host resolver / URL / archive / auth / discovery)

- [~] S-01 · port-fresh · agent-env::source::hosts · merge-into agent-env::source · -> envctl_agent_env::source (host classifiers) · refs: · status: merged, re-verify pending
- [~] S-02 · port-fresh · agent-env::source::{RepoUrl,parse_repo_url} · merge-into agent-env::source · -> envctl_agent_env::source::{RepoUrl,parse_repo_url} · refs: · status: merged (source.rs:105), re-verify pending
- [~] S-03 · port-fresh · agent-env::source::{BrowseDerived,derive_browse_url} · merge-into agent-env::source · -> envctl_agent_env::source::{BrowseDerived,derive_browse_url} · refs: · status: merged, re-verify pending
- [~] S-04 · port-fresh · agent-env::source::archive_url (branch arm) · merge-into agent-env::source · -> envctl_agent_env::source::archive_url · refs: · status: merged (source.rs:424), re-verify pending; retry=S-15
- [~] S-05 · port-fresh · agent-env::source::archive_url (ref arm) · merge-into agent-env::source · -> envctl_agent_env::source::archive_url · refs: · status: merged, re-verify pending
- [~] S-06 · port-fresh · agent-env::source (url encoders) · merge-into agent-env::source · -> envctl_agent_env::source (encode_gitlab_path/encode_github_ref) · refs: · status: merged, re-verify pending
- [~] S-07 · port-fresh · agent-env::source::download_extract · merge-into agent-env::source · -> envctl_agent_env::source::download_extract · refs: · status: merged (pure-Rust flate2+tar), re-verify pending
- [~] S-08 · port-fresh · agent-env::source::rewrite_browse_to_raw_url · merge-into agent-env::source · -> envctl_agent_env::source::rewrite_browse_to_raw_url · refs: · status: merged, re-verify pending
- [~] S-09 · port-fresh · agent-env::source::UrlRequestAuth · merge-into agent-env::source · -> envctl_agent_env::source::UrlRequestAuth · refs: · status: merged, re-verify pending
- [~] S-10 · port-fresh · agent-env::source (env cred readers) · merge-into agent-env::source · -> envctl_agent_env::source (github/gitlab/gitea auth headers) · refs: · status: merged, re-verify pending
- [~] S-11 · port-fresh · agent-env::source::auth_for_request_url · merge-into agent-env::source · -> envctl_agent_env::source::auth_for_request_url · refs: · status: merged, re-verify pending
- [~] S-12 · port-fresh · agent-env::source::auth_env_inline_help · merge-into agent-env::source · -> envctl_agent_env::source::auth_env_inline_help · refs: · status: merged, re-verify pending
- [~] S-13 · port-fresh · agent-env::source::http_fetch_auth_hint · merge-into agent-env::source · -> envctl_agent_env::source::http_fetch_auth_hint · refs: · status: merged, re-verify pending
- [ ] S-14 · port-fresh · agent-env::source::materialize_source · merge-into agent-env::source · -> envctl_agent_env::source::{materialize_source,MaterializedSource} · refs: · status: not merged
- [ ] S-15 · port-fresh · agent-env::source::materialize_source (Default main→master retry arm) · merge-into agent-env::source · -> envctl_agent_env::source::materialize_source (retry) · refs: · status: not merged (deferred remainder)
- [ ] S-16 · port-fresh · agent-env::source::{resolve_source_root,repo_name_hint} · merge-into agent-env::source · -> envctl_agent_env::source::{resolve_source_root,repo_name_hint} · refs: · status: not merged
- [ ] S-17 · port-fresh · agent-env::source::discover · merge-into agent-env::source · -> envctl_agent_env::source::{discover,discover_with_root_name,discover_skills_in_subdir} · refs: · status: not merged
- [ ] S-18 · port-fresh · agent-env::source::discover_mcps · merge-into agent-env::source · -> envctl_agent_env::source::discover_mcps · refs: research#left-behind (feeds C-04/MC-01) · status: not merged
- [ ] S-19 · port-fresh · agent-env::source::resolve_mcp_entry · merge-into agent-env::source · -> envctl_agent_env::source::resolve_mcp_entry · refs: · status: not merged
- [ ] S-20 · port-fresh · agent-env::source::discover_commands · merge-into agent-env::source · -> envctl_agent_env::source::{discover_commands,walk_commands} · refs: research#left-behind (feeds C-03/PR-01) · status: not merged
- [ ] S-21 · port-fresh · agent-env::source::resolve_command_entry · merge-into agent-env::source · -> envctl_agent_env::source::{resolve_command_entry,resolve_named_command} · refs: · status: not merged

## src/lock.rs (SHA-256 agent-asset lock — separate from engine FNV-1a component lock)

- [~] L-01 · port-fresh · agent-env::lock::AssetEntry · merge-into agent-env::lock · -> envctl_agent_env::lock::AssetEntry · refs: research#D1 (distinct from engine LockEntry) · status: merged, re-verify pending
- [~] L-02 · port-fresh · agent-env::lock::AgentLockFile · merge-into agent-env::lock · -> envctl_agent_env::lock::{AgentLockFile,AGENT_ASSETS_KEY} · refs: research#D1 · status: merged (lock.rs:73), re-verify pending
- [~] L-03 · port-fresh · agent-env::lock::AgentLockFile (methods) · merge-into agent-env::lock · -> envctl_agent_env::lock::AgentLockFile::{get/save/remove/list_tracked_asset,clear_all} · refs: · status: merged (lock.rs:159-185), re-verify pending
- [~] L-04 · port-fresh · agent-env::lock::lock_path · merge-into agent-env::lock · -> envctl_agent_env::lock::lock_path · refs: research#D1 (file=agent-env.lock, not envctl.lock) · status: merged (lock.rs:236), re-verify pending
- [~] L-05 · port-fresh · agent-env::lock::{load,save} · merge-into agent-env::lock · -> envctl_agent_env::lock::{load,save} · refs: · status: merged (lock.rs:244,260), re-verify pending
- [~] L-06 · port-fresh · agent-env::lock::{LockMode,lock_check,LockDrift} · merge-into agent-env::lock · -> envctl_agent_env::lock::{LockMode,lock_check,LockDrift,DriftStatus} · refs: research#D1 (separate from engine diff) · status: merged (lock.rs:98,193), re-verify pending; command-level drift = C-09 remainder

## src/state.rs (machine-local agent-sync runtime — preserve lock↔runtime separation)

- [ ] ST-01 · port-fresh · agent-env::runtime::RuntimeState · new agent-env::runtime (MIRROR engine's separation, do NOT fold into engine::runtime — different payload) · -> envctl_agent_env::runtime::RuntimeState · refs: research#D3, AP-02 · status: not merged
- [ ] ST-02 · port-fresh · agent-env::runtime::* · new agent-env::runtime · -> envctl_agent_env::runtime::{runtime_state_path,load,save,clear} · refs: research#D3 · status: not merged

## src/profile.rs (SKILL.md metadata)

- [ ] P-01 · port-fresh · agent-env::profile::read_skill_profile · new agent-env::profile · -> envctl_agent_env::profile::{read_skill_profile,read_skill_profile_from_dir} · refs: · status: not merged
- [ ] P-02 · port-fresh · agent-env::profile::format_updated_ago · new agent-env::profile · -> envctl_agent_env::profile::format_updated_ago · refs: · status: not merged

## src/lib.rs (config-path resolution)

- [ ] CP-01 · port-fresh · agent-env::config_path::default_config_path · new agent-env::config_path (rename KASETTO_CONFIG→envctl) · -> envctl_agent_env::config_path::{default_config_path,resolve_config_path,Preferences} · refs: · status: not merged

## src/commands/* (business logic → Engine methods; clap glue = front-end)

- [ ] C-01 · port-fresh · Engine::agent_sync · merge-into engine (new Engine method, drives agent-env) · -> envctl_engine::Engine::agent_sync · refs: research#engine-grain (verbs are agent-asset, parallel to component verbs) · status: not merged
- [ ] C-02 · port-fresh · agent-sync skills phase · merge-into engine (agent_sync internals) · -> envctl_engine (sync_skills) · refs: · status: not merged
- [ ] C-03 · port-fresh · agent-sync commands phase · merge-into engine (agent_sync internals; needs PR-01) · -> envctl_engine (sync_commands) · refs: research#left-behind PR-01 (dep) · status: not merged
- [ ] C-04 · port-fresh · agent-sync mcps phase · merge-into engine (agent_sync internals; needs MC-01) · -> envctl_engine (sync_mcps) · refs: research#left-behind MC-01 (dep) · status: not merged
- [ ] C-05 · port-fresh · agent-env::sync::remove_stale (shared) · new agent-env::sync · -> envctl_agent_env::sync::remove_stale · refs: · status: not merged
- [ ] C-06 · port-fresh · agent-env::sync (key conventions) · new agent-env::sync · -> envctl_agent_env::sync (skill_key/asset-id conventions) · refs: · status: not merged
- [ ] C-07 · port-fresh · Engine::agent_add · merge-into engine (new Engine method) · -> envctl_engine::Engine::agent_add · refs: · status: not merged
- [ ] C-08 · port-fresh · Engine::agent_remove · merge-into engine (new Engine method, alias rm) · -> envctl_engine::Engine::agent_remove · refs: · status: not merged
- [ ] C-09 · port-fresh · Engine::agent_lock (+--check/--upgrade-package) · merge-into engine (new Engine method; NOT the component `lock --check`) · -> envctl_engine::Engine::agent_lock · refs: research#engine-grain (distinct from cli Cmd::Lock@main.rs:269) · status: not merged
- [ ] C-10 · port-fresh · Engine::agent_list · merge-into engine (new Engine method) · -> envctl_engine::Engine::agent_list · refs: · status: not merged
- [ ] C-11 · port-fresh · Engine::agent_clean · merge-into engine (new Engine method; needs MC-02 remove) · -> envctl_engine::Engine::agent_clean · refs: research#left-behind MC-02 (dep) · status: not merged
- [ ] C-12 · port-fresh · agent-env::config_edit::{resolve_local_config_path,split_at_ref}+sync_after · new agent-env::config_edit + engine · -> envctl_agent_env::config_edit::* + Engine sync_after · refs: · status: not merged
- [ ] C-13 · port-fresh · Engine::agent_init_template · merge-into engine (folded into add/sync) · -> envctl_engine::Engine (init template) · refs: · status: not merged
- [ ] C-14 · port-fresh · Engine::agent_clean (asset portion only) · merge-into engine (binary-self-removal is front-end) · -> envctl_engine::Engine::agent_clean (asset cleanup) · refs: · status: not merged

## LEFT-BEHIND — discovered by the sweep (referenced as deps, never rowed)

- [ ] MC-01 · port-fresh · agent-env::mcps::merge_mcp_config (+ merge/codex/pack) · new agent-env::mcps (mod+merge+codex+pack) · -> envctl_agent_env::mcps::merge_mcp_config · refs: research#left-behind (src/mcps/{mod:16,merge:39-53,codex:11,pack:8}; dep of C-04/MC consumers) · status: not merged — MISSING from parity ledger; 4-format additive MCP merge (mcpServers/VsCode/OpenCode/CodexToml), never-clobber. Depends on F-04 SettingsFile.
- [ ] MC-02 · port-fresh · agent-env::mcps::{remove_mcp_server,servers_present_in_settings} · new agent-env::mcps · -> envctl_agent_env::mcps::{remove_mcp_server,servers_present_in_settings} · refs: research#left-behind (src/mcps/mod:30,58 + codex:35,47; dep of C-11/clean) · status: not merged — MISSING from parity ledger; per-format server removal + presence (drives needs_fetch_mcps + clean).
- [ ] PR-01 · port-fresh · agent-env::prompts::apply_command (+ parse + transform) · new agent-env::prompts (mod+parse+transform) · -> envctl_agent_env::prompts::apply_command · refs: research#left-behind (src/prompts/{mod:18,parse:28,transform:40-110}; dep of C-03) · status: not merged — MISSING from parity ledger; Markdown parse + 5-format render (MarkdownFrontmatter nested-path / MarkdownPlain / PromptMd / PromptFile `{{{ input }}}`+`invokable:true` / GeminiToml) + dest-path/ensure-parent. Depends on M-27 CommandTarget/CommandFormat.

## FRONT-END — envctl owns rendering (intentional divergence)

- [≠] FRONTEND-01 · front-end · src/cli.rs clap wiring · envctl CLI (TASK-0014) · -> envctl CLI (`envctl agent {sync,add,remove,lock,list,clean}`) · refs: · status: intentional-divergence (verb semantics = C-01..C-14)
- [≠] FRONTEND-02 · front-end · src/app.rs dispatch · envctl CLI · -> envctl CLI dispatch into Engine · refs: · status: intentional-divergence
- [≠] FRONTEND-03 · front-end · src/main.rs / bin/kst.rs · envctl bins · -> envctl bins · refs: · status: intentional-divergence
- [≠] FRONTEND-04 · front-end · src/ui.rs terminal rendering · envctl Event/printer · -> envctl CLI/GUI rendering · refs: · status: intentional-divergence (non-printing Engine emits Events)
- [≠] FRONTEND-05 · front-end · src/banner.rs · envctl banner · -> envctl CLI · refs: · status: intentional-divergence
- [≠] FRONTEND-06 · front-end · src/colors.rs · envctl palette · -> envctl CLI · refs: · status: intentional-divergence
- [≠] FRONTEND-07 · front-end · src/update_notifier.rs · envctl own update · -> envctl · refs: · status: intentional-divergence (do NOT port)
- [≠] FRONTEND-08 · front-end · src/commands/self_update.rs · envctl own self-update · -> envctl · refs: · status: intentional-divergence
- [≠] FRONTEND-09 · reuse-Y(analogy) · src/commands/doctor.rs · envctl existing doctor (no re-port) · -> print_doctor (crates/cli/src/main.rs:876) · refs: research#engine-already-absorbed · status: intentional-divergence (probe logic covered by M-15/16/19)
- [≠] FRONTEND-10 · front-end · src/commands/completions.rs · envctl completions · -> envctl CLI · refs: · status: intentional-divergence

## ALREADY-PORTED in envctl engine — DO NOT re-port (reuse-by-analogy, component grain)

- [≠] AP-01 · reuse-Y(analogy) · kasetto §2 component-lock analog · engine lock untouched · -> envctl_engine::lock (FNV-1a, envctl.lock, crates/engine/src/lock.rs:134/164; CLI Cmd::Lock@main.rs:269) · refs: research#D1 (verified NOT a duplicate of agent-env SHA-256 lock) · status: intentional-divergence — separate keyed domain; rehashing components under SHA-256 = downgrade
- [≠] AP-02 · reuse-Y(analogy) · kasetto §16 runtime analog · engine runtime untouched · -> envctl_engine::runtime (crates/engine/src/runtime.rs:11) · refs: research#D3 · status: intentional-divergence — ST-01/02 MIRROR the separation in agent-env, do not fold (different payload)
- [≠] AP-03 · reuse-Y(analogy) · kasetto doctor analog · engine doctor untouched · -> print_doctor (crates/cli/src/main.rs:876) · refs: research#engine-already-absorbed · status: intentional-divergence — workstation doctor, not agent-asset; no re-port

---

## Count summary

| Class | Count |
|---|---|
| `port-fresh` (merged `- [~]`, re-verify pending) | 55 |
| `port-fresh` (todo `- [ ]`, ABSORB into agent-env/engine) | 41 |
| `port-fresh` (left-behind `- [ ]`, NEW rows) | 3 (MC-01, MC-02, PR-01) |
| `reuse-Y` (direct symbol) | 0 |
| `extend-Y` | 0 |
| `map-onto-substrate` | 0 |
| `reuse-Y`/front-end (`- [≠]`) | 13 (10 FRONTEND + 3 AP) |
| **Total merge rows** | **112 parity + 3 left-behind = 115** |

**Duplications found: 0** (lock collision verified distinct, research#D1). **Left-behind found: 3**
(MC-01/MC-02/PR-01, 7 source files — research#left-behind). **Merge-DONE** requires every `- [~]`/`- [ ]`
→ `- [x]` (re-verified in Y + Y green) and all 3 left-behind rows ported+merged+verified.
