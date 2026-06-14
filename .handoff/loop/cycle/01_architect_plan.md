# TASK-0013 — Engine wiring for the agent-env subsystem (Epic C)
# Architect plan (feature-architect) — 2026-06-14

VERDICT: GO

Wire kasetto's 6 agent-asset verbs into envctl's engine as `Engine::agent_{sync,add,remove,lock,list,clean}`
in a new `crates/engine/src/agent/` module tree that orchestrates the already-ported `crates/agent-env`
library; the engine stays sync, non-printing, emits new `Event::Agent*` variants. All mutating verbs
default to preview (dry-run) and require explicit `apply`; the MCP merge stays additive/never-clobber;
lock has 3 modes (`plain`/`--update`/`--locked`) keyed to the SHA-256 `agent-env.lock`, leaving the
engine's FNV-1a component lock (`crates/engine/src/lock.rs`) untouched. Phase ordering ported faithfully
from kasetto v3.2.0 `src/commands/{sync/mod,add,remove,lock,list,clean}.rs` + the M-22 `resolve_scope`
file-read fallback folded into the engine entry path.

## Target repos
**envctl only.** Engine modules (8 new + 2 edits + Cargo.toml). Tightly-coupled modules (add/remove
call sync; shared `AgentCtx`) → SEQUENTIAL single-crew build.
- NEW `crates/engine/src/agent/mod.rs` — re-exports + `AgentCtx` (engine analog of kasetto `SyncContext`)
  + scope/config resolution incl. **M-22** fallback arm.
- NEW `agent/sync.rs` — `agent_sync` + 3 phases (C-01/02/03/04).
- NEW `agent/edit.rs` — `agent_add` (C-07) + `agent_remove` (C-08) + `config_edit`→`sync_after` (C-12).
- NEW `agent/lock.rs` — `agent_lock` (C-09 `--check`/`--upgrade-package`). At `crate::agent::lock`,
  NEVER merges with `crate::lock` (FNV-1a component lock).
- NEW `agent/list.rs` — `agent_list` (C-10).
- NEW `agent/clean.rs` — `agent_clean` (C-11/C-14, asset cleanup; binary-self-removal out of scope).
- NEW `agent/report.rs` — engine typed returns (`AgentReport`/`AgentList`/`AgentLockOutcome`/`AgentEditOutcome`)
  wrapping `agent_env::report::*`.
- EDIT `crates/engine/src/event.rs` — `Event::Agent*` variants.
- EDIT `crates/engine/src/lib.rs` — `pub mod agent;` + 6 `impl Engine` methods.
- EDIT `crates/engine/Cargo.toml` — add `envctl-agent-env` path dep (only new dep; pure-Rust, no C).

## Engine API (signatures)
All `&self`, typed `Agent*Spec` opts (in `agent/mod.rs` so TASK-0014 builds them from clap/GUI
identically), `&EventSink`, return `anyhow::Result<T>`.
- `AgentSyncSpec { config_path: Option<String>, scope_override: Option<Scope>, apply: bool /*false=DRY-RUN*/, lock_mode: AgentLockMode }`
- `AgentLockMode { Plain, Update{only:Vec<String>}, Locked }`
- `AgentAddSpec { source, section(AddSectionSel: skills|mcps|commands +names), git_ref, branch, sub_dir, config_path, scope_override, apply, no_sync, no_verify, lock_mode }` (Locked requires no_sync — ported rule)
- `AgentRemoveSpec` (mirror of Add); `AgentLockSpec { config_path, scope_override, check:bool, upgrade_only:Vec<String> }`
- `AgentListSpec { scope_override, kind: ListKind(All|Skills|Mcps|Commands) }`; `AgentCleanSpec { scope_override, apply:bool }`
- `impl Engine { agent_sync→AgentReport; agent_add/agent_remove→AgentEditOutcome; agent_lock→AgentLockOutcome; agent_list→AgentList; agent_clean→AgentReport }`
- `apply:bool` field (not dry_run) so Default = preview. `config_path:None` → M-22 fallback
  (`config_path::default_config_path` + `load_config_recursive` → `Config::resolved_scope`).
- `agent_env::AgentEnvError` → `anyhow` via `?`. Engine NEVER `process::exit` — kasetto's exit(1) on
  failed>0 becomes `AgentReport{summary.failed>0}`; front-end maps to exit code.

## Event variants (event.rs)
`AgentRunStarted{verb:AgentVerb, scope, dry_run, lock_mode}`, `AgentAction{source, asset, status, error}`
(emitted per-asset as the driver processes — live tree for GUI/CLI), `AgentRunFinished{report:AgentReport}`,
`AgentLockChecked{drift:Vec<AgentLockDriftItem>}`. `AgentVerb{Sync,Add,Remove,Lock,List,Clean}`.

