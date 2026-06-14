# Parity ledger — kasetto → envctl-agent-env (no-feature-left-behind contract)

- **SOURCE:** `meta/kasetto` @ **v3.2.0** (git tag `v3.2.0`, `ec01cca release: v3.2.0`, Cargo 3.2.0). Pure-Rust CLI; module tree `src/{model,source,fsops,commands,mcps,prompts,bin}/` + top-level `src/{cli,app,state,lock,profile,ui,banner,colors,update_notifier,error,lib,main}.rs`.
- **TARGET:** envctl crate `crates/agent-env` (package `envctl-agent-env`) in worktree `/home/drdave/Desktop/meta/.worktrees/task-0012-agent-env/envctl`.
- **SEED:** commit `6ecb270` ports the foundational modules `crates/agent-env/src/{config,extend,source,hash,lock,lib}.rs` + `tests/no_downgrade.rs` (61 unit + 1 integration test GREEN). Seed-credited rows are `- [~]`; partial-remainder rows are explicit `- [ ]`.
- **DESIGN CORPUS:** `.claude/skills/rust-feature-impl/references/kasetto-absorption.md` (11→6 verb map, no-downgrade checklist), `.handoff/decisions/ADR-0001`, `docs/KASETTO-FEATURES.md` (stale @ v3.0.0 — SOURCE wins).

## Scope boundary (ABSORPTION, not verbatim clone)

kasetto is a Rust CLI; envctl absorbs its **logic** and renders via its **own** Engine + CLI/GUI. Three classes:
- **ABSORB** (`- [ ]` / `- [~]`): model schema, source resolver, fsops, lock-mode logic, MCP merge, command transforms, command business logic.
- **FRONT-END** (`- [≠] front-end: envctl owns rendering`): `ui.rs`, `banner.rs`, `colors.rs`, clap wiring (`cli.rs`/`app.rs`/`main.rs`/`completions.rs`), `update_notifier.rs`, `self_update.rs`. Recorded so the pre-DONE sweep treats them as intentionally-diverged, NOT missed. Their VERB SEMANTICS are captured as absorb rows under commands.
- **SEED-CREDITED** (`- [~]`): everything in commit 6ecb270 (config/extend/source/hash/lock foundational). The parity-verifier upgrades these to `- [x]` on differential PASS.

## Status legend

| Mark | Meaning | Who sets it |
|------|---------|-------------|
| `- [ ]` | not ported | cartographer (seed) |
| `- [~]` | ported, parity **unproven** (or partial) | porter / seed |
| `- [x]` | ported **and** differentially parity-verified | orchestrator, on verifier PASS |
| `- [!] blocked: <reason>` | can't proceed | any |
| `- [≠] <reason>` | intentional divergence (front-end) | cartographer (scope boundary) / owner |

**Only `- [x]` and `- [≠]` count toward DONE.** A `- [~]` is an unproven claim.

Row format: `- [ ] <id> · <source-path>:<symbol> · <contract> · -> <rust-target> · deps: <ids|none>`

---

## Parity-verifier pass — 2026-06-14 (cluster: C-* SYNC ENGINE — Engine integration tests)

**+6 rows flipped `→ [x]`** (80→86). FIRST Engine-integration cluster: the kasetto `sync` command logic
lives in `crates/engine/src/agent/*` (TASK-0013), so verified via `crates/engine/tests/agent_sync_parity.rs`
(NEW, +15 tests) driving `Engine::agent_sync` over identical kasetto fixtures. `cargo test -p envctl-engine`
**74 passed** (baseline 59); agent-env still 311; no-c PASS; fmt clean. Reproduced all 13 kasetto certified
vectors (7 skills + 3 commands + 3 mcps) + 2 cross-cutting. Proven: **C-01** sync orchestrator (dry_run
zero-writes; `--locked`+`--update` contradiction is now a COMPILE-TIME impossibility via mutually-exclusive
`AgentLockMode` — kasetto's runtime error hardened, an UPGRADE not a downgrade), **C-02** sync_skills
(first-run/unchanged, tampered-dest repair, multi-dest local repair w/o source, wildcard holds-to-locked +
update-prunes, locked errors-when-absent / succeeds-satisfiable), **C-03** sync_commands (supported-agent
transforms + skip-unsupported + empty-block prune; unchanged-no-fetch; locked-error), **C-04** sync_mcps
(**ADDITIVE/never-clobber — pre-existing broker/repowire/weave SURVIVE a sync that adds servers; the #1
no-downgrade invariant**; no-new-servers unchanged; needs_fetch), **C-05** remove_stale + never-prune-on-
failure (`if summary.failed==0` guard), **C-06** lock-key conventions (`<source>::<name>` /
`command::{src}::{name}` / `mcp::{src}::{file}` — proved transitively, a key divergence would surface as
re-install/spurious-locked-error in the 15 tests). **0 BLOCKED.** Remaining 10 `[ ]` = M-22 + S-15
(network/engine residue) + C-07..C-14 (add/remove/lock/list/clean/init/source_edit/uninstall verbs).

## Parity-verifier pass — 2026-06-14 (cluster: runtime-state / profile / dirs / config-path leaves)

**+6 rows flipped `→ [x]`** (74→80). 7 new test fns added to `parity_vs_kasetto.rs` (suite 75→82);
`cargo test -p envctl-agent-env` **311 passed, 1 ignored** (baseline 304). Verbatim kasetto v3.2.0
golden vectors through agent-env's public API. Proven: **XC-03** dirs/XDG resolution (HOME-unset ERR;
XDG_*_HOME honored only when non-empty else `$HOME/.config|.local/share|.cache`), **ST-01/02** runtime
state round-trip + load_latest_failures (broken|source_error extraction; lock↔runtime separation
ADR-0001 §4; path = cache `runtime/{hash_str(lock_path)}.json`), **P-01** read_skill_profile (frontmatter
desc wins, `#` heading→title, missing→fallback), **P-02** format_updated_ago (Ns/m/h/d ago, in Ns,
unknown), **CP-01** config-path priority ($ENV > local > prefs source > global > local fallback).
**0 BLOCKED.** envctl renames verified+asserted (NOT downgrades — same logic, envctl-namespaced):
app-dir `kasetto`→`agent-env`; env var `KASETTO_CONFIG`→`ENVCTL_AGENT_CONFIG`; filename
`kasetto.yaml`→`agent-env.yaml`. Remaining 16 `[ ]` = M-22 + S-15 (engine/network residue) + C-01..C-14
(command orchestrators — verify via Engine integration tests, which also close the 6 `[~]` residue).

## Parity-verifier pass — 2026-06-14 (cluster: fsops + config_edit mutation engine)

**+14 rows flipped `→ [x]`** (60→74). 29 new test fns added to `parity_vs_kasetto.rs` (suite 46→75);
`cargo test -p envctl-agent-env` **304 passed, 1 ignored** (baseline 275). Verbatim kasetto v3.2.0
golden vectors through agent-env's public API. Proven: **F-03** copy_dir (+x bit, symlink-follow,
dst-clean — unix arms PASS on Linux), **F-04** SettingsFile load/save (+ invalid-JSON ERR), **F-05**
resolve_path (leading-`~` only), **F-06** select_targets (wildcard sort, broken-skill, path override,
invalid-field ERR), **F-07/08/09** resolve_destinations / resolve_mcp_settings_targets /
resolve_command_targets (per-agent by scope, dedup, no-agents ERR), **F-10** scope_root/relativize_dest/
resolve_dest round-trip (lock portability core), **FE-01…FE-06** the comment-preserving config_edit
mutation engine (insert_item byte-exact incl. inline-list normalize + empty-file + 4-space indent;
remove_item disambiguate/sub_dir; remove_names collapse/ERR arms; item_exists; raw-line primitives
proved transitively via byte-exact FE-02/03/04 fixtures). **0 BLOCKED.** Residue: F-03 `cfg(windows)`
READONLY-strip arm unreachable on Linux (platform, not faked); render_item/FE-06 primitives are private
→ proved transitively (no stub).

