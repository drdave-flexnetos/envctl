# Verification report: TASK-0013 — Engine wiring for the agent-env subsystem (Epic C)

Guardian: invariant-guardian · 2026-06-13 · branch `task-0013-engine-agent`
Worktree: `/home/drdave/Desktop/meta/.worktrees/task-0012-agent-env/envctl`

## Verdict — PASS

The change wires kasetto's 6 agent-asset verbs into the engine as `Engine::agent_{sync,add,
remove,lock,list,clean}` over the ported `crates/agent-env` library, engine-first and
non-printing. Every NON-NEGOTIABLE invariant holds under real gate + test + code-inspection
evidence. No blocking findings.

## Gate results
| Gate | Exit | Decisive line |
|------|------|---------------|
| `ci/gates/no-c.sh` | 0 | `resolved graph clean: rustls=['0.23.40'] on ring=['0.17.14']; zero aws-lc/openssl/C-SQLite` → `NO-C GATE PASS` |
| `ci/gates/shape.sh` | 0 | `SHAPE GATE PASS` |
| `ci/gates/enable.sh` | 0 | `ENABLE GATE PASS` |
| no-c cross-check | n/a | `cargo tree -p envctl-engine | grep -Ei 'mimalloc|libmimalloc|libsqlite3|openssl|aws-lc'` → EMPTY (grep exit 1). The engine's new `envctl-agent-env` path dep pulled NO banned C crate and NO mimalloc. |

## cargo
| Check | Exit | Result |
|-------|------|--------|
| `cargo build -p envctl-engine -p envctl` | 0 | Finished, clean |
| `cargo test --workspace` (raw, `--no-fail-fast`) | 0 | **31 suites, 498 passed, 9 ignored, 0 failed**, zero `FAILED` lines (the 9 ignored are the pre-existing libsql-remote tests that need a running sqld) |
| `cargo test -p envctl-engine --test agent_sync` | 0 | **8/8 agent integration tests pass** (named below) |
| `cargo clippy --workspace -- -D warnings` | 0 | Finished, no warnings emitted |
| `cargo fmt --all -- --check` | 0 | Clean |

The 8 agent tests (raw cargo output): `sync_preview_writes_nothing_then_apply_installs`,
`mcp_sync_is_additive_never_clobbers_existing_servers`, `lock_check_reports_drift_then_clean`,
`locked_mode_fails_closed_without_lock_then_passes_when_locked`,
`remove_then_sync_after_prunes_skill`, `clean_preview_keeps_then_apply_removes_tracked_only`,
`m22_fallback_resolves_default_config_from_cwd`, `never_prune_when_a_source_fails` — all ok.

## Invariant checks
1. **No C in trust boundary** — PASS. `no-c.sh` exit 0; engine `cargo tree` banned-crate grep empty;
   rustls 0.23.40 single version on ring 0.17.14. The only new dep is the pure-Rust `envctl-agent-env`
   path dep (`crates/engine/Cargo.toml`).
2. **No mimalloc** — PASS. `cargo tree -p envctl-engine` grep for `mimalloc|libmimalloc` empty;
   no-c gate's mimalloc arm green.
3. **Engine non-printing** — PASS. `grep -rn 'println!|eprintln!|print!' crates/engine/src/agent/`
   is EMPTY (exit 1). The 2 `process::exit` hits and the `crate::lock`/`clap` hits are all in
   doc-comments (`///`/`//!`); the non-comment grep is empty (exit 1). Methods emit `Event`s + return typed data.
4. **No `process::exit` in engine** — PASS. `grep 'process::exit' crates/engine/src/agent/
   crates/agent-env/src/driver.rs` → only 2 doc-comment occurrences (report.rs:29, sync.rs:23);
   zero code. Engine maps `failed>0` → caller exit code instead (asserted by test 4 + 8).
5. **FNV-1a component lock untouched** — PASS. `crates/engine/src/lock.rs` is NOT in the diff
   (`git diff` empty for it). `agent/lock.rs` imports `envctl_agent_env::lock` only; `grep
   'crate::lock' crates/engine/src/agent/` → only 2 doc-comments, zero code. The agent lock is the
   separate SHA-256 `agent-env.lock`; the two locks share no code.
6. **Fail-closed + preview default** — PASS. `AgentSyncSpec::default()` sets `apply: false`
   (mod.rs:131). Test 1 asserts preview leaves BOTH `.claude/skills/alpha` absent AND `agent-env.lock`
   absent (real zero-write check), then apply asserts both exist. The `apply` gate guards mkdir + save
   + runtime in `sync.rs::run_sync_in_ctx`.
