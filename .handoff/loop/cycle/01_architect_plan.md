# TASK-0014 architect plan — envctl agent CLI group (CLI-only; GUI→TASK-0014b)

VERDICT: GO. Thin adapter over existing Engine::agent_* (no engine logic change).
CLI-only this cycle; GUI deferred (TASK-0014b) — engine parity guarantees zero churn.

## Surface: add Cmd::Agent { #[command(subcommand)] cmd: AgentCmd } in crates/cli/src/main.rs
AgentCmd: Sync/Add/Remove/Lock/List/Clean → Engine::agent_{sync,add,remove,lock,list,clean}.
--json is GLOBAL (Cli.json) — inherit, no per-verb flag. apply: bool defaults FALSE (fail-closed).
ScopeArg{Global,Project}→AgentScope; ListKindArg{All,Skills,Mcps,Commands}→AgentListKind.
lock_mode_from(locked,update): --locked→Locked; --update[names]→Update{only}; else Plain.

Field maps (specs at engine/src/agent/mod.rs:115-196):
- sync: config→config_path, scope→scope_override, apply→apply, locked/update→lock_mode
- add: source(pos)→source; skill/mcp/command→AgentSectionSel; ref/branch/sub_dir/config/scope/apply/no_sync/no_verify/locked
- remove: same as add minus no_verify
- lock: config/scope/check/upgrade_package→upgrade_only/locked→lock_mode
- list: scope/kind only
- clean: scope/apply only

## Render/exit: run_agent(engine, AgentCmd, json) modeled on run_action (main.rs:540-655)
Worker thread calls Engine method w/ channel sink; drain rx.iter(). Extend print_event
(main.rs:724) with 4 Event::Agent* arms (currently fall through _ => {}). --json = pretty
serialized RETURN value (matches auto-detect/graph/lock), uniform across all 6.
Exit: sync/clean → report.summary.failed>0 ⇒ exit(1); add/remove → outcome.sync.failed>0 ⇒ 1;
lock --check → non-empty drift ⇒ 1; list → 0. Engine bail!s propagate → exit 1.

## Files: crates/cli/src/main.rs (enums+AgentCmd+run_agent+print_event arms+dispatch at ~main.rs:219);
maybe a tiny engine:lib.rs pub use only if a return-field isn't reachable (low prob).
Tests: NEW crates/cli/tests/agent.rs (model tests/dashboard.rs): per-verb dry-run zero-writes;
--json shape (list, lock --check); exit codes (lock --check drift→1, list→0); flag-conflict
(add --ref --branch → nonzero). Unit test lock_mode_from + ScopeArg conversion.

## Invariants: engine non-printing single-lib (CLI thin); fail-closed apply=false default;
--json+exit contract; no new dep (no-c PASS); one rustls ring-only.
Open Q: clean has no --confirm in spec (preview-default is the guard; don't invent CLI confirm).