## Parity-verifier pass — 2026-06-14 (cluster: model schema + 21-preset table + config loader + SHA-256 lock)

**+27 rows flipped `→ [x]`** (33→60). 27 new test fns added to `parity_vs_kasetto.rs` (suite 19→46);
`cargo test -p envctl-agent-env` **275 passed, 1 ignored** (baseline 248). Verbatim kasetto v3.2.0
golden vectors through agent-env's public API. Proven: **M-01/03/04/06/07** (Scope/Config/SourceSpec
schema + serde), **M-09…M-14** (the 21-agent-preset path table — global/project skill+MCP paths for all
21 presets incl. the github-copilot OS-branch — the #1 no-downgrade row), **M-17/19/20/21** (target
dedup+sort, scoped command targets, resolve_scope CLI>cfg>Global), **M-23/24/25/26** (lock-value types,
report types, McpSettingsFormat), **CFG-01/02** (load_config_any + recursive `extends` chain/cycle/depth),
**L-01…L-06** (AssetEntry, LockFile round-trip incl. legacy-v1 restamp, lock_path, load/save, lock_check
diff + LockMode). 3 rows carry a residue note (design-fold/network — see ⟨⟩ inline): M-24 `State` folded
into runtime/driver; L-03 `list_installed_*` re-derived in driver; **CFG-03 stays `[~]`** (remote http
arm network-only — local arm proven; close via `Engine::agent_sync` integration). No stub, no fake.

## Parity-verifier pass — 2026-06-14 (cluster: source-resolver auth/discovery)

