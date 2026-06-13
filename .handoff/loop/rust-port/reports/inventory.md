# Rust-port inventory — kasetto v3.2.0 → envctl-agent-env

## Source & target

- **SOURCE:** `meta/kasetto` @ git tag **v3.2.0** (`ec01cca`, Cargo `version = "3.2.0"`) — confirmed HEAD is the v3.2.0 tag, so the inventory targets v3.2.0 directly (not the stale `docs/KASETTO-FEATURES.md` @ v3.0.0, and not the installed 3.1.0 binary). The v3.1+ surface a naive v3.0.0 port would drop (`add`, `remove`/`rm`, `lock --check`, `lock --upgrade-package`, `config_edit.rs`, `source_edit.rs`) is **all present at HEAD** and is fully inventoried below.
- **TARGET:** `crates/agent-env` (package `envctl-agent-env`), driven engine-first through the Engine API, surfaced as `envctl agent {sync,add,remove,lock,list,clean}`.
- **SEED:** commit `6ecb270` ports `src/{config,extend,source,hash,lock,lib}.rs` + `tests/no_downgrade.rs` (61 unit + 1 integration GREEN).

## Source size

13,700 lines of Rust across 53 `*.rs` files. The 5 heaviest carry the absorb-critical logic: `config_edit.rs` (811), `sync/skills.rs` (813), `sync/commands.rs` (721), `sync/mcps.rs` (658), `fsops/mod.rs` (656), `source/mod.rs` (642).

## Ledger counts

**81 total ledger rows**, classified:

| Class | Mark | Count | Meaning |
|-------|------|------:|---------|
| Seed-credited (ported, unproven) | `- [~]` | **35** | foundational config/extend/source/hash/lock surface in commit 6ecb270 — parity-verifier upgrades to `- [x]` |
| Remaining to port | `- [ ]` | **33** | real absorb work not yet in the seed (incl. seed PARTIAL-remainder rows) |
| Front-end / already-ported (intentional divergence) | `- [≠]` | **13** | envctl owns rendering / clap / self-update / doctor; recorded so the pre-DONE sweep treats them as diverged, not missed |

- **Absorb units total** (`- [~]` + `- [ ]`) = **68**.
- **Seed-covered** = 35 `- [~]` (foundational layer GREEN but unproven; 4 of these are PARTIAL — see below).
- **Remaining to port** = 33 `- [ ]`.

### Seed PARTIAL-remainder rows (explicit `- [ ]` so a partial port can't read as complete)

The seed implementer log (`.handoff/loop/cycle/02_implementer_log.md`) flags three plan-sanctioned defers; each is now a distinct `- [ ]` row plus the surrounding un-ported surface:

1. **21-preset per-agent native path methods** (M-09 enum shape is `- [~]`; the path TABLES are `- [ ]`): M-11 `global_path`, M-12 `project_path`, M-13 `mcp_settings_target`, M-14 `mcp_project_target`, M-15 `commands_global_path`, M-16 `commands_project_path`, plus M-17–M-20 (`all_*_targets`, dedup helpers, `vscode_user_mcp_json` OS-branch) and the M-26/M-27 format enums (`McpSettingsFormat`×4, `CommandFormat`×5) those methods return.
2. **`resolve_scope` file-read fallback** (M-22): the seed ports CLI>cfg>Global (M-21 `- [~]`); the "read default config when no Config passed" arm is `- [ ]`.
3. **`main → master` archive retry** (S-15): the seed's `archive_url(GitPin::Default)` returns the `main` URL; the live second-HTTP-attempt retry is the materializer's job (`- [ ]`, part of the un-seeded `materialize_source` S-14).

The entire `commands/*` business-logic layer (sync orchestration, add/remove/lock/list/clean, source_edit), `config_edit.rs` (the comment-preserving `add`/`remove` mutation engine), the MCP-merge (4 formats), the command-format transforms (5), source discovery (`discover`/`discover_mcps`/`discover_commands`/`resolve_*_entry`/`materialize_source`), `copy_dir`, `select_targets`, `SettingsFile`, `RuntimeState`, `profile.rs`, and the sync-result value types (Summary/Action/Report/InstalledSkill) are **all un-seeded** `- [ ]`.

## 11 → 6 verb mapping (the no-downgrade collapse — zero behavior dropped)

| # | kasetto verb | envctl `agent` target | Ledger rows | Notes |
|---|--------------|-----------------------|-------------|-------|
| 1 | `sync` | `agent sync` | C-01..C-06 | core provision: fetch+transform+install skills/mcps/commands; `--dry-run`/`--json`/`--update`/`--locked`/`--scope` |
| 2 | `add` | `agent add` | C-07 (+ FE-02/FE-05 insert, C-12 split_at_ref/sync_after) | v3.1; appends skill/mcp/command source; deep browse-URL decompose; verify-on-add |
| 3 | `remove`/`rm` | `agent remove` (alias `rm`) | C-08 (+ FE-03/FE-04, C-12) | v3.1; subtract names or drop whole source; alias preserved |
| 4 | `lock` | `agent lock` | C-09 (+ L-06) | re-resolve+pin; carries `--check`/`--upgrade-package` (v3.1) |
| 5 | `lock --check` | `agent lock --check` | C-09 (diff_summary) | CI/verify, exit-1 on drift, never writes |
| 6 | `lock --upgrade-package` | `agent lock --upgrade-package` | C-09 (upgrade_active) | v3.1; restrict re-resolve to named skills |
| 7 | `list` | `agent list` | C-10 | enumerate installed skills/mcps/commands, scope-merged |
| 8 | `clean` | `agent clean` | C-11 | remove all provisioned assets, fail-closed |
| 9 | `init` | `agent add` (init path) / `agent sync` bootstrap | C-13 | template+path = business; TTY prompt/banner = front-end |
| 10 | `status` | `agent list` (status mode) / `agent lock --check` | C-10 / C-09 | folds in |
| 11 | `validate` | `agent lock --check` (validate mode) | C-09 | folds in |

