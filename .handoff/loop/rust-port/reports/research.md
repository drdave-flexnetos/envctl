# Reuse map — kasetto v3.2.0 ⟷ envctl (rust-port-MERGE research)

**Mode:** verify/merge. **X (source):** `/home/drdave/Desktop/meta/kasetto` @ v3.2.0 (Cargo 3.2.0).
**Y (dest):** envctl worktree `/home/drdave/Desktop/meta/.worktrees/task-0012-agent-env/envctl`
@ `8006b4c` (branch `task-0012-agent-env`). **Unit list:** `.handoff/loop/rust-port/parity-ledger.md`
(112 rows: 55 `- [~]`, 44 `- [ ]`, 13 `- [≠]`).

**Method:** git-kb code intelligence (`git-kb code index/symbols/callers`, 719 symbols indexed over
`crates/{agent-env,engine,cli}`) + source read of both repos. Graph, not grep, for symbol existence;
grep only for file-coverage sweep and string evidence.

## Headline

The merge is **near-greenfield with one shared-substrate analogy**. envctl's two domains are cleanly
separated: `crates/engine` provisions the **workstation** (manifest *components*); the new
`crates/agent-env` provisions the **agent assets** (skills/MCPs/commands). They share **no
symbol-level code** — agent-env carries its own pure-Rust HTTP/crypto/lock. The only Y-already-provides
relationship is **analogy at the component grain** (engine's lock/runtime/doctor), which kasetto's
*agent-asset* lock/runtime/doctor cannot reuse directly (different keying, different file). So:

- **reuse-Y (direct symbol reuse): 0.** No agent-asset unit maps onto an existing engine symbol.
- **reuse-by-analogy / DO-NOT-RE-PORT: 3** — the AP rows (`- [≠]`): engine lock (FNV-1a, component
  grain), engine runtime (component grain), engine doctor. These remain untouched; the agent-asset
  lock/runtime/doctor verbs are a **parallel** surface that agent-env owns.
- **port-fresh: 99** — 55 already landed in agent-env (re-verify pending) + 41 still-todo `- [ ]`
  ABSORB units + 3 newly-discovered left-behind (mcps/prompts). agent-env is the home for all of them.
- **extend-Y: 0** — there is no *partial* agent-asset impl in Y to complete; in particular the lock is
  **not** a duplicate (proven below).
- **map-onto-substrate: 0** — nothing here delegates to hf/weave/grit/icm.
- **front-end (`- [≠]`): 13** — FRONTEND-01..10 + the M-22/S-15-style deferred arms stay port-fresh.

## reuse-Y / Y-provides table (the analogy layer — component grain, DO NOT re-port)

| kasetto capability (X) | Y already provides (symbol · file:line) | class | evidence |
|---|---|---|---|
| §2 committed content-lock + `lock --check` CI gate (AP-01) | `envctl_engine::lock` — `LockFile`, `LockEntry`, `lock_path` (`crates/engine/src/lock.rs:52`), `generate` (:86), `diff`→`LockDriftKind{Added,Removed,Changed}` (:105,:112), `component_hash`→`fnv1a_hex` (:134,:164), `LOCK_FILENAME="envctl.lock"` (:17), driven by CLI `Cmd::Lock{check}` (`crates/cli/src/main.rs:269`) | reuse-Y (analogy) | engine lock is **FNV-1a**, keyed by **component**, file `envctl.lock`, `LOCK_VERSION=1`. The kasetto agent-asset lock is **SHA-256**, keyed by **skill/asset**, file `agent-env.lock`, `LOCK_VERSION=2`. Rehashing components under SHA-256 = downgrade → keep separate. |
| §16 machine-local runtime, kept OUT of committed lock (AP-02) | `envctl_engine::runtime` — `RuntimeState{last_run:Option<LastRun>}` (`crates/engine/src/runtime.rs:11`), `load` (:60), `record_run` (:68); cache-keyed by manifest-dir hash | reuse-Y (analogy) | engine runtime tracks the **component-run** record (`LastRun{verb,phase,RunSummary}`). kasetto's `state.rs` runtime (ST-01/02) tracks the **agent-sync** report (`latest_report`, `installed_at: BTreeMap`, `load_latest_failures`). Same *separation principle*, different payload — mirror it, don't fold. |
| `doctor` diagnostics (AP-03 / FRONTEND-09) | `print_doctor` (`crates/cli/src/main.rs:876`) — probes writable dirs, tool versions, sudo/UEFI/secure-boot/nvidia, run-log; reads `runtime::load(...).last_run` | reuse-Y (analogy) | envctl's doctor diagnoses the **workstation**, not agent-asset health. kasetto's `commands/doctor.rs` is front-end rendering (FRONTEND-09); its only absorb-adjacent probe (command-dir-writable) is covered by M-15/M-16/M-19 target resolution, already in agent-env (`crates/agent-env/src/agent.rs:117-137`). No re-port. |

> **Why no direct reuse-Y for agent-asset units:** the engine exposes **none** of the agent-asset
> surface — `grep` over `crates/engine/src/*.rs` finds no `copy_dir`, `resolve_path`, `dirs`,
> `SettingsFile`, `http_client`, `hash_dir/hash_str` (only the component-scoped `fnv1a_hex`). agent-env
> brings its own (`crates/agent-env/src/{hash,source}.rs`). So every ABSORB unit lands **fresh in
> agent-env**; none can be satisfied by an existing engine symbol.

## Duplication findings (agent-env vs engine — the "no downgrade / nothing left behind" core)

**FINDING-D1 — lock: NOT a duplicate (verified). No action.**
Two locks coexist by design:
- `crates/engine/src/lock.rs` — `LockFile`/`LockEntry{content_hash,requires,resolved}`,
  `component_hash()` = canonical-JSON → **FNV-1a** (`:134-136`, `fnv1a_hex` `:164`), file
  `envctl.lock` (`:17`), `LOCK_VERSION=1` (`:16`). Tracks **manifest components**.
- `crates/agent-env/src/lock.rs` — `AgentLockFile`/`AgentLockEntry`/`AssetEntry`, `lock_check()`→
  `Vec<LockDrift>` (`:193`), `LockMode{Plain,Update,Locked}` (`:98`) with `allows_fetch`/
  `should_resolve`, file `agent-env.lock` (`:35`), `LOCK_VERSION=2` (`:28`), **SHA-256** content hash
  via `crates/agent-env/src/hash.rs` (`sha2::Sha256`, `:12`). Tracks **agent assets** (skills/MCPs/
  commands) — a *separately keyed* lock.
The agent-env lock header states it explicitly: *"a separate type from the engine's FNV-1a component
lock … they do not share code"* (`crates/agent-env/src/lock.rs:5-6`, `lib.rs` doc). Different hash
algorithm, different file, different version, different key domain → **genuinely distinct, not an
accidental duplicate.** Matches ledger AP-01. **Class: leave both (no `extend-Y`).**

**FINDING-D2 — hashing: NOT a duplicate.** engine has only `fnv1a_hex` (component spec, `lock.rs:164`);
agent-env has `hash_dir/hash_str/hash_file` (SHA-256, `hash.rs:18/45/52`) for skill-tree content. No
overlap.

**FINDING-D3 — runtime: NOT a duplicate, but a separation to PRESERVE.** engine `RuntimeState`
(component-run) and kasetto `state.rs RuntimeState` (agent-sync) are different payloads. ST-01/ST-02
must land as agent-env's **own** runtime (or reuse engine's *pattern*), keeping lock↔runtime separation
(ADR-0001 §4). Not a duplicate; a parallel.