**+11 rows flipped `→ [x]`** by differential golden-vector verification (kasetto v3.2.0 `ec01cca`'s
own `#[cfg(test)]` modules fed through agent-env's public API). 7 new test fns added to
`crates/agent-env/tests/parity_vs_kasetto.rs`; suite 12→19 fns; `cargo test -p envctl-agent-env`
**248 passed, 1 ignored** (baseline 241). Proven: **S-09/S-10/S-11** (env cred readers + auth_for_request_url
via `archive_url(...).1: UrlRequestAuth` — GitHub Bearer / GitLab PRIVATE-TOKEN+JOB-TOKEN / Bitbucket
basic / Gitea token; GitLab repo refuses GitHub Bearer), **S-14/S-16** (materialize_source local arm +
resolve_source_root sub-dir), **S-17** (discover skills root/hint/subdir), **S-18** (discover_mcps),
**S-19** (resolve_mcp_entry), **S-20/S-21** (discover_commands + resolve_command_entry namespaced),
**XC-04** (now_unix/now_unix_str).

**4 rows remain `[~]` — parity-by-in-module-oracle, NOT cross-crate differential (honest, not faked):**
`S-07` (download_extract tar-slip guard — `pub(crate) extract_tar_gz`, network-only entry; covered by
agent-env in-crate `extract_rejects_parent_dir_traversal`), `S-12` (auth_env_inline_help — `pub(crate)`,
no public seam), `S-13` (http_fetch_auth_hint — `pub(crate)`, surfaces only in network error message),
`S-15` (materialize_source main→master retry — live second HTTP, `#[ignore]` network in both kasetto
and agent-env). Each verified by its own verbatim in-module vector; close via the **Engine integration
surface** (`Engine::agent_sync` exercises materialize/download — TASK-0013 merged) in a later cycle, or a
`pub` test seam. The retry/guard LOGIC is present and source-visible; only the offline cross-crate
differential is blocked.

## Parity-verifier pass — 2026-06-13 (`/verify`, independent differential vs kasetto v3.2.0)

22 rows flipped `→ [x]` by **runtime differential observation**, not code review. Method: kasetto's
own `#[cfg(test)]` golden vectors (the source's *certified* behavior — `cargo test` @ `meta/kasetto`
v3.2.0 `ec01cca`: **216 passed**) were fed through agent-env's **public API**; agent-env reproduced
every value (`cargo test -p envctl-agent-env --test parity_vs_kasetto`: **12 suites / ~55 golden
assertions, 0 mismatches**). Two headline items were additionally cross-checked against the **live
kasetto binary** (true black box): `kasetto add <blob-url>` derived `source/branch=main/sub-dir=skills/personal/skills=[edit-article]` *identically* to agent-env `derive_browse_url` (S-03); and
`kasetto lock` hashed a local skill to `7795ac…916d69`, *identical* to an independent SHA-256 framing
oracle that agent-env `hash_dir` also matched (F-01, 3-way). Harness: `crates/agent-env/tests/parity_vs_kasetto.rs`.

| Rows `→ [x]` | Differential evidence (oracle = kasetto src + live binary) |
|---|---|
| S-01,S-02 | `parse_repo_url`: 4-host classification (github/GHE/gitlab-subgroup/gitlab.com/bitbucket/codeberg), `.git`+slash trim — `kasetto src/source/parse.rs::tests` |
| S-03 | `derive_browse_url`: blob/SKILL.md split, tree path, 40-hex SHA→pinned, gitlab `/-/`, plain/local→None — `parse.rs::tests` **+ live `kasetto add`** |
| S-04,S-05,S-06 | `archive_url`: GH web vs api(token) branch+ref, `%2F` ref-encode, bitbucket, gitea — `src/source/remote.rs::tests` |
| S-08 | `rewrite_browse_to_raw_url`: GH blob/raw, gitea src/branch+tag, gitlab `/-/blob`→api/v4 raw, non-http→None — `remote.rs::tests` |
| F-01 | `hash_dir`: rel\0content\0 framing == independent SHA-256 == **live `kasetto lock`** (3-way) — `src/fsops/hash.rs` |
| F-02 | `hash_str`/`hash_file` == published SHA-256 (`"hello"`, empty) — 3rd-party oracle |
| X-01,X-02,X-03 | `extract_extends` (string/list/absent), `merge_yaml` scalar-replace + identity merge (source/ref/sub-dir, mcps, commands) — `src/model/extend.rs::tests` |
| M-02,M-08 | `Config` 6-key parse + untagged `CommandsField`/`CommandEntry` (wildcard/Name/Obj), `sub_dir` alias — `src/model/config.rs::tests` |
| M-05 | `git_pin_of` precedence ref>branch>Default — `config.rs` git_pin |
| M-15,M-16,M-18 | `Agent::commands_{global,project}_path` table (claude/windsurf/gemini/cursor/copilot/codex/warp/trae), `all_command_global_targets` dedup+sort — `src/model/agent.rs::tests` |
| M-27 | `CommandFormat` 5-variant transforms — exercised via PR-01 |
| MC-01,MC-02 | `merge_mcp_config` **additive/no-clobber** (preserve existing servers, keep real secret over `__FROM_SOURCE_PACK__`), CodexToml `[mcp_servers]` — `src/mcps/mod.rs::tests` (#1 no-downgrade unit) |
| PR-01 | 5 command-format transforms (frontmatter nested-path, plain strip, prompt.md, prompt-file `{{{ input }}}`+invokable, gemini.toml) + `parse` frontmatter/body + `destination_path` relpaths — `src/prompts/{transform,parse}.rs::tests` |

**Remaining `[~]` (36) were NOT independently verified this pass** — porter/seed claims still unproven.
The same harness extends to them (port each kasetto golden vector → assert via agent-env public API).
Highest-value next: XC-01..04 (error/http/dirs/clock), M-09..M-14/M-17 (21-preset skill+MCP path table),
CFG-01..03 (recursive `extends` loader), L-01..06 (SHA-256 asset lock), S-09..13 (host auth).

---

## CROSS-CUTTING (port first — everything depends on these)

- [~] XC-01 · src/error.rs:err/Error/Result · string-message error channel: `err(impl Into<String>)` → `Box<dyn Error+Send+Sync>` via `io::Error::other`; `Result<T>` alias. Every absorbed fn returns this. · -> agent-env::AgentEnvError (thiserror: Message/Io/Yaml) + Result + err() · deps: none
- [~] XC-02 · src/fsops/http.rs:http_client · process-wide `OnceLock<Client>`; connect-timeout 10s, total 30s, UA `kasetto/{VERSION}`; pure-Rust rustls+ring (NO C TLS). ERROR: build failure cached & re-returned as Message. · -> agent-env::source::http_client · deps: XC-01
- [x] XC-03 · src/fsops/dirs.rs:dirs_home/dirs_xdg_{config,data,cache}_home/dirs_kasetto_{config,data,cache} · XDG resolution: HOME (ERR "HOME is not set" if unset); XDG_*_HOME honored only when non-empty else `$HOME/.config|.local/share|.cache`; kasetto_* append `kasetto`. OS quirk: env-driven, no platform branch. · -> agent-env::dirs::* (envctl-namespaced dir) · deps: XC-01
- [x] XC-04 · src/fsops/mod.rs:now_unix/now_unix_str · SystemTime since UNIX_EPOCH as secs; `.unwrap_or(0)` on clock-before-epoch. · -> agent-env::util::now_unix{,_str} · deps: none

---

## src/model/* (config schema, agent presets, lock value types)

- [x] M-01 · src/model/config.rs:Scope · enum Global(default)/Project; serde rename `global`/`project`; `#[default]`=Global. · -> agent-env::config::Scope · deps: XC-01
- [x] M-02 · src/model/config.rs:Config · 6-key schema: destination:Option<String>, scope:Option<Scope>, agent:Option<AgentField>, skills:Vec<SourceSpec>(default), mcps:Vec<McpSourceSpec>(default), commands:Vec<CommandSourceSpec>(default). + `extends` stripped pre-parse (see X-*). EDGE: every list `#[serde(default)]` so absent = empty. · -> agent-env::config::Config · deps: M-01,M-03,M-04,M-08
- [x] M-03 · src/model/config.rs:Config::agents/resolved_scope · agents(): One→vec![a], Many→clone, None→[]; resolved_scope(): scope.unwrap_or_default()=Global. · -> agent-env::config::Config::{agents,resolved_scope} · deps: M-02
- [x] M-04 · src/model/config.rs:SourceSpec · skill source: source:String, branch/git_ref(rename `ref`)/sub_dir(rename `sub-dir` alias `sub_dir`):Option, skills:SkillsField. · -> agent-env::config::SourceSpec · deps: M-08
- [x] M-05 · src/model/config.rs:GitPin + SourceSpec::git_pin · ref>branch>Default precedence; Default = try main then master. · -> agent-env::config::{GitPin,SourceSpec::git_pin} · deps: M-04
- [x] M-06 · src/model/config.rs:SourceSpec::expected_revision · local source (no `://`)→"local"; else `ref:{r}` / `branch:{b}` / Default→"branch:main". Drives needs_fetch retarget detection. · -> agent-env::config::SourceSpec::expected_revision · deps: M-05
- [x] M-07 · src/model/config.rs:McpSourceSpec/CommandSourceSpec + as_source_spec · MCP source (no sub_dir, mcps:McpsField) & command source (sub_dir, commands:CommandsField); `as_source_spec()` projects to SourceSpec(skills=Wildcard "*"); MCP forces sub_dir=None. · -> agent-env::config::{McpSourceSpec,CommandSourceSpec} · deps: M-04,M-08
- [x] M-08 · src/model/config.rs:SkillsField/SkillTarget/McpsField/McpEntry/CommandsField/CommandEntry · untagged enums: Wildcard(String) | List(Vec<{Name(String)|Obj{name,path:Option}}>). The selector shape shared across all 3 kinds. · -> agent-env::config::{SkillsField,SkillTarget,McpsField,McpEntry,CommandsField,CommandEntry} · deps: none
- [x] M-09 · src/model/agent.rs:Agent (21-preset enum) + AGENT_PRESETS · 21 serde-renamed variants (amp,antigravity,augment,claude-code,cline,codex,continue,cursor,gemini-cli,github-copilot,goose,junie,kiro-cli,openclaw,opencode,openhands,replit,roo,trae,warp,windsurf) + AGENT_PRESETS slice. SEED = enum shape only. · -> agent-env::config::{Agent,AGENT_PRESETS} · deps: none
- [x] M-10 · src/model/agent.rs:AgentField · untagged One(Agent)|Many(Vec<Agent>). · -> agent-env::config::AgentField · deps: M-09
- [x] M-11 · src/model/agent.rs:Agent::global_path · per-preset global SKILLS dir for all 21 (e.g. claude-code→`.claude/skills`, codex→`.codex/skills`, windsurf→`.codeium/windsurf/skills`, amp|replit→`.config/agents/skills`, cline|warp→`.agents/skills`). SEED-DEFERRED (TASK-0013). · -> agent-env::config::Agent::global_path · deps: M-09
- [x] M-12 · src/model/agent.rs:Agent::project_path · per-preset PROJECT skills dir for all 21 (diverges from global: amp|replit→`.agents/skills`, goose→`.goose/skills`, opencode→`.opencode/skills`, windsurf→`.windsurf/skills`). SEED-DEFERRED. · -> agent-env::config::Agent::project_path · deps: M-09
- [x] M-13 · src/model/agent.rs:Agent::mcp_settings_target (global) · per-preset native MCP config path + McpSettingsFormat for all 21 (claude-code→`.claude.json` McpServers; github-copilot→VS Code user mcp.json VsCodeServers w/ OS branch; codex→`.codex/config.toml` CodexToml; opencode→`.config/opencode/opencode.json` OpenCode; continue→`.continue/mcpServers/kasetto.json`; rest McpServers). OS quirk: vscode_user_mcp_json branches macOS/Windows(APPDATA)/Linux. SEED-DEFERRED. · -> agent-env::config::Agent::mcp_settings_target · deps: M-09,M-26
- [x] M-14 · src/model/agent.rs:Agent::mcp_project_target · per-preset PROJECT MCP path+format for all 21 (claude-code→`.mcp.json`; github-copilot→`.vscode/mcp.json`; many fall through to `.mcp.json` McpServers). SEED-DEFERRED. · -> agent-env::config::Agent::mcp_project_target · deps: M-09,M-26
- [x] M-15 · src/model/agent.rs:Agent::commands_global_path · per-preset global commands dir + CommandFormat, `Option` (None = unsupported). Supported: claude-code/windsurf/opencode/continue/amp/augment/roo/codex(MarkdownFrontmatter or PromptFile)/gemini-cli(GeminiToml); 12 presets→None. SEED-DEFERRED. · -> agent-env::config::Agent::commands_global_path · deps: M-09,M-27
- [x] M-16 · src/model/agent.rs:Agent::commands_project_path · per-preset project commands dir + CommandFormat, `Option`. Supported set differs from global (cursor→`.cursor/commands` MarkdownPlain; cline→`.clinerules/workflows` MarkdownPlain; github-copilot→`.github/prompts` PromptMd; openhands→`.openhands/microagents`); 8→None. SEED-DEFERRED. · -> agent-env::config::Agent::commands_project_path · deps: M-09,M-27
- [x] M-17 · src/model/agent.rs:all_mcp_settings_targets/all_mcp_project_targets · map AGENT_PRESETS → mcp targets, dedup by path (HashSet), sort by path. For `clean` manifest wipe. SEED-DEFERRED. · -> agent-env::config::{all_mcp_settings_targets,all_mcp_project_targets} · deps: M-13,M-14
- [x] M-18 · src/model/agent.rs:all_command_global_targets/all_command_project_targets · map AGENT_PRESETS → command targets via dedup_command_targets (flatten Option, dedup by path, sort). SEED-DEFERRED. · -> agent-env::config::{all_command_global_targets,all_command_project_targets} · deps: M-15,M-16
- [x] M-19 · src/model/agent.rs:command_global_targets/command_project_targets · same dedup over a SPECIFIC agent set (for doctor scoping). SEED-DEFERRED. · -> agent-env::config::{command_global_targets,command_project_targets} · deps: M-15,M-16
- [x] M-20 · src/model/agent.rs:dedup_targets/dedup_command_targets/cmd/vscode_user_mcp_json/mcp_servers_target · private helpers: HashSet path-dedup + sort; `cmd()` builds CommandTarget; `vscode_user_mcp_json` OS-branch; `mcp_servers_target` McpServers ctor. SEED-DEFERRED. · -> agent-env::config (private helpers) · deps: M-09,M-26,M-27
- [x] M-21 · src/model/config.rs:resolve_scope · CLI override > cfg.resolved_scope() > (file-read fallback) > Global. SEED ports CLI>cfg>Global; **file-read fallback DEFERRED** → see M-22. · -> agent-env::config::resolve_scope · deps: M-03
- [ ] M-22 · src/model/config.rs:resolve_scope (file-read fallback branch) · when no Config passed, read `load_config_any(default_config_path())` and use its scope. SEED-DEFERRED (a sync-command concern, TASK-0013). MUST be added as a distinct path; not a regression. · -> agent-env::config::resolve_scope (fallback arm) · deps: M-21,X-04,CFG-01
- [x] M-23 · src/model/types.rs:SkillEntry · lock skill row: destination(scope-relative, legacy-absolute honored)/hash/skill/description(default)/source/source_revision/scope(Option, skip_if_none). SEED folds these fields into AgentLockEntry. · -> agent-env::lock::AgentLockEntry (folded) · deps: M-01
- [x] M-24 · src/model/types.rs:State + LOCK_VERSION · State{version,skills:BTreeMap}; LOCK_VERSION=2 (portable format). SEED: crate-local LOCK_VERSION=2 (AgentLockFile). · -> agent-env::lock (version/state) · deps: M-23 ⟨parity 2026-06-14: schema+LOCK_VERSION proven; kasetto `State` type intentionally folded into runtime/driver (no behavior lost)⟩
- [x] M-25 · src/model/types.rs:Summary/Action/Report/InstalledSkill/SyncFailure · sync result value types: Summary{installed,updated,removed,unchanged,broken,failed}; Action{source,skill,status,error}; Report{run_id,config,destination,dry_run,summary,actions}; InstalledSkill (list view); SyncFailure{name,source,reason}. NOT in seed (sync-result surface). · -> agent-env::report::{Summary,Action,Report,InstalledSkill,SyncFailure} · deps: M-01
- [x] M-26 · src/model/mod.rs:McpSettingsFormat + McpSettingsTarget · enum McpServers/VsCodeServers/OpenCode/CodexToml (the 4 MCP-merge formats); McpSettingsTarget{path,format}. NOT in seed. · -> agent-env::config::{McpSettingsFormat,McpSettingsTarget} · deps: none
- [x] M-27 · src/model/mod.rs:CommandFormat + CommandTarget · enum MarkdownFrontmatter/MarkdownPlain/PromptMd/PromptFile/GeminiToml (the 5 command-format transforms); CommandTarget{path,format}. NOT in seed. · -> agent-env::config::{CommandFormat,CommandTarget} · deps: none

---

## src/fsops/* (hash, config loader, copy, settings, edit, target selection)

- [x] F-01 · src/fsops/hash.rs:hash_dir · recursive collect_files + sort + SHA-256; rel path bytes `\`→`/` normalized (OS-invariant); per-file: rel + NUL + content + NUL. SEED: ported verbatim. · -> agent-env::hash::hash_dir · deps: XC-01
- [x] F-02 · src/fsops/hash.rs:hash_str/hash_file · SHA-256 of a string / single file (8192 buf reader). SEED: ported. · -> agent-env::hash::{hash_str,hash_file} · deps: XC-01
- [x] X-01 · src/model/extend.rs:extract_extends · strip `extends` from mapping; String→[s], Sequence→filter String items, else []. Non-mapping→[]. SEED. · -> agent-env::extend::extract_extends · deps: M-08
- [x] X-02 · src/model/extend.rs:merge_yaml · overlay-on-base; scalars replace; skills/mcps/commands lists merge by identity; non-mapping side returns other. SEED. · -> agent-env::extend::merge_yaml · deps: X-03
- [x] X-03 · src/model/extend.rs:merge_source_list/identity_of/string_field · identity = (source, ref|branch|"", sub-dir|sub_dir|""); same-identity replaced, new appended. SEED. · -> agent-env::extend (merge internals) · deps: M-08
- [x] CFG-01 · src/fsops/config.rs:load_config_any · top-level loader: recursive merge → deserialize Config → cfg_dir (origin base_dir or cwd) + label. ERROR: parse failure labelled. SEED. · -> agent-env::extend::load_config_any · deps: X-01,X-02,CFG-02
- [x] CFG-02 · src/fsops/config.rs:load_config_recursive + ConfigOrigin · MAX_EXTENDS_DEPTH=8 depth guard (ERR "extends depth limit exceeded"); per-origin canonical_id cycle guard (ERR "circular extends detected"); parents merged in order, then self. visited cloned per branch + removed after. SEED. · -> agent-env::extend::load_config_recursive · deps: CFG-03
- [~] CFG-03 · src/fsops/config.rs:fetch_config_text · http(s): rewrite_browse_to_raw → auth → fetch; ERR on non-2xx (with auth hint) + HTML-login-page detection. local: canonicalize (ERR "config not found"), read, parent as base_dir. SEED. · -> agent-env::extend::fetch_config_text · deps: XC-02,S-08,S-12,S-13 ⟨parity 2026-06-14: LOCAL arm proven; remote http(s) arm network-only — close via Engine::agent_sync integration⟩
- [x] F-03 · src/fsops/copy.rs:copy_dir/copy_dir_contents/copy_file · verbatim recursive copy; MAX_COPY_DEPTH=32 (ERR symlink-cycle); dst removed first; SYMLINK followed (canonicalize → recurse dir / copy file); fs::copy preserves +x bit; Windows: strip READONLY (cfg(windows)). ERROR: depth-exceed. NOT in seed. · -> agent-env::fsops::copy_dir · deps: XC-01 ⟨parity 2026-06-14: unix copy/symlink/+x arms PASS on Linux; cfg(windows) READONLY-strip arm platform-residue (unreachable here)⟩
- [x] F-04 · src/fsops/settings.rs:SettingsFile::load/save · JSON wrapper: load existing or `{}` (ERR "invalid settings JSON"); save pretty-printed, create parent dirs. NOT in seed. · -> agent-env::fsops::SettingsFile · deps: XC-01
- [x] F-05 · src/fsops/mod.rs:resolve_path · expand ONLY leading `~/` or bare `~` to HOME (mid-path `~` literal); absolute kept, else base.join. EDGE: home-resolve failure → raw. NOT in seed. · -> agent-env::fsops::resolve_path · deps: XC-03
- [x] F-06 · src/fsops/mod.rs:select_targets + BrokenSkill + TargetSelection · Wildcard "*"→all available, sorted by name (stable); List: Name→lookup-or-broken; Obj{path}→base(abs|source_root.join).join(name) checked for SKILL.md, else broken; Obj{no path}→lookup. ERR "invalid skills field" on non-* wildcard. NOT in seed. · -> agent-env::fsops::select_targets · deps: M-08
- [x] F-07 · src/fsops/mod.rs:resolve_destinations · explicit destination → [resolve_path]; else per-agent global_path/project_path by scope; ERR "must define either destination or a supported agent preset" when no agents. NOT in seed. · -> agent-env::fsops::resolve_destinations · deps: F-05,M-11,M-12
- [x] F-08 · src/fsops/mod.rs:resolve_mcp_settings_targets · per-agent mcp target by scope, dedup by path; empty agents → []. NOT in seed. · -> agent-env::fsops::resolve_mcp_settings_targets · deps: M-13,M-14
- [x] F-09 · src/fsops/mod.rs:resolve_command_targets · per-agent command target by scope (filter unsupported Option=None), dedup by path; empty agents → []. NOT in seed. · -> agent-env::fsops::resolve_command_targets · deps: M-15,M-16
- [x] F-10 · src/fsops/mod.rs:scope_root/relativize_dest/resolve_dest · scope_root: Project→project_root, Global→home; relativize_dest: strip_prefix(root) else absolute kept; resolve_dest: inverse (absolute kept, else root.join). Lock portability core. NOT in seed (engine has own; agent-asset variant needed). · -> agent-env::fsops::{scope_root,relativize_dest,resolve_dest} · deps: XC-03
- [x] FE-01 · src/fsops/config_edit.rs:Section + Pin + Selector + SourceItem + RemoveOutcome · edit value types: Section{Skills,Mcps,Commands} w/ key()/singular(); Pin{Ref,Branch,None} w/ value(); Selector{Wildcard,Names}; SourceItem{source,pin,sub_dir,selector}; RemoveOutcome{NotFound,WholeItem,Names}. NOT in seed. · -> agent-env::config_edit::{Section,Pin,Selector,SourceItem,RemoveOutcome} · deps: XC-01
- [x] FE-02 · src/fsops/config_edit.rs:insert_item · comment-preserving append under section.key(); creates section if absent; normalizes inline `key: []`/`{}`; ERR if inline non-empty list. Indent inherited from first item (default 2); inserts before trailing blanks/comments. EDGE: empty file. NOT in seed. · -> agent-env::config_edit::insert_item · deps: FE-01,FE-06
- [x] FE-03 · src/fsops/config_edit.rs:remove_item + find_match · drop whole item matching (source[,pin][,sub_dir]); `Ok(false)` no-match; ERR "disambiguate" when ambiguous (same source+filters, multiple). sub_dir==Some("") matches entries with no sub-dir. NOT in seed. · -> agent-env::config_edit::remove_item · deps: FE-01,FE-06
- [x] FE-04 · src/fsops/config_edit.rs:remove_names · subtract names from selector list; last name → WholeItem; ERR on wildcard ("remove the whole entry"), object-form entries ("edit directly"), or any missing name ("not found"). NotFound when source absent. NOT in seed. · -> agent-env::config_edit::remove_names · deps: FE-01,FE-06
- [x] FE-05 · src/fsops/config_edit.rs:item_exists + render_item · item_exists: exact (source,pin|"",sub_dir|"") identity match; render_item: emit `- source:`/ref|branch/sub-dir/selector(wildcard `"*"` or named list) at indent. NOT in seed. · -> agent-env::config_edit::{item_exists,render_item} · deps: FE-01
- [x] FE-06 · src/fsops/config_edit.rs:parse_items/extract_fields/field_kv/is_top_level_key/find_top_level/next_top_level/section_inline_value/indent_of/is_dash/split_lines/join_lines/splice · raw-line YAML editing primitives: top-level key detection (col-0, `key:`), section item parsing at shallowest dash indent, field extraction (source/ref|branch/sub-dir|sub_dir), quote-trimming kv, newline-preserving join. NOT in seed. · -> agent-env::config_edit (line primitives) · deps: none

---

## src/source/* (multi-host resolver, URL parse/rewrite, archive, auth, discovery)

- [x] S-01 · src/source/hosts.rs:extract_host/is_gitlab_host/is_bitbucket_host/is_gitea_style_host · host classification: strip scheme→host; gitlab = gitlab.com|.gitlab.com|gitlab.*; bitbucket = bitbucket.org|www; gitea-style = codeberg/gitea/forgejo (+www). SEED. · -> agent-env::source::hosts · deps: none
- [x] S-02 · src/source/parse.rs:RepoUrl + parse_repo_url · 4 variants GitHub/GitLab/Bitbucket/Gitea; trim trailing `/`+`.git`; ERR "unsupported URL scheme" (non-http), "unsupported repository URL". GitLab subgroups (≥3 seg), GHE 2-seg (github.com requires exactly 2). SEED. · -> agent-env::source::{RepoUrl,parse_repo_url} · deps: S-01,XC-01
- [x] S-03 · src/source/parse.rs:BrowseDerived + derive_browse_url · decompose blob/tree browse URL → source+branch|git_ref+sub_dir+skill_name; GitLab `/-/` separator dropped; 40-hex ref→git_ref(pinned) else branch; `.../SKILL.md`→parent=sub_dir,name=skill; tree→path=sub_dir. None for plain/local. EDGE: marker<3 or no rest. SEED. · -> agent-env::source::{BrowseDerived,derive_browse_url} · deps: S-01
- [x] S-04 · src/source/remote.rs:remote_repo_archive_branch · GitHub: token→api.github.com tarball (ref %2F-encoded), no-token→web `archive/refs/heads/{branch}.tar.gz`; others delegate to archive_ref. SEED (as archive_url precedence builder; note materialize main→master retry deferred, see S-15). · -> agent-env::source::archive_url (branch arm) · deps: S-02,S-09
- [x] S-05 · src/source/remote.rs:remote_repo_archive_ref · GitHub token→api tarball / no-token→`archive/{ref}.tar.gz`; GitLab→`api/v4/projects/{enc}/repository/archive.tar.gz?sha={ref}`; Bitbucket→`get/{ref}.tar.gz`; Gitea→`{host}/{owner}/{repo}/archive/{ref}.tar.gz`. SEED. · -> agent-env::source::archive_url (ref arm) · deps: S-02,S-09
- [x] S-06 · src/source/remote.rs:encode_gitlab_path/encode_github_ref · `/`→`%2F` (GitLab project path & GitHub ref single-segment). SEED. · -> agent-env::source (url encoders) · deps: none
- [~] S-07 · src/source/remote.rs:download_extract · dst-clean → create → GET(auth) → ERR on unreachable/HTTP-non-2xx(+auth hint)/HTML-instead-of-tar.gz; gzip(flate2)→tar; **tar-slip guard**: strip top dir, ERR "unsafe archive path" on ParentDir; create parent dirs, unpack. SEED (pure-Rust flate2+tar, no C zlib). · -> agent-env::source::download_extract · deps: XC-02,S-08
- [x] S-08 · src/source/remote.rs:rewrite_browse_to_raw_url + rewrite_{github_blob,gitea_src,gitlab_raw_url} · github blob|raw→raw.githubusercontent.com; gitea src/{branch|commit|tag}→raw (+query); gitlab `/-/raw|blob/`→api/v4 files raw (or `.`-segment heuristic, default ref main). None for unrecognized/non-http. SEED. · -> agent-env::source::rewrite_browse_to_raw_url · deps: S-01
- [x] S-09 · src/source/auth.rs:UrlRequestAuth + apply + for_{github,gitlab,bitbucket,gitea}_archive · headers+optional basic; apply: basic_auth then headers. GitHub Bearer; GitLab PRIVATE-TOKEN|JOB-TOKEN; Bitbucket basic; Gitea `token`. SEED. · -> agent-env::source::UrlRequestAuth · deps: S-10
- [x] S-10 · src/source/auth.rs:{github,gitlab,gitea}_auth_headers/bitbucket_basic_credentials/first_env_var · ENV-ONLY creds (never config/lock): GITHUB_TOKEN|GH_TOKEN→Bearer; GITLAB_TOKEN→PRIVATE-TOKEN else CI_JOB_TOKEN→JOB-TOKEN; BITBUCKET_EMAIL+TOKEN or USERNAME+APP_PASSWORD; GITEA|CODEBERG|FORGEJO_TOKEN→`token`. SEED. · -> agent-env::source (env cred readers) · deps: none
- [x] S-11 · src/source/auth.rs:auth_for_request_url · classify host → headers/basic for fetching a remote resource (config/archive). SEED. · -> agent-env::source::auth_for_request_url · deps: S-01,S-10
- [~] S-12 · src/source/auth.rs:auth_env_inline_help · per-host-family env-var hint string (GitHub/GitLab/Bitbucket/Gitea/none). SEED. · -> agent-env::source::auth_env_inline_help · deps: S-01
- [~] S-13 · src/source/auth.rs:http_fetch_auth_hint · 401|403→" - {help}"; 404→" - if private, {help}"; else "". SEED. · -> agent-env::source::http_fetch_auth_hint · deps: S-12
- [x] S-14 · src/source/mod.rs:materialize_source + MaterializedSource · http: parse → git_pin → fetch+extract (ref/branch/Default), source_revision label; **main→master retry on Default** (second download_extract, ERR appends "also tried master"); resolve_source_root(sub_dir); discover_with_root_name(hint). local: resolve_path, no cleanup_dir, rev "local". SEED partial — S-15 covers main→master retry remainder. NOT otherwise in seed. · -> agent-env::source::materialize_source · deps: S-02,S-05,S-07,S-16,S-17,F-05
- [ ] S-15 · src/source/mod.rs:materialize_source (GitPin::Default main→master retry) · live second HTTP attempt to `master` when `main` 404s; SEED `archive_url(GitPin::Default)` returns the `main` URL only — the RETRY is the materializer's job. SEED-DEFERRED remainder (TASK-0013). · -> agent-env::source::materialize_source (retry arm) · deps: S-14
- [x] S-16 · src/source/mod.rs:resolve_source_root + repo_name_hint · sub_dir: empty→ERR; absolute→ERR "must be relative"; ParentDir|RootDir→ERR "must not escape"; not-exists→ERR; not-dir→ERR. repo_name_hint: last path segment per host variant. NOT in seed. · -> agent-env::source::{resolve_source_root,repo_name_hint} · deps: S-02
- [x] S-17 · src/source/mod.rs:discover/discover_with_root_name/discover_skills_in_subdir · root SKILL.md→named by hint; scan `<root>/` + `<root>/skills/` for `*/SKILL.md`; WARN on subdir shadowing root skill (eprintln). NOT in seed. · -> agent-env::source::discover · deps: none
- [x] S-18 · src/source/mod.rs:discover_mcps · root `.mcp.json`/`mcp.json` + `mcps/*.json`; WARN if legacy `mcp/` present w/o `mcps/` (eprintln). NOT in seed. · -> agent-env::source::discover_mcps · deps: none
- [x] S-19 · src/source/mod.rs:resolve_mcp_entry · Name→`mcps/{name}.json`; Obj{path}→`{path}/{name}.json` (default mcps/); auto-append `.json`; ERR "MCP entry not found". NOT in seed. · -> agent-env::source::resolve_mcp_entry · deps: M-08
- [x] S-20 · src/source/mod.rs:discover_commands/walk_commands · walk `<root>/commands/**/*.md`; nested dirs → `:`-namespaced names (git/commit.md→`git:commit`); skip non-md. NOT in seed. · -> agent-env::source::discover_commands · deps: none
- [x] S-21 · src/source/mod.rs:resolve_command_entry/resolve_named_command · Name→namespaced lookup (ERR "not found"); Obj{path}→`{path}/{name}.md` (ERR "not found"), derived name strips `.md`; Obj{no path}→namespaced lookup. NOT in seed. · -> agent-env::source::resolve_command_entry · deps: S-20

---

## src/lock.rs (SHA-256 agent-asset lock — separate keyed section)

- [x] L-01 · src/lock.rs:AssetEntry · tracked non-skill asset: kind/name/hash/source/destination(CSV: command paths or MCP server names)/source_revision(default, skip-if-empty). SEED. · -> agent-env::lock::AssetEntry · deps: M-01
- [x] L-02 · src/lock.rs:LockFile + Default + default_version · {version(default 2),skills:BTreeMap<SkillEntry>,assets:BTreeMap<AssetEntry>}; unknown fields ignored (legacy-tolerant). SEED: AgentLockFile (decoupled, LOCK_VERSION=2, AGENT_ASSETS_KEY). · -> agent-env::lock::AgentLockFile · deps: L-01,M-23
- [x] L-03 · src/lock.rs:LockFile::{state,apply_state,get/save/remove/list_tracked_asset,clear_all,list_installed_commands,list_installed_mcps} · asset CRUD by id; list_installed_mcps splits dest CSV, sort+dedup; list_installed_commands sort+dedup names; filter by kind. SEED (asset helpers). · -> agent-env::lock::AgentLockFile (methods) · deps: L-02 ⟨parity 2026-06-14: get/save/remove/list_tracked/clear_all proven; list_installed_{commands,mcps} re-derived in driver by design⟩
- [x] L-04 · src/lock.rs:lock_path · Project→project_root/`kasetto.lock`; Global→kasetto_data/`kasetto.lock`. SEED (envctl.lock embedding is TASK-0017). · -> agent-env::lock::lock_path · deps: XC-03,M-01
- [x] L-05 · src/lock.rs:load_lock/save_lock · load: missing/empty→default; parse YAML (ERR labelled); save: stamp LOCK_VERSION (legacy v1→2 restamp), create parents, YAML write. EDGE: legacy-v1 absolute-dest honored, unknown fields dropped. SEED. · -> agent-env::lock::{load,save} · deps: L-02
- [x] L-06 · (seed) commands/lock.rs:diff → lock_check + LockMode · 3 modes plain/Update(Vec)/Locked; `allows_fetch()`/`should_resolve()`; lock_check→Vec<LockDrift> (added/removed/updated by hash|rev). SEED captures mode semantics + drift as pure type. Command-level drift (skills+assets, upgrade-package filter) = C-04 remainder. · -> agent-env::lock::{LockMode,lock_check,LockDrift} · deps: L-02

---

## src/state.rs (machine-local runtime — kept OUT of committed lock)

- [x] ST-01 · src/state.rs:RuntimeState · {last_run:Option, latest_report:Option, installed_at:BTreeMap} — machine-local, regenerated each sync, never committed (lock↔runtime separation, ADR-0001 §4). updated_at/set_updated_at/forget/save_report_json/load_latest_failures (extract broken|source_error actions). NOT in seed (envctl runtime.rs is the analog — must preserve separation). · -> agent-env::runtime::RuntimeState (or reuse engine runtime) · deps: M-25
- [x] ST-02 · src/state.rs:runtime_state_path/load/save/clear_runtime_state · path keyed by hash_str(lock_path) under cache/`runtime/{key}.json`; load missing/empty→default; save pretty JSON + parent dirs; clear removes file. NOT in seed. · -> agent-env::runtime::* · deps: F-02,L-04,XC-03

---

## src/profile.rs (SKILL.md metadata extraction)

- [x] P-01 · src/profile.rs:read_skill_profile/read_skill_profile_from_dir · parse SKILL.md → (title, description): frontmatter name/description; body first `#` heading→title, first non-heading line→description (strip `-`/`*`); fallbacks (name, "No description."). EDGE: missing file → fallback. NOT in seed. · -> agent-env::profile::read_skill_profile · deps: none
- [x] P-02 · src/profile.rs:format_updated_ago · parse unix ts → "Ns/m/h/d ago" (or "in Ns" future, "unknown" on parse-fail). NOT in seed. · -> agent-env::profile::format_updated_ago · deps: XC-04

---

## src/lib.rs (config-path resolution)

- [x] CP-01 · src/lib.rs:default_config_path/resolve_config_path + Preferences + DEFAULT_*_FILENAME · priority: $KASETTO_CONFIG → ./kasetto.yaml (local) → prefs `source:` (XDG config/config.yaml) → global kasetto.yaml → ./kasetto.yaml fallback. (envctl renames KASETTO_CONFIG / filenames as appropriate.) NOT in seed. · -> agent-env::config_path::default_config_path · deps: XC-03

---

## src/commands/* (command BUSINESS LOGIC — the WHAT; clap glue is FRONT-END)

> Verb mapping (11→6) recorded here so nothing is orphaned. The clap parsing in cli.rs/app.rs is FRONT-END (FRONTEND-* rows); the orchestration logic below is ABSORB.

- [x] C-01 · src/commands/sync/mod.rs:run + SyncOptions + SyncContext + SyncMut · `sync` orchestrator: load_config → resolve_scope → resolve_destinations → load lock+state+runtime → sync_skills/commands/mcps → save lock+runtime+report. ERR: `--locked`+`--update` contradiction. EDGE: failed>0 → exit(1). dry_run skips writes. · -> Engine::agent_sync (kasetto sync → `agent sync`) · deps: CFG-01,M-21,F-07,L-05,ST-02,C-02,C-03,C-05,M-25
- [x] C-02 · src/commands/sync/skills.rs:sync_skills (+ needs_fetch, dest_status, HashCache, process_single_skill, process_locked_skill, desired_skill_names, ensure_locked_satisfiable, remove_stale_skills) · per-source: locked-satisfiable check → needs_fetch (retarget rev / dest-hash mismatch / wildcard-bootstrap) → fetch(materialize+select+copy) or lock-skip(local repair from good dest); unchanged when hash+all-dests match; **never prune on failure** (failed>0 skips remove_stale). EDGE: multi-dest repair, tampered-dest repair. · -> Engine agent-sync (skills phase) · deps: S-14,F-06,F-03,F-01,P-01,C-06
- [x] C-03 · src/commands/sync/commands.rs:sync_commands (+ desired_command_names, needs_fetch_commands, ensure_locked_satisfiable_commands, apply_pending, remove_stale) · per-command-source: locked check → needs_fetch (no local repair — installed file is a transform) → materialize+discover/resolve → hash source → unchanged when hash+all-dest-files exist → apply_command per target, record dest CSV; empty `commands:` → remove_stale(empty). never-prune-on-failure. · -> Engine agent-sync (commands phase) · deps: S-14,S-20,S-21,PR-01,F-02,C-06
- [x] C-04 · src/commands/sync/mcps.rs:sync_mcps (+ desired_mcp_file_names, needs_fetch_mcps, ensure_locked_satisfiable_mcps, apply_pending, remove_stale) · per-mcp-source: locked check → needs_fetch (servers-present-in-every-target / retarget) → materialize+discover/resolve → read server_names from mcpServers → unchanged when hash+servers-present → merge_mcp_config per target, record server CSV; no-agents+orphans → scrub from ALL known agents (fallback targets). **additive/never-clobber** (the broker/repowire/weave invariant). never-prune-on-failure. · -> Engine agent-sync (mcps phase) · deps: S-14,S-18,S-19,MC-01,F-02,C-06,M-17
- [x] C-05 · src/commands/sync/mod.rs:remove_stale (shared) + update_active_for_source + sync_label_with + file_name_str · shared orphan-cleanup: skip if in desired_ids, bump removed, push removed|would_remove action, run teardown closure (not on dry_run); update_active: `--update` no-names=all else name-match. · -> agent-env::sync::remove_stale (shared) · deps: M-25
- [x] C-06 · src/commands/sync/skills.rs:skill_key + lock-key convention · `<source>::<name>` skill key; `command::{src}::{name}` / `mcp::{src}::{file}` asset ids — single source of truth so writer/lookup can't drift. · -> agent-env::sync (key conventions) · deps: none
- [ ] C-07 · src/commands/add.rs:run + AddOptions + plan_edits + resolve_pin + verify_source + selector_from + named_skills · `add` (verb 2): ERR --ref+--branch / --locked w/o --no-sync / @ref+--ref conflict / already-exists. split_at_ref → derive_browse_url → resolve_pin (ref>branch>@ref>derived) → plan_edits (kind flags → sections; nothing→skills:"*"; SKILL.md→1-skill list; MCP no sub-dir) → verify_source (fetch once, assert named skills) → insert_item per edit → sync_after. dry_run previews. · -> Engine::agent_add (kasetto add/init → `agent add`) · deps: FE-02,FE-05,S-03,S-14,F-06,C-12,C-01
- [ ] C-08 · src/commands/remove.rs:run + RemoveOptions + remove_whole_source + remove_by_kind · `remove`/`rm` (verb 3): split_at_ref+derive → pin/sub_dir identity → any kind flags? remove_by_kind (per section: `*`→remove_item, else remove_names; ERR not-found) else remove_whole_source (all 3 sections; ERR "not found in any list"; MCP ignores sub-dir). sync_after prunes. dry_run previews. · -> Engine::agent_remove (kasetto remove → `agent remove`, alias rm) · deps: FE-03,FE-04,S-03,C-12,C-01
- [ ] C-09 · src/commands/lock.rs:run + LockOptions + refresh_asset_revisions + diff_summary + Drift/DriftStatus + upgrade_active · `lock` (verb 4): re-resolve skills (materialize+hash, upgrade-package filter carries unchanged), refresh asset revision pins (no content hash), write lock. `--check`: diff prev vs rebuilt (added/removed/updated by hash|rev for skills + rev for assets), exit(1) on drift, never write. `--upgrade-package`: restrict re-resolve to sources providing named skills. · -> Engine::agent_lock (kasetto lock + --check + --upgrade-package → `agent lock`) · deps: S-14,F-06,F-01,L-05,L-06,P-01
- [ ] C-10 · src/commands/list.rs:run + load_skills_mcps_commands + installed_skills_from_lock + {mcp,command}_asset_entries + AssetEntry + scope helpers · `list`/`status` (verbs 7,10): read lock(s) (scope or merged global+project), build InstalledSkill (read_skill_profile, updated_ago) + MCP/command asset rows, sort, filter by ListKind. Drift/status folds into list+lock --check per ADR. · -> Engine::agent_list (kasetto list/status → `agent list`) · deps: L-05,P-01,P-02,ST-02,M-25
- [ ] C-11 · src/commands/clean.rs:run + apply_removals + CleanOutput · `clean` (verb 8): load lock → count skills/mcps/commands → apply_removals (rm skill dirs, rm command files, remove_mcp_server from all known-agent targets) → clear_all + save + clear runtime. dry_run previews; **fail-closed best-effort** (ignores per-item rm errors, never partial-prunes the lock before disk). · -> Engine::agent_clean (kasetto clean → `agent clean`) · deps: L-05,MC-02,M-17,F-10,ST-02
- [ ] C-12 · src/commands/source_edit.rs:resolve_local_config_path + split_at_ref + sync_after · shared add/remove plumbing: ERR on remote config edit; `@<ref>` tail-split (SSH/userinfo round-trip safe); sync_after runs a plain sync post-edit. · -> agent-env::config_edit::{resolve_local_config_path,split_at_ref} + Engine sync_after · deps: CP-01,C-01
- [ ] C-13 · src/commands/init.rs:run + TEMPLATE + init_config_path · `init` (verb 9): write commented YAML template (local or `--global`); ERR/prompt on existing (TTY overwrite). Folds into `agent add` (init path) / `agent sync` bootstrap per ADR. BUSINESS = template + path resolution; the TTY prompt + banner are FRONT-END. · -> Engine::agent_init_template (folded into add/sync) · deps: XC-03,CP-01
- [ ] C-14 · src/commands/uninstall.rs:run + count_assets + UninstallCounts + remove_{dir,file}_if_exists · `self uninstall`: count assets → clean::run → remove config+data dirs + kst + binary. ENVCTL-SCOPED: binary/dir removal is envctl's own install concern; the ASSET-cleanup portion folds into `agent clean`. The binary-self-removal is FRONT-END/divergent. · -> Engine::agent_clean (asset portion only) · deps: C-11,L-05

---

## FRONT-END — envctl owns rendering (recorded as intentionally-diverged, NOT missed)

- [≠] FRONTEND-01 · src/cli.rs:Cli/Commands/SyncArgs/ScopeArgs/OutputArgs/ListKind/ColorMode/SelfAction · front-end: clap subcommand/flag wiring. VERB SEMANTICS captured by C-01..C-14; envctl re-expresses as `envctl agent {sync,add,remove,lock,list,clean}` clap. `--dry-run`/`--json`/`--locked`/`--frozen`(alias)/`--update`/`--upgrade-package`/`--scope` flags are the no-downgrade flag surface — must reach the Engine methods. · -> envctl CLI (TASK-0014) · deps: C-01..C-14
- [≠] FRONTEND-02 · src/app.rs:run + dispatch + should_suppress_notice + current_program_name · front-end: clap dispatch to command::run + update-notice gating. Replaced by envctl's CLI dispatch into Engine. · -> envctl CLI (TASK-0014) · deps: FRONTEND-01
- [≠] FRONTEND-03 · src/main.rs / src/bin/kst.rs · front-end: binary entrypoints. envctl has its own. · -> envctl bins · deps: none
- [≠] FRONTEND-04 · src/ui.rs · front-end: all terminal rendering (spinners, trees, chips, headers, JSON print, color gating, short_source, status_tail, glyphs). envctl renders via its own Event/printer; the non-printing Engine emits Events instead. · -> envctl CLI/GUI rendering (TASK-0014) · deps: none
- [≠] FRONTEND-05 · src/banner.rs · front-end: ASCII banner. envctl owns its banner. · -> envctl CLI · deps: none
- [≠] FRONTEND-06 · src/colors.rs · front-end: ANSI color constants + clap_styles. envctl owns its palette. · -> envctl CLI · deps: none
- [≠] FRONTEND-07 · src/update_notifier.rs · front-end: GitHub release update-check cache/notice. Task brief: envctl has its own; do NOT port. (doctor's update_check rows depend on it → also envctl-owned.) · -> envctl (own update mechanism) · deps: none
- [≠] FRONTEND-08 · src/commands/self_update.rs · front-end: download+replace binary from GitHub releases + is_newer. envctl manages its own installation. · -> envctl (own self-update) · deps: none
- [≠] FRONTEND-09 · src/commands/doctor.rs · front-end: diagnostics RENDERING — but envctl already has `doctor` (ALREADY-PORTED per absorption §4); agent-env does NOT re-implement diagnostics. The command-dir-writable / collect_command_dirs probe logic is the only absorb-adjacent bit and is covered by M-15/M-16/M-19. · -> envctl existing doctor (no re-port) · deps: M-19
- [≠] FRONTEND-10 · src/commands/completions.rs · front-end: clap_complete shell completions. envctl owns its completions. · -> envctl CLI · deps: FRONTEND-01

---

## ALREADY-PORTED in envctl — DO NOT re-port (absorption §4)

- [≠] AP-01 · kasetto component-lock analog · envctl `crates/engine/src/lock.rs` is the **FNV-1a** component lock (LockDriftKind Added/Removed/Changed, `lock --check` CI gate) — UNTOUCHED. agent-env adds a SEPARATE keyed SHA-256 section (L-01..L-06). Rehashing components under SHA-256 = downgrade. · -> (engine lock, untouched) · deps: none
- [≠] AP-02 · kasetto runtime analog · envctl `crates/engine/src/runtime.rs` — preserve lock↔runtime separation (ST-01/ST-02 reuse or mirror it). · -> (engine runtime) · deps: none
- [≠] AP-03 · kasetto doctor · envctl `doctor` already ported — agent-env does not re-implement (see FRONTEND-09). · -> (engine doctor) · deps: none

---

## LEFT-BEHIND (added 2026-06-13 by the rust-port-MERGE verify sweep — were dep-referenced but unrowed)

The verify/merge researcher's left-behind sweep found these kasetto v3.2.0 capabilities referenced
as deps (MC-01/MC-02/PR-01) in C-03/C-04/C-11 but never given a row. agent-env has the format TYPE
shells (M-26/M-27) + target resolution (M-13..M-20) but NOT these merge/transform IMPLEMENTATIONS.
The merge-ledger (`.handoff/loop/rust-port/merge-ledger.md`) is authoritative in verify-merge mode.

- [x] MC-01 · src/mcps/{mod,merge,pack}.rs:merge_mcp_config/merge_into_json_key/merge_mcp_servers_object/merge_vscode_servers_object/merge_opencode_mcp_object/read_source_mcp_servers · the ADDITIVE, never-clobber MCP merge across 3 JSON formats (mcpServers/VsCode/OpenCode) — MUST preserve pre-existing servers (global broker/repowire/weave). #1 no-downgrade risk. · -> agent-env::mcp · deps: M-26,F-04
- [x] MC-02 · src/mcps/{mod,codex}.rs:remove_mcp_server/servers_present_in_settings/merge_codex_config_toml/json_mcp_server_to_codex_toml_table · the CodexToml (4th) MCP format + server removal + presence check (TOML, additive). · -> agent-env::mcp (codex) · deps: MC-01
- [x] PR-01 · src/prompts/{mod,parse,transform}.rs:apply_command/Parsed::parse/render/destination_path · the 5 command-format transforms (MarkdownFrontmatter nested-path, MarkdownPlain, PromptMd, PromptFile {{{input}}}/invokable, GeminiToml) + frontmatter split (CRLF-norm, unclosed `---`→err). · -> agent-env::command · deps: M-27,F-04