`self update` / `self uninstall` / `completions` / `doctor` are **front-end / envctl-owned** (FRONTEND-07/08/09/10, C-14): the asset-cleanup portion of uninstall folds into `agent clean` (C-11/C-14); the binary/dir/self-update mechanics are envctl's own install concern; `doctor` is already ported (do not re-implement).

## Biggest no-downgrade risks (where a naive port silently drops capability)

1. **MCP additive/never-clobber merge (C-04 + MC-01..MC-02, 4 formats).** The single most likely silent regression (absorption §7). `merge_into_json_key` only inserts keys NOT already present — a config containing global `broker`/`repowire`/`weave` MUST still contain them after `agent sync`, side-by-side with the 6 baseline servers. The 4 formats (McpServers, VsCodeServers w/ stdio/http type injection, OpenCode local/remote shape, CodexToml remote/stdio table) each have their own transform + remove + present-check; **all 4 must merge additively**. The named regression fixture (seed 3 + assert 9) is mandatory.
2. **`config_edit.rs` comment-preserving mutation (FE-01..FE-06, 811 lines).** `add`/`remove` edit the user's YAML at the raw-line level (serde round-trip would drop comments + reorder keys). The disambiguation rules (ambiguous source+pin+sub-dir → ERR), the wildcard/object-form refusals in `remove_names`, the inline-list normalization, and the indent-inheritance are all easy to under-port. Every error branch is its own ledger row.
3. **21-agent native-path table (M-11..M-20).** Six per-preset path methods × 21 agents, each with its own destination layout, plus the global/project divergences (amp/replit, cline/warp, windsurf, goose) and the OS-branching VS Code path. SEED ported only the enum shape — the entire path table is the remaining work and a subset is a downgrade.
4. **`--locked` zero-network + never-prune-on-partial-failure (C-02/C-03/C-04, L-06).** Three sync phases each re-implement: locked-satisfiable pre-check, needs_fetch decision, and the `summary.failed == 0` guard around `remove_stale` (a failure must NOT prune already-installed assets). `--locked` must have no reachable fetch path. The seed's `LockMode::allows_fetch()/should_resolve()` captures the type-level semantics; the command-level enforcement is un-seeded.
5. **tar-slip path-traversal guard (S-07, seeded).** `download_extract` strips the top archive dir and refuses `ParentDir` with "unsafe archive path" — a security guard, fail-closed, never weaken. Pure-Rust flate2 (`rust_backend`/miniz_oxide) + tar, no C zlib (no-c gate).
6. **lock↔runtime separation (ST-01/ST-02, L-04, AP-01/AP-02).** Machine-local `RuntimeState` (last_run, installed_at, latest_report) stays in the cache dir, OUT of the committed lock. The committed SHA-256 agent-asset section is a SEPARATE keyed section in `envctl.lock`; the FNV-1a component section is UNTOUCHED. Two hash families coexist by design — rehashing components under SHA-256 is a downgrade.
7. **Source resolver completeness (S-01..S-21).** 4 host families (GitHub +GHE, GitLab +subgroups/self-hosted, Bitbucket, Gitea/Forgejo/Codeberg), browser-URL→raw rewrite (3 host dialects), `SOURCE@REF` pin (`split_at_ref` SSH/userinfo round-trip), default-branch main→master fallback, env-only credentials (never serialized), and `extends` cycle+depth(8) guards. Mostly seeded; the discovery/materialize layer (S-14..S-21) and the main→master retry (S-15) are not.

## Deferred / coverage notes

- **No source area was sampled.** Every `*.rs` file carrying logic was read in full; front-end files (ui/banner/colors/cli/app/update_notifier/self_update/completions) are recorded as `- [≠]` rows, not omitted.
- **Tests are inventory.** kasetto's inline `#[cfg(test)]` suites (≈120 tests) enumerate the behaviors the authors cared about; each distinct behavior (separator-invariant hash, ambiguous-remove error, additive-merge-preserves-existing, locked-zero-network, main→master fallback, deep-URL SHA-pin, etc.) is a future parity fixture and is reflected in the contract text of its row.
- **Pre-DONE left-behind sweep** must re-walk the source and diff against this ledger; any `- [ ]`/`- [~]` row, or any source unit absent here, blocks DONE. Treat "that's everything" as a hypothesis to disprove.

## Headline

**68 absorb units** — **35 seed-covered `- [~]`** (foundational config/extend/source/hash/lock; 4 PARTIAL), **33 `- [ ]` remaining** (all of commands/* business logic, config_edit mutation engine, MCP 4-format merge, 5 command transforms, source discovery/materialize, copy/select/settings/runtime/profile, sync-result types, 21-agent path table), and **13 `- [≠]` front-end** rows (clap/ui/banner/colors/self-update/doctor/completions — envctl owns rendering).