## Phase sequences (ported faithfully — see kasetto commands/*)
- **agent_sync (C-01..04)** ports `sync/mod.rs::run`: guard Locked+Update; `load_config_recursive`→cfg;
  `resolve_scope`; `resolve_destinations`; `scope_root`; if apply mkdir dests; build `AgentCtx`
  (dry_run=!apply); `lock::load`+`runtime::load`; phases **sync_skills→sync_commands→sync_mcps**
  (Locked never fetches; fetch via `materialize_source`+`select_targets`+`hash_dir`+`copy_dir`; MCP via
  `merge_mcp_config` never-clobber); **never-prune-on-failure** (`remove_stale` only when failed==0);
  if apply `lock::save`+`save_runtime`; assemble+emit `AgentRunFinished`; return.
- **agent_add (C-07)+C-12** ports `add.rs`: resolve config path (M-22); reject Locked&&!no_sync;
  `split_at_ref`+`derive_browse_url`; plan edits; `item_exists` guard; preview→emit planned actions+return;
  apply→optional verify_source→`insert_item`→write config→`!no_sync`→`self.agent_sync(apply=true)` in-process.
- **agent_remove (C-08, alias rm)** ports `remove.rs`: `config_edit::remove_item`/`remove_names`; preview
  returns RemoveOutcome actions; apply writes + optional sync_after (prunes orphans via remove_stale).
- **agent_lock (C-09)** ports `lock.rs` (distinct from component lock): re-resolve active sources
  (`upgrade_active` from upgrade_only); `check=true`→`prev.lock_check(&next)`→emit `AgentLockChecked`+return
  drift (no save); `check=false`→`lock::save`. `--check` honors `lock_mode=Locked` for zero-network audit.
- **agent_list (C-10)** ports `list.rs`: `load_skills_mcps_commands` (scope-merged when no override),
  filter by kind, return `AgentList`. Read-only.
- **agent_clean (C-11/C-14)** ports `clean.rs`: enumerate tracked skills+mcp+command asset ids; preview→
  counts only; apply→`apply_removals` (delete dirs; `remove_mcp_server` for TRACKED mcps only —
  pre-existing broker/repowire/weave never in agent lock → untouchable), `lock.clear_all`+save, clear runtime.

## Front-end parity (TASK-0014 = thin adapter)
clap/GUI builds `Agent*Spec`, calls one `Engine::agent_*`, drains EventSink to render tree/grid, maps
`report.summary.failed>0`→exit code. `--apply`→apply; `--locked`→Locked; `--update[names]`→Update;
`--check`/`--upgrade-package`→AgentLockSpec; `--scope`/`--config`/`rm`alias/section selectors→fields;
`--json`→front-end serializes the (already Serialize) return. No engine method needs a json flag.

## Tests (engine inline + `crates/engine/tests/agent_sync.rs`, local sources + tempdir, NO network, fixtures)
1. sync preview(no writes, would_install) vs apply(installs+lock). 2. **MCP never-clobber** (seed
broker/repowire/weave → after sync all 9 present). 3. lock --check drift (mutate content→drift; clean→empty).
4. **--locked zero-network** (unlocked source→locked_error+failed, no fetch; locked→unchanged). 5. remove+
sync-after prune. 6. clean preview vs apply (untracked MCP survives). 7. **M-22 fallback** (config_path:None→
scope from default-config file). 8. never-prune-on-failure (good+failing source→good assets kept).
CI gates: no-c (new agent-env path dep stays C-free/mimalloc-banned), shape (not tripped), fmt/clippy/test.

## Invariant checklist — ALL PASS
engine non-printing (Event-only, no println/clap; kasetto print_*/ui/exit dropped = FRONTEND-04 divergence) ·
front-end parity (6 typed methods) · fail-closed preview-default + Locked zero-network + never-prune-on-failure ·
MCP additive never-clobber (merge-only sync, lock-tracked-only clean) · no C (only pure-Rust agent-env path dep;
no-c+mimalloc-ban green; `cargo tree -p envctl-engine` clean) · one rustls ring-only · shape gate green ·
FNV-1a component lock untouched (agent lock = separate crate::agent::lock on agent-env.lock SHA-256) ·
conventions (snake_case/PascalCase/inline tests/area-prefixed commit) · no-downgrade (6 verbs + check/upgrade
+ init folded + dry-run/json/locked on every verb).

## Risks / implementer notes (none blocking)
1. **Drivers live in kasetto, not agent-env.** The `commands/*.rs` + `sync/{skills,commands,mcps}.rs`
   business logic is what this card ports. Per-helper decision: PURE asset logic (`needs_fetch`,
   `refresh_asset_revisions`, `load_skills_mcps_commands`, `desired_skill_names`, `verify_source`,
   `plan_edits`, `remove_by_kind`, `apply_removals`, `ensure_locked_satisfiable`) → port into a new
   `agent-env` helper module (keeps library complete, engine thin); Event-emitting ORCHESTRATION (per-verb
   `run` flow) → engine `agent/*.rs`. Follow the `agent_env::sync::remove_stale` precedent (library-side).
2. **agent_add self-sync** = call `self.agent_sync(apply=true, sink)` in-process (not a subprocess); watch
   the `&EventSink` borrow.
3. **lock --check network:** kasetto re-resolves (fetches) to diff; keep that, but honor `lock_mode=Locked`
   for a true zero-network audit.
4. **Do NOT** unify agent-env.lock into envctl.lock here — that's TASK-0017. Write a separate `agent-env.lock`.