7. **`--locked` zero-network fail-closed** — PASS. Test 4 asserts: unlocked source under Locked →
   `failed>0` AND `installed==0` AND an action with `status == "locked_error"` AND
   `!.claude/skills/alpha.exists()` (no fetch). After a plain lock+install, Locked is satisfied
   (`failed==0`, `unchanged>=2`). `agent_lock --check`+Locked (`lock.rs:44` `zero_network`) skips
   `rebuild_lock` entirely → diffs prev against itself, truly no network.
8. **MCP additive never-clobber** — PASS. Test 2 seeds broker/repowire/weave into `.mcp.json`,
   runs apply-sync, re-reads the file, asserts all 5 keys present (3 pre-existing + 2 merged: github,
   context7). Clean (test 6) removes only lock-tracked MCP ids — untracked `weave` survives, tracked
   `github` removed.
9. **Never-prune-on-failure** — PASS. Test 8: a sibling source that errors at materialize
   (`sub-dir: no-such-subdir`) bumps `failed>0`; asserts `removed==0` and the good locked `alpha`
   SKILL.md survives. Driver gates `remove_stale_*` on `summary.failed==0`.
10. **M-22 fallback** — PASS. Test 7: `config_path: None` from cwd resolves the local `agent-env.yaml`
    via `default_config_path`; asserts `scope == Project` and `installed>=2`.
11. **Rust-native / one rustls ring-only / conventions** — PASS. Only new dep is a workspace path
    crate; snake_case modules, PascalCase types, inline + integration tests; fmt/clippy clean.

## Parity check (Engine method → front-end readiness for TASK-0014)
Exactly **6** `pub fn agent_*` on `impl Engine`, all `&self`, all `anyhow::Result<T>`:
- `agent_sync`  → `crates/engine/src/agent/sync.rs:24`  → returns `AgentReport`
- `agent_add`   → `crates/engine/src/agent/edit.rs:29`  → returns `AgentEditOutcome`
- `agent_remove`→ `crates/engine/src/agent/edit.rs:139` → returns `AgentEditOutcome`
- `agent_lock`  → `crates/engine/src/agent/lock.rs:26`  → returns `AgentLockOutcome`
- `agent_list`  → `crates/engine/src/agent/list.rs:18`  → returns `AgentList`
- `agent_clean` → `crates/engine/src/agent/clean.rs:24` → returns `AgentReport`

All return types derive `Serialize + Deserialize` (`agent/report.rs` lines 30/50/59/70/83/99), so
TASK-0014 `--json` is a thin serialize and clap/GUI both build the typed `Agent*Spec`. No clap in the
engine (only a comment at mod.rs:112). 4 Event variants present (`AgentRunStarted`/`AgentAction`/
`AgentRunFinished`/`AgentLockChecked`, event.rs:61/69/76/80). This is a CLI+GUI-shared surface, not a
single front-end — parity-ready.

## Findings
- **NOTE (non-blocking, expectation drift):** the task brief said the MCP never-clobber test asserts
  "9 servers"; the committed fixture pack (`tests/fixtures/agent/pack/mcps/servers.json`) ships only
  github + context7, so test 2 asserts **5** servers present (3 seeded + 2 merged). The *substance*
  the brief required — pre-existing broker/repowire/weave survive AND the new servers are added — is
  proven exactly. The "9" was a stale brief number, not a test defect.
- **NOTE (out of scope, not part of this change):** untracked `crates/agent-env/tests/parity_vs_kasetto.rs`
  is a TASK-0012 artifact, not TASK-0013; excluded from this verdict (it does not affect any gate).
- **NOTE (process):** the TASK-0013 work is currently uncommitted in the worktree (HEAD is the
  TASK-0012 merge #76). I verified the worktree state as delivered; the orchestrator commits on PASS.
- **TOOLING NOTE:** `rtk proxy cargo test --workspace` summarized/truncated its captured output
  (56-line file, partial suite list). I did NOT read that as clean — I re-ran the workspace tests with
  raw `cargo test --workspace --no-fail-fast` (bypassing the rtk hook) to get the authoritative
  31-suite / 498-passed / 0-failed tally above. Fail-closed honored.

## Re-test needed
None — PASS. If the orchestrator wants the brief's "9-server" assertion literally, extend the fixture
pack with 3 more MCP entries (optional cosmetic; the invariant is already proven). To reconfirm after
commit:
```
bash ci/gates/no-c.sh && bash ci/gates/shape.sh && bash ci/gates/enable.sh
cargo test -p envctl-engine --test agent_sync
cargo test --workspace --no-fail-fast
cargo clippy --workspace -- -D warnings && cargo fmt --all -- --check
```