**FINDING-D4 — config-parse / source-fetch / merge_yaml: NOT duplicated in engine.** engine has its own
TOML manifest model (`crates/engine/src/model.rs`) for *components*; it does **not** parse kasetto's
YAML agent config. agent-env owns `config.rs`/`extend.rs`/`source.rs`. No collision.

**Net: 0 real duplicates.** The one structurally-tempting collision (lock) is verified-distinct. No unit
needs `extend-Y` for de-duplication.

## Left-behind sweep (re-scan kasetto/src vs the ledger — assume something was missed)

File-coverage sweep: every `kasetto/src/**/*.rs` checked for a ledger row referencing its path.
**7 files are referenced by NO ledger row:**

| kasetto file | symbols | status | finding |
|---|---|---|---|
| `src/commands/mod.rs` | (module decls only) | benign | Pure `mod` glue (12 `pub(crate) mod …` lines, no logic). Each command already rowed (C-01..C-14). **Not a left-behind** — trivially covered. |
| `src/mcps/mod.rs` | `merge_mcp_config` (:16), `remove_mcp_server` (:30), `servers_present_in_settings` (:58), `json_remove_top_level_key` | **LEFT-BEHIND** | Real logic, **referenced as dep `MC-01`/`MC-02` by C-04/C-11 but has NO row.** |
| `src/mcps/merge.rs` | `merge_into_json_key`, `merge_mcp_servers_object` (:39), `merge_vscode_servers_object` (:43), `merge_opencode_mcp_object` (:49), `normalize_vscode_server`, `mcp_entry_to_opencode` | **LEFT-BEHIND** | The 3 JSON MCP-merge formats (mcpServers/VsCode/OpenCode) + additive-never-clobber. Part of `MC-01`. |
| `src/mcps/codex.rs` | `merge_codex_config_toml` (:11), `remove_server` (:35), `servers_present` (:47), `json_mcp_server_to_codex_toml_table` (:83) | **LEFT-BEHIND** | The CodexToml MCP-merge format (4th format). Part of `MC-01`/`MC-02`. |
| `src/mcps/pack.rs` | `read_source_mcp_servers` (:8) | **LEFT-BEHIND** | Reads `mcpServers` from a pack JSON. Shared by all merge formats. Part of `MC-01`. |
| `src/prompts/mod.rs` | `apply_command` (:18), `destination_path` (re-export) | **LEFT-BEHIND** | **Referenced as dep `PR-01` by C-03 but has NO row.** The command-transform driver. |
| `src/prompts/parse.rs` | `Parsed` (:5), `Parsed::description` (:14), `parse` (:28) | **LEFT-BEHIND** | Markdown-frontmatter split (CRLF-norm, opening-without-closing `---` → err). Part of `PR-01`. |
| `src/prompts/transform.rs` | `render` (:40), `destination_path` (:105), `ensure_parent_dirs` (:110), `derive_relpath`, `render_prompt_file`, `render_gemini_toml`, `toml_string`, `name_to_nested_path`, `flatten_name` | **LEFT-BEHIND** | The 5 command-format transforms (MarkdownFrontmatter nested-path, MarkdownPlain, PromptMd, PromptFile→`{{{ input }}}`/`invokable:true`, GeminiToml). Part of `PR-01`. |

