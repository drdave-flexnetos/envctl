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
