# Implementation log: TASK-0012 — `crates/agent-env` standalone library (kasetto absorption P0, Epic C)

Status: **GREEN** — builds + 61 tests pass + clippy clean + fmt clean + no-c gate PASS.
Ported faithfully from the LIVE `meta/kasetto` **v3.2.0** source (Cargo 3.2.0). No downgrade.

## Changes
- `Cargo.toml` (root): added `"crates/agent-env"` to `[workspace] members`; added `flate2`
  (`default-features=false, features=["rust_backend"]`) and `tar = "0.4"` rows to
  `[workspace.dependencies]` (one-rustls discipline preserved — reuse existing serde*/sha2/
  reqwest/toml/thiserror rows).
- `crates/agent-env/Cargo.toml` (NEW): package `envctl-agent-env`; version/edition/rust-version/
  license `.workspace = true`; `[lints.rust] unsafe_code = "forbid"`; deps serde/serde_json/
  serde_yaml/toml/sha2/thiserror/flate2/tar all `workspace`, reqwest `{ workspace, features=["blocking"] }`
  (adds only the `blocking` client feature on top of the workspace ring-only pin). dev-dep serde_yaml.
- `crates/agent-env/src/lib.rs` (NEW): crate root + `AgentEnvError` (thiserror: `Message`/`Io`/`Yaml`)
  + `Result<T>` alias + `err()` helper (mirrors kasetto's string-message `err(...)` channel) + re-exports.
- `crates/agent-env/src/config.rs` (NEW): the 6-key+extends schema + 21-preset agent enum (shape only).
- `crates/agent-env/src/extend.rs` (NEW): `extends` extract/merge + recursive loader w/ cycle+depth guards.
- `crates/agent-env/src/source.rs` (NEW): multi-host resolver + archive URL builders + tar-slip guard + env-only auth.
- `crates/agent-env/src/hash.rs` (NEW): OS-invariant SHA-256 (`hash_dir`/`hash_file`/`hash_str`).
- `crates/agent-env/src/lock.rs` (NEW): SHA-256 agent-asset lock + 3 modes + `lock_check` drift.
- `crates/agent-env/tests/no_downgrade.rs` (NEW): all-6-keys+extends round-trip (no-downgrade proof).

## Engine API (parity contract — N/A this card)
TASK-0012 is the **standalone library only**. No Engine/CLI/GUI wiring (deferred to TASK-0013/0014).
Public surface callers will drive (re-exported from `lib.rs`):
- config: `Config`, `Scope {Global,Project}`, `SourceSpec`/`McpSourceSpec`/`CommandSourceSpec`,
  `SkillsField`/`McpsField`/`CommandsField`, `SkillTarget`/`McpEntry`/`CommandEntry`, `AgentField`,
  `Agent` (21 presets) + `AGENT_PRESETS`, `GitPin`, `SourceSpec::git_pin()`/`git_pin_of()`,
  `expected_revision()`, `resolve_scope()`.
- extend: `extract_extends()`, `merge_yaml()`, `load_config_recursive()`/`load_config_any()`,
  `MAX_EXTENDS_DEPTH = 8`.
- source: `RepoUrl`, `parse_repo_url()`, `BrowseDerived`+`derive_browse_url()`,
  `rewrite_browse_to_raw_url()`, `archive_url(parsed, &GitPin)` (ref>branch>default precedence),
  `download_extract()` (tar-slip guard), `UrlRequestAuth` (env-only creds), `http_client()`.
- hash: `hash_dir()`/`hash_file()`/`hash_str()`.
- lock: `AgentLockFile`, `AgentLockEntry`, `AssetEntry`, `LockMode {Plain, Update(Vec<String>), Locked}`,
  `lock_check()` → `Vec<LockDrift>`, `load()`/`save()`/`lock_path()`, `LOCK_VERSION = 2`,
  `AGENT_ASSETS_KEY = "agent_assets"`.

## kasetto v3.2.0 → agent-env mapping
| agent-env module | kasetto v3.2.0 source |
|---|---|
| `config.rs` | `src/model/config.rs` (schema, GitPin, git_pin, expected_revision) + `src/model/agent.rs` (21-preset enum + AGENT_PRESETS) |
| `extend.rs` | `src/model/extend.rs` (extract_extends, merge_yaml, identity-keyed merge) + `src/fsops/config.rs` (load_config_recursive, MAX_EXTENDS_DEPTH=8 cycle+depth guards, remote fetch) |
| `source.rs` | `src/source/{hosts,parse,remote,auth}.rs` + `src/fsops/http.rs` (http_client) — parse_repo_url, derive_browse_url, archive URL builders, rewrite_browse_to_raw_url, download_extract tar-slip guard, UrlRequestAuth env-only |
| `hash.rs` | `src/fsops/hash.rs` (verbatim: SHA-256, `\`→`/` normalized) |
| `lock.rs` | `src/lock.rs` (LockFile/AssetEntry/load/save) + `src/commands/lock.rs` (drift diff → `lock_check`, mode logic → `LockMode`) |

Faithful-port notes (no behavior dropped):
- kasetto's boxed `err()` → typed `AgentEnvError::Message(String)`; control flow line-for-line.
- kasetto's `LockFile` is engine-coupled (`SkillEntry`/`State`/`LOCK_VERSION` from `model::types`).
  Decoupled here: `AgentLockEntry` folds in the skill-entry fields; `LOCK_VERSION=2` is crate-local.
  This is the **separate SHA-256 type** mandated by ADR-0001 §4/§10 — `crates/engine/src/lock.rs`
  (FNV-1a) is untouched.
- `archive_url(parsed, &GitPin)` exposes the ref>branch>default precedence as a pure URL builder.
  The `main → master` *retry* fallback (a second HTTP attempt) is the higher-level materializer's
  job (TASK-0013) since it needs a live fetch; `GitPin::Default` returns the `main` URL here. Noted
  in the `archive_url` doc-comment.
- `LockMode` adds `allows_fetch()` (`--locked` = false → zero network) and `should_resolve()`
  (selective `--update`) capturing kasetto's `commands/lock.rs` + sync mode semantics as a pure,
  testable type (no network in this card).

## Tests added (61 total: 60 unit + 1 integration; all pass)
- config (10): wildcard/list/{name,path}, `sub-dir`/`sub_dir` alias, ref>branch>default pin +
  expected_revision, local→`local`, `git_pin_of`, AgentField One|Many, 21-preset count, resolve_scope.
- extend (10): extract string/list/absent; merge scalars-replace/keep-base-keys/append-distinct/
  override-same-identity/distinct-refs+sub-dirs/mcps+commands; loader extends-relative/chains/
  override-in-extends; **cycle guard**; **depth>8 guard**.
- source (24): parse GitHub/GHE/GitLab-subgroup/Bitbucket/Codeberg + non-http reject; derive_browse_url
  blob-SKILL.md/tree/sha-pin/gitlab-dash/plain-none; **archive_url ref>branch>default precedence** +
  GitHub web/api+token + bitbucket/gitea/gitlab-subgroup-encode builders; rewrite github/gitea/gitlab/
  skip; **tar-slip guard** (`repo/../evil.txt` → Err `"unsafe archive path"`) + safe-extract-strips-top-dir.
- hash (4): **separator-invariant** (`a\b`≡`a/b`), stable-across-runs, content-change-differs, file/str.
- lock (8): round-trip skills+assets (**scope-relative destination**), default-when-missing, **legacy
  v1 restamp**, `lock_check` added/removed/updated + hash/rev change, **`--locked` zero-network**
  (`allows_fetch`/`should_resolve` false), `--update` selective resolve, asset helpers.
- integration (1): **all-6-keys+extends round-trip** (`tests/no_downgrade.rs`) — destination, scope,
  agent(Many), 2 skills sources (base `*` + child narrowed-list, distinct identities), mcps, commands
  all preserved through `load_config_recursive` + deserialize.

## Build/test status (commands run; rtk proxy = raw passthrough)
- `rtk proxy cargo build -p envctl-agent-env` → exit=0
- `rtk proxy cargo test -p envctl-agent-env` → 60 lib + 1 integration + 0 doc → **all pass**, exit=0
- `rtk proxy cargo clippy -p envctl-agent-env --all-targets -- -D warnings` → exit=0 (clean)
- `rtk proxy cargo fmt -p envctl-agent-env` then `... -- --check` → exit=0 (clean)
- `bash ci/gates/no-c.sh` → **NO-C GATE PASS**, exit=0
- `rtk proxy cargo build -p envctl-engine -p envctl` → exit=0 (root Cargo.toml change is non-breaking)
- Supply-chain spot checks: `cargo tree -p envctl-agent-env` shows **no** mimalloc/libmimalloc/sqlite/
  openssl/aws-lc/zlib; rustls=0.23.40 on **ring**; flate2→**miniz_oxide** (pure-Rust), no libz.

## Deviations
None in scope. Two **plan-sanctioned defers** (explicitly authorized by the architect plan + task brief):
1. **21-preset per-agent native path methods** (`global_path`/`mcp_settings_target`/`commands_*_path`,
   `all_*_targets`, `CommandFormat`/`McpSettingsFormat`) → DEFERRED to TASK-0013. Only the enum shape
   (21 serde-renamed variants + `AGENT_PRESETS`) is ported here, as the brief specified.
2. **`resolve_scope` config-path file-read fallback** (kasetto reads the default config when no `Config`
   is passed) → DEFERRED to TASK-0013 (a `sync`-command concern). The library form takes the loaded
   `Config` directly: CLI override > cfg > Global. Noted in the `resolve_scope` doc-comment.
3. **`main → master` archive retry** (second live HTTP attempt) → TASK-0013 materializer. `archive_url`
   returns the `main` URL for `GitPin::Default`; the URL-builder precedence (ref>branch>default) is
   fully ported and tested. Noted in the doc-comment.

Out of TASK-0012 scope by design (TASK-0013…0018): Engine module + Events, CLI verbs
`agent {sync,add,remove,lock,list,clean}`, GUI parity, MCP-merge additive/never-clobber + the
broker/repowire/weave regression fixture, 5 command-format transforms, the full sync orchestration
(`materialize_source`/`discover`/install), and the `envctl.lock` embedding under `[agent_assets]`.

## Handoff notes (for the invariant-guardian)
- **No-c is green and pre-armed:** Gate 3.5 auto-arms on `envctl-agent-env` and forbids mimalloc; the
  resolved tree is clean (verified). The package is named exactly `envctl-agent-env` as the gate expects.
- **tar-slip guard:** `source::extract_tar_gz` rejects `Component::ParentDir` fail-closed with
  `"unsafe archive path"`. The unit test hand-crafts a raw ustar header for `repo/../evil.txt` because
  `tar::Builder::append_data` itself refuses `..` — the hand-crafted archive is what a real attacker
  emits, so the test exercises **our** guard, not the builder's. Verify
  `source::tests::extract_rejects_parent_dir_traversal` covers the escape and the companion safe test
  confirms top-dir stripping still writes legitimate entries.
- **`--locked` zero-network:** `LockMode::Locked.allows_fetch() == false` and `should_resolve() == false`
  for every source — there is no fetch path reachable under `Locked`. Verify
  `lock::tests::locked_mode_is_zero_network_and_never_resolves`.
- **Separate SHA-256 lock type:** `crates/engine/src/lock.rs` (FNV-1a component lock) is **untouched** —
  confirm no edits there. `AgentLockFile`/`AGENT_ASSETS_KEY` is a distinct type ready for TASK-0017's
  keyed-section embedding.
- **Env-only credentials:** `UrlRequestAuth` reads creds from env vars only (GITHUB/GH_TOKEN,
  GITLAB/CI_JOB_TOKEN, BITBUCKET_EMAIL+TOKEN / USERNAME+APP_PASSWORD, GITEA/CODEBERG/FORGEJO_TOKEN) —
  never from config or lock. No credential field is serialized anywhere.
- **No-downgrade proof:** `tests/no_downgrade.rs::all_six_keys_plus_extends_round_trip` exercises all 6
  config keys + extends end-to-end; if any key drops, it fails.
- The three plan-sanctioned defers above are TASK-0013 scope — not regressions.

## rust-port cycle: model/* completion

**Rows ported (parity ledger):**
- **M-09 finish** — `Agent::global_path` / `project_path` for all 21 presets (seed had enum shape only).
- **M-11** `Agent::global_path` · **M-12** `Agent::project_path` — exact per-preset SKILLS dirs (incl. divergences: amp|replit global `.config/agents/skills` vs project `.agents/skills`; goose global `.config/goose/skills` vs project `.goose/skills`; opencode global `.config/opencode/skills` vs project `.opencode/skills`; windsurf `.codeium/windsurf/skills` vs `.windsurf/skills`; cline|warp → `.agents/skills`).
- **M-13** `Agent::mcp_settings_target` (global) · **M-14** `Agent::mcp_project_target` — per-preset native MCP path + `McpSettingsFormat`, incl. github-copilot VS Code **OS branch** (`vscode_user_mcp_json`: macOS `~/Library/Application Support/Code/User/mcp.json`, Windows `%APPDATA%/Code/User/mcp.json`, Linux `~/.config/Code/User/mcp.json`), codex→`.codex/config.toml` CodexToml, opencode→OpenCode, continue→`.continue/mcpServers/kasetto.json`, 7-preset project fallthrough → `.mcp.json` McpServers.
- **M-15** `Agent::commands_global_path` (9 supported, 12 None) · **M-16** `Agent::commands_project_path` (13 supported, 8 None) — exact dirs + `CommandFormat` per preset.
- **M-17** `all_mcp_settings_targets` / `all_mcp_project_targets` · **M-18** `all_command_global_targets` / `all_command_project_targets` · **M-19** `command_global_targets` / `command_project_targets` (scoped) — HashSet path-dedup + sort.
- **M-20** private helpers `dedup_targets` / `dedup_command_targets` / `cmd` / `vscode_user_mcp_json` / `mcp_servers_target`.
- **M-25** sync-result types `Summary{installed,updated,removed,unchanged,broken,failed}` / `Action{source,skill,status,error}` / `Report{run_id,config,destination,dry_run,summary,actions}` / `InstalledSkill` / `SyncFailure{name,source,reason}` (types + serde only; sync engine = TASK-0013).
- **M-26** `McpSettingsFormat`(McpServers/VsCodeServers/OpenCode/CodexToml) + `McpSettingsTarget{path,format}`.
- **M-27** `CommandFormat`(MarkdownFrontmatter/MarkdownPlain/PromptMd/PromptFile/GeminiToml) + `CommandTarget{path,format}`.

**Files changed:**
- NEW `crates/agent-env/src/agent.rs` — M-11..M-20, M-26, M-27 (impl Agent path methods + helpers + format/target types).
- NEW `crates/agent-env/src/report.rs` — M-25 sync-result types.
- `crates/agent-env/src/lib.rs` — declare+re-export `agent` and `report` modules; updated the scope-boundary doc (path-mapping is no longer deferred; only the sync *engine* is TASK-0013).

**Test count delta:** lib tests 60 → 78 (**+18**). Ported kasetto's 3 inline `agent.rs` tests verbatim + added all-21-preset exact-mapping coverage (global/project paths, both MCP targets incl. copilot OS branch, both command sets incl. the 12-None / 8-None splits, dedup/sort + scoped-set + empty-set), plus 4 `report.rs` serde tests (field names, scope rename, null-error retained). Workspace total stays green; no other crate touched.

**Gate results (raw via `rtk proxy`):** build=0 · test=78 lib + 1 integ + 0 doc, all pass · clippy -D warnings=0 · fmt --check=0 · no-c=PASS.

**Verbatim-name note:** kasetto's `continue` preset writes its own merge-marker file `.continue/mcpServers/kasetto.json` (both global M-13 and project M-14). This is kasetto's SELF-named drop file inside the agent-native `.continue/mcpServers/` dir, not an agent-native path. Per the naming note it is kept **verbatim** (byte-for-byte parity for the differential verifier); the product-identity rename is TASK-0013 Engine-wiring's job. All agent-native paths (`.claude/skills`, `.codex/config.toml`, the VS Code user `mcp.json`, etc.) are unchanged. The `kasetto_config` arg on `mcp_settings_target`/`all_mcp_settings_targets` is threaded through verbatim (kasetto reserves it; unused by every preset today).

## rust-port-merge cycle: command transforms (PR-01)

**Cluster:** the command-format transform engine (ledger PR-01 — left-behind).
**Source:** kasetto v3.2.0 `src/prompts/{mod,parse,transform}.rs` (82+97+206 L).
**Landed:** NEW module `crates/agent-env/src/command.rs`, declared `pub mod command;`
and re-exported (`apply_command, destination_path, ensure_parent_dirs, parse, render, Parsed`)
from `crates/agent-env/src/lib.rs`.

**Landing decision:** new-module (not merge-into-existing). The `CommandFormat` /
`CommandTarget` value types already lived in `agent.rs` (re-exported from `lib.rs`); the
transforms dispatch on them. No symbol conflict — `apply_command`/`render`/`destination_path`
/`parse`/`Parsed` were absent from the crate, so a clean add.

**Ported VERBATIM (no downgrade), all 5 command-format transforms:**
- `MarkdownFrontmatter` — namespaced `:` → nested subdirs (`git:commit` → `git/commit.md`);
  frontmatter re-emitted between fences.
- `MarkdownPlain` — body only, frontmatter stripped; flat `-` name.
- `PromptMd` — `<name>.prompt.md`, frontmatter preserved.
- `PromptFile` (Continue Dev) — `$ARGUMENTS`→`{{{ input }}}`, `invokable: true` injected,
  no double-inject when already present; flat `-` name.
- `GeminiToml` — `description = "…"` (TOML-escaped) + `prompt = """…"""` heredoc.
Plus `parse` (CRLF-normalize; opening `---` without closing `---` → fail-closed error),
`Parsed::description` (serde_yaml extraction), `derive_relpath`/`name_to_nested_path`/
`flatten_name`/`toml_string`/`render_prompt_file`/`render_gemini_toml`/`ensure_parent_dirs`,
and the `apply_command` driver.

**Error mapping:** kasetto `err(...)`/`Result` → `crate::err`/`crate::Result`
(`AgentEnvError::Message`), mirroring the sibling modules. `crate::fsops::temp_dir` (test-only)
→ the local per-module `temp_dir` helper used elsewhere in the crate's tests.

**Test delta (X parity + Y not-regressed):**
- kasetto's `prompts/*` `#[cfg(test)] mod tests` ported VERBATIM (paths adapted):
  3 parse + 7 transform + 2 driver = 12.
- Added DUAL-GATE coverage: `destination_path` for all 5 formats, `render` for all 5 with
  no-frontmatter, the frontmatter opening-without-closing edge (error), CRLF normalization,
  and TOML quote-escape = 6.
- Crate unit tests **78 → 96 (+18)**; existing 78 unchanged (Y not regressed). Plus the
  1 `no_downgrade.rs` integration test still green.

**DUAL GATE — all green (raw via `rtk proxy`):**
`build=0` · `test=0` (96 unit + 1 integration, 0 failed) · `clippy -D warnings=0` ·
`fmt=0` · `no-c=0` (rustls 0.23.40 on ring 0.17.14, zero aws-lc/openssl/C-SQLite).

**Invariants:** non-printing library (Result-returning, no println/clap), `forbid(unsafe_code)`,
snake_case modules / PascalCase types, inline `#[cfg(test)] mod tests`, no C / one rustls ring-only.

**Commit:** `de7a6a6` (not pushed). No stub/dropped format — every branch ported.

## FE-* — config-edit mutation engine (TASK-0012) — DONE
- **Module:** `crates/agent-env/src/config_edit.rs` (new); `pub mod` + `pub use` in `lib.rs`.
- **Ported verbatim from** kasetto v3.2.0 `src/fsops/config_edit.rs` (811L) + reusable parsing from `src/commands/source_edit.rs`.
  - Value types: `Section`(key/singular), `Pin`(Ref/Branch/None, value), `Selector`(Wildcard/Names), `SourceItem`, `RemoveOutcome`(NotFound/WholeItem/Names).
  - Mutation fns: `insert_item`, `remove_item`, `remove_names`, `item_exists` — raw line/text editing, **comment-preserving (NOT lossy serde round-trip)**.
  - Source parsing: `split_at_ref`, `is_remote_source`, `ensure_local_config`.
  - Error map: `crate::error::{err,Result}` → `crate::{err,Result}` (AgentEnvError::Message channel). Visibility `pub(crate)`→`pub` so lib re-exports.
  - Out of scope (deferred TASK-0013/0014): `sync_after`, default-path resolve, clap/print drivers (add.rs/remove.rs command logic).
- **DUAL GATE:**
  - X parity: kasetto `#[cfg(test)] mod tests` ported VERBATIM (all pass) + 6 split_at_ref/local-config tests + 3 added comment-preservation gate tests (add & remove preserve surrounding comments).
  - Y: whole crate green — **130 lib tests** (was 96, +34), clippy `-D warnings` clean, fmt clean, **no-c PASS**.
- **Comment-preservation verbatim & non-lossy:** YES — text/line-level editing carried over byte-for-byte; no `serde_yaml::from_str`→`to_string` swap.
- **Commit:** `75b2ec6` (not pushed).

## XC-03 / XC-04 / F-03..F-10 — fsops path/target-resolution cluster (+ dirs/util leaf deps) (TASK-0012) — DONE
- **Modules:** `crates/agent-env/src/{dirs,util,fsops}.rs` (all NEW); `pub mod` + `pub use` re-exports appended to `lib.rs`.
- **Ported verbatim from** kasetto v3.2.0 `src/fsops/{dirs,mod,copy,settings}.rs` (NO downgrade — every branch/error path/cfg-arm carried):
  - `dirs` (XC-03): `dirs_home` (ERR "HOME is not set"), `dirs_xdg_{config,data,cache}_home` (honor `XDG_*_HOME` only when non-empty, else `$HOME/.config|.local/share|.cache`), and the per-product dir helpers.
    - **Kasetto-dir rename:** the product-self-named `dirs_kasetto_{config,data,cache}` → **`dirs_agent_env_{config,data,cache}`** (append `agent-env`, not `kasetto`). The agent-NATIVE XDG bases are kept byte-for-byte; only the kasetto-self-named leaf changed (documented in the module header + each fn doc). Threaded through `resolve_mcp_settings_targets` (the `kasetto_config` arg → `agent_env_config`).
  - `util` (XC-04): `now_unix` / `now_unix_str` (saturating `unwrap_or(0)` pre-epoch guard).
  - `fsops` (F-03..F-10): `copy_dir`/`copy_dir_contents`/`copy_file` (MAX_COPY_DEPTH=32 symlink-cycle ERR, dst removed first, symlink-followed via canonicalize, `fs::copy` +x preserve, `cfg(windows)` READONLY strip kept); `SettingsFile::load/save` (JSON, load-existing-or-`{}`, "invalid settings JSON" ERR, pretty save + parent-dir create) — **self-contained, faithful SUPERSET of parallel PR #73** (`fsops::SettingsFile`); `resolve_path` (F-05, leading-`~/` & bare `~` only); `select_targets`+`BrokenSkill`+`TargetSelection` (F-06: wildcard `*`→all sorted, Name→lookup-or-broken, Obj{path}→SKILL.md-checked else broken, non-`*` wildcard → "invalid skills field"); `resolve_destinations` (F-07); `resolve_mcp_settings_targets` (F-08); `resolve_command_targets` (F-09); `scope_root`/`relativize_dest`/`resolve_dest` (F-10 lock-portability core).
- **Integration / mapping:** reused agent-env `config` (SourceSpec/SkillsField/SkillTarget/Config/Scope) + `Agent` path methods (M-11..M-16: `project_path`/`global_path`/`mcp_*_target`/`commands_*_path`) + `agent::{McpSettingsTarget,CommandTarget}`. kasetto `err(...)`/`Result` → `crate::{err,Result}` (`AgentEnvError::Message`). Added `AgentEnvError::Json(#[from] serde_json::Error)` variant so `SettingsFile::save`'s `?` on the pretty serializer maps cleanly (matches PR #73's documented intent).
- **DUAL GATE:**
  - X parity: kasetto `#[cfg(test)] mod tests` from `mod.rs`/`copy.rs`/`settings.rs` ported VERBATIM + required coverage: `resolve_path` leading-`~`-only (mid-path `~` literal), `select_targets` wildcard sort + missing/explicit-path/relative-path + non-`*` error + Obj-missing-SKILL.md, `scope_root`/`relativize_dest`/`resolve_dest` round-trip + out-of-root, copy +x / symlink-follow / **depth-guard symlink-cycle refusal**, `resolve_destinations`/`resolve_mcp_settings_targets`/`resolve_command_targets` per-agent + dedupe + filter. (HOME-dependent tests made race-immune — no process-global env mutation, since other modules' tests poke HOME concurrently.)
  - Y not regressed: whole crate green — **159 lib tests** (was 130, +29) + 1 doctest, clippy `-D warnings` clean, fmt clean, **no-c PASS** (rustls 0.23.40 on ring 0.17.14, zero aws-lc/openssl/C-SQLite).
- **Invariants:** non-printing library (Result, no println/clap), `forbid(unsafe_code)`, snake_case modules / PascalCase types / SCREAMING_SNAKE consts (`MAX_COPY_DEPTH`), inline `#[cfg(test)] mod tests`, MSRV 1.80, cross-platform `cfg(windows)` arm preserved, no C / one rustls ring-only.
- **No stub / no dropped branch.** Faithful superset of PR #73; later merge reconciles to this copy.

---

## TASK-0012 cluster: source materialize + asset discovery (S-14..S-21) — rust-port-MERGE

**Source:** `kasetto/src/source/mod.rs` (642L). **Dest:** `crates/agent-env/src/source.rs` (EXTEND on S-01..S-13).
**Branch:** `task-0012-source-discovery` (off develop). **Commit:** `4363bdb`.

**Units ported (all full, no stubs):**
- S-14 `materialize_source` (+ `MaterializedSource`): local in-place (`source_revision="local"`, `cleanup_dir=None`) vs remote `download_extract` into caller-provided `stage` (`cleanup_dir=Some(stage)`); revision label carried (`ref:`/`branch:`/`branch:main`).
- S-15 `GitPin::Default` main→master retry arm ported verbatim (`.or_else` second fetch, "(also tried branch `master` after `main`)" message).
- S-16 `resolve_source_root` + `repo_name_hint`: sub-dir validated (empty/absolute/`..`/RootDir fail-closed; not-found / not-dir errors), repo-name hint per host variant.
- S-17 `discover` / `discover_with_root_name` / `discover_skills_in_subdir`: root-level SKILL.md named by hint, `skills/` subdir walk, shadow warning.
- S-18 `discover_mcps`: `.mcp.json`/`mcp.json` root + `mcps/*.json` (legacy `mcp/` warning; reworded to drop the kasetto-self name).
- S-19 `resolve_mcp_entry`: Name→`mcps/`, Obj→custom/default dir, auto `.json`.
- S-20 `discover_commands` / `walk_commands`: `commands/**/*.md` → `:`-namespaced.
- S-21 `resolve_command_entry` / `resolve_named_command`: Name via discovery, Obj via path.

**Integration / mapping:** reused agent-env `RepoUrl`/`parse_repo_url`/`remote_repo_archive_{ref,branch}`/`download_extract`; `crate::config::{SourceSpec,GitPin,McpEntry,CommandEntry}`, `crate::fsops::resolve_path`. `err(...)`→`AgentEnvError::Message`. Kept the caller-provided `stage` signature (no kasetto-named cache dir was introduced; `dirs_agent_env_cache` not needed). Re-exported new symbols from `lib.rs`.

**DUAL GATE:**
- X parity: ported kasetto's `mod tests` verbatim (adapted to local `temp_dir` + `crate::config` types) + added `resolve_source_root` un-nest/escape coverage. Covers discover (root + skills/ subdir), sub-dir honoring, mcp Name/Obj/auto-json/missing, command nested-namespace/Obj/missing, wildcard vs named.
- Y not regressed: whole crate green. **202 tests pass (was 181), 1 ignored.**

**Network-test handling:** the only networked path (remote `materialize_source` main→master) is a single `#[ignore]`d test (`remote_materialize_main_to_master_fallback`, hits `github.com/git/git`); the un-nest logic it depends on is exercised offline via `resolve_source_root_*` against pre-stripped stage trees. Suite stays offline + deterministic, mirroring the existing `extract_tar_gz` fixture approach.

**Verify:** build / test (202 pass, 1 ignored) / `clippy --all-targets -D warnings` / `fmt --check` / `ci/gates/no-c.sh` — all green.

---

## rust-port-MERGE cycle — agent-env LIBRARY tail (ST/P/CP/C-05-06) — commit 3dbe7d0

**Porter:** rust-port-porter (verify/merge mode). Branch `task-0012-source-discovery`.

**Units ported (4 new modules, verbatim from kasetto v3.2.0, no downgrade):**
- `runtime.rs` (ST-01/ST-02) ← `kasetto/src/state.rs` — `RuntimeState` (+ updated_at/set/forget/save_report_json/load_latest_failures) + `runtime_state_path`/`load`/`save`/`clear`. Separate agent-asset runtime payload (kept distinct from any engine runtime). Adapted `lock_path(scope,root)?` → agent-env's infallible `lock_path(scope,root,&dirs_agent_env_data()?)`.
- `profile.rs` (P-01/P-02) ← `kasetto/src/profile.rs` — `read_skill_profile`/`read_skill_profile_from_dir` + `format_updated_ago`. UI-only `list_color_enabled` intentionally NOT ported (non-printing library).
- `config_path.rs` (CP-01) ← `kasetto/src/lib.rs` — `default_config_path`, `resolve_config_path`, `Preferences` (+ filename/env consts).
- `sync.rs` (C-05/C-06) ← `kasetto/src/commands/sync/` — pure `remove_stale` + `StaleEntry`; key conventions `skill_key`/`command_asset_id`/`mcp_asset_id` (+ `*_action_label`). Engine `agent_sync` driver remains TASK-0013 (out of scope).

**kasetto → envctl renames (absorbed tool's OWN identity):**
- env var `KASETTO_CONFIG` → `ENVCTL_AGENT_CONFIG`
- config filename `kasetto.yaml` → `agent-env.yaml` (local + global)
- per-product config dir `kasetto/` → `agent-env/` (`dirs_agent_env_config`)
- runtime cache dir `kasetto/` → `agent-env/` (`dirs_agent_env_cache`)

**Integration:** reused `config::Scope`, `lock::{AssetEntry,AgentLockFile,lock_path}`, `hash::hash_str`, `dirs::*`, `report::{Action,Summary,SyncFailure}`, `util::now_unix`. `err(...)`/`Result` → `crate::AgentEnvError`/`crate::Result`.

**DUAL GATE — PASS:** kasetto inline tests ported verbatim+adapted (RuntimeState round-trip/clear, load_latest_failures, format_updated_ago boundaries, resolve_config_path priority ladder, remove_stale absent-only). Whole crate **226 passed / 1 ignored** (was 202). `clippy -D warnings` clean, `fmt --check` clean, `ci/gates/no-c.sh` PASS. HOME/XDG-mutating tests serialized via `env_lock` mutex (race-safe with sibling env tests).