**Cross-check via the ledger's own deps:** `MC-01`, `MC-02`, `PR-01` appear **only** in `deps:` fields
(C-03 → PR-01; C-04 → MC-01; C-11 → MC-02) — never as a row id. The cartographer wired the dependencies
but never created the dependency rows. agent-env confirms the gap: it has the MCP/command **type**
shells (`McpSettingsFormat`/`CommandFormat`/`McpSettingsTarget`/`CommandTarget`, M-26/M-27, in
`crates/agent-env/src/agent.rs:28-67`) and the **target-resolution** (M-13..M-20), but **NOT** the
merge/transform **implementations**. So the sync commands C-03/C-04/C-11 have no engine to call.

**Three new rows added** (covering the 7 logic files; `commands/mod.rs` excluded as glue):

- `MC-01` — MCP-pack merge engine across all 4 native formats (mcps/{mod,merge,codex,pack}.rs).
- `MC-02` — MCP server removal + presence check across all 4 formats (mcps/mod.rs + codex.rs).
- `PR-01` — command (slash-command/prompt) parse + 5-format transform + write (prompts/{mod,parse,transform}.rs).

All three are **port-fresh** into agent-env (new modules `agent-env::mcps`, `agent-env::prompts`),
preconditions for the C-03/C-04/C-11 sync phases (TASK-0013). **No-downgrade impact: without these, the
`agent sync` MCP and command phases cannot land — they are not optional.**

## Engine-already-absorbed verification (CLAUDE.md claim)

CLAUDE.md / ledger AP-01..03 claim engine already absorbed kasetto's §2 lock / §16 runtime / doctor /
`lock --check`. **Confirmed at the COMPONENT grain:**
- lock + `lock --check`: `crates/engine/src/lock.rs` + `crates/cli/src/main.rs:269` (`Cmd::Lock{check}`,
  exits nonzero on drift). ✓
- runtime: `crates/engine/src/runtime.rs`. ✓
- doctor: `crates/cli/src/main.rs:876` (`print_doctor`). ✓
These cover kasetto §2/§16/doctor **for manifest components**. They do **not** cover the **agent-asset**
lock/runtime/doctor verbs (L-*, ST-*, C-09 `agent lock --check`, C-10 `agent list/status`), which are a
distinct surface agent-env owns. So the AP rows are correctly `- [≠]` (analogy, do-not-re-port) — and
the agent-asset lock/runtime units remain **port-fresh in agent-env**, not reuse-Y.

## Per-class count summary (112 parity rows + 3 left-behind = 115)

| Class | Count | Which |
|---|---|---|
| `reuse-Y` (direct symbol) | **0** | — (no agent-asset unit maps onto an existing engine symbol) |
| `reuse-Y` / DO-NOT-RE-PORT (analogy) | **3** | AP-01, AP-02, AP-03 (`- [≠]`; engine lock/runtime/doctor, component grain) |
| `extend-Y` | **0** | — (lock proven non-duplicate; no partial agent-asset impl to complete) |
| `port-fresh` | **99** | 55 already-landed-in-agent-env (`- [~]`, re-verify pending) + 41 todo ABSORB `- [ ]` + 3 left-behind (MC-01/MC-02/PR-01) |
| `map-onto-substrate` | **0** | — |
| `front-end` (`- [≠]`) | **13** | FRONTEND-01..10 |
| **#duplications found** | **0** | (lock collision verified distinct) |
| **#left-behind found** | **3** | MC-01, MC-02, PR-01 (7 source files) |

> Reconciliation of the 44 `- [ ]` parity rows: 41 are ABSORB units → **port-fresh into agent-env**; the
> remaining "front-end-flavored" deferred arms (M-22 fallback, S-15 retry) are still port-fresh
> (deferred logic), not front-end. The 3 AP `- [≠]` of the 13-front-end bucket are the analogy/reuse
> rows; the 10 FRONTEND-* are envctl-owned rendering.
