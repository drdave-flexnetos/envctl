# Loop state — rust-port-MERGE (kasetto → envctl, Epic C absorption, VERIFY/MERGE mode)
session_started: 2026-06-13T17:23:35Z
loop: rust-port-merge
mode: verify-merge                                        # dest_repo set → classification-driven; reuse-Y units skip the fresh port, verify-only vs X
branch: task-0012-agent-env
worktree: /home/drdave/Desktop/meta/.worktrees/task-0012-agent-env/envctl
state_dir: .handoff/loop/rust-port/                       # NAMESPACED (avoids clobbering the forge-loop's flat .handoff/loop/{backlog,loop_state,HANDOFF}.md). Resumes read state HERE, not the flat path.
source_root: /home/drdave/Desktop/meta/kasetto            # X = pivoshenko/kasetto v3.2.0 (Cargo 3.2.0, ec01cca) — source-of-truth, itself Rust
source_toolchain: cargo                                   # verifier RUNS kasetto (cargo) to differentially diff against
dest_repo: envctl                                         # Y = where the merge lands (crates/agent-env now; crates/engine for runtime units already in envctl)
dest_worktree: /home/drdave/Desktop/meta/.worktrees/task-0012-agent-env/envctl
dest_branch: task-0012-agent-env
dest_base: develop
rust_target: crates/agent-env (package envctl-agent-env)  # + engine/cli wiring (TASK-0013/0014); reuse-Y units may already live in crates/engine (lock/runtime/doctor per CLAUDE.md)
cycle_budget: 3
cycles_this_session: 2
cycles_total: 7
ledger: merge 90 [~] merged · 12 [ ] to-merge (Engine C-* = TASK-0013) · 0 [x] verified · 13 [≠] front-end
last_item: fsops resolution + dirs/util (XC-03/04, F-03..F-10) — PR #75; prior: command transforms (PR-01) + config_edit (FE-*) + MCP merge (MC-01/02, PR #73 MERGED)
status: VERIFY-MERGE MODE — strong progress 2026-06-13/14. ALL 3 LEFT-BEHIND ENGINES NOW PORTED:
  MCP additive-never-clobber merge (MC-01/MC-02, 4 formats — PR #73 MERGED) + command-format transforms
  (PR-01, 5 formats) + comment-preserving config_edit (FE-*) + fsops resolution/dirs/util (PR #75).
  Tests 78→181; dual gate green every cycle (kasetto-verbatim tests + envctl Y not regressed + no-c).
  merge-ledger: 74 [~] / 28 [ ] / 0 [x] / 13 [≠].
  NEXT: (1) the **parity-verifier pass** — upgrade the 74 [~] → [x] (independent differential vs kasetto;
  a representative sample already matched in /verify: sha256 vs sha256sum, 4-host resolver, config, 21-agent).
  (2) the remaining 28 [ ] — mostly Engine-level command/sync BUSINESS LOGIC (C-* sync/add/remove/lock/
  list/clean) = **TASK-0013 engine wiring** (the agent-env LIBRARY surface is now ~complete) + small
  leaves (M-22 resolve_scope fallback, S-18 discover_mcps). (3) DONE gate: 100% [x]/[≠] + left-behind
  sweep clean + Y green. Engine wiring = TASK-0013, CLI verbs = TASK-0014.
last_update: 2026-06-14T00:40:00Z

## Run framing (read before any cycle)
- This rust-port runs as the IMPLEMENTATION ENGINE for forge-loop **TASK-0012** (Epic C: kasetto
  full-feature unification into envctl, no downgrade). Its parity ledger extends across TASK-0012..0018.
- **Source is itself Rust** (kasetto is a Rust CLI). So this is an ABSORPTION port (kasetto crate →
  envctl library+engine), not a cross-language translation. "Parity" = the envctl agent-env surface
  reproduces kasetto v3.2.0 BEHAVIOR (config parse, extends merge, resolver, hash, lock modes, the
  11 verbs' logic, MCP merge) — verified by porting kasetto's own tests + differential fixtures.
- **Scope boundary (engine-first invariant):** absorb LOGIC (src/{model,source,fsops,lock,commands
  logic,mcps}) into the library/engine. Kasetto's terminal PRESENTATION (src/{ui,banner,colors}.rs)
  and its clap wiring (src/cli.rs, app.rs, main.rs, update_notifier.rs) are NOT ported verbatim —
  they map to envctl's OWN CLI/GUI front-ends (TASK-0014), so their ledger rows are behavioral
  (verb semantics) not pixel-rendering. The cartographer marks these `- [≠] front-end: envctl owns rendering`.
- **Seed credited:** commit 6ecb270 already ports config/extend/source/hash/lock (foundational) with
  61 unit tests + no_downgrade integration test GREEN. The cartographer credits these as `- [~]`
  (ported, parity-unproven) — the parity-verifier upgrades to `- [x]` after differential proof vs kasetto.

## Gates (non-negotiable, every cycle)
no C in trust boundary (no SQLite/OpenSSL/aws-lc; mimalloc DROPPED; flate2 rust_backend) ·
one rustls ring-only · ci/gates/no-c.sh green · non-printing library · forbid(unsafe_code) ·
never weaken the parity gate / never stub to pass (the cardinal rule).
