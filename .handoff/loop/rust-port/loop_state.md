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
cycles_this_session: 3   # SESSION-2 BUDGET 3/3 REACHED → HAND OFF. cycle1=leaves, cycle2=C-* sync engine, cycle3=C-* verbs
cycles_total: 13
parity: 93 [x] verified · 7 [~] residue/gap · 2 [ ] pending (M-22,S-15) · 13 [≠] front-end (parity-ledger.md authoritative; DONE = all [x]/[≠]; DONE-equiv 106/115)
ledger: merge 102 [~] merged · 0 [ ] to-merge · 13 [≠] front-end (ABSORPTION COMPLETE THROUGH ENGINE; remaining work = parity-verifier pass + 1 no-downgrade fix)
last_item: PARITY cluster C-* VERBS — +7 [x] (C-07/08/09/10/11/13/14) via crates/engine/tests/agent_command_parity.rs (+22 tests, engine 74→96); C-12 [~] (remote-config-reject GAP = C-12-FIX, a real no-downgrade engine fix)
session2_summary: SESSION-2 successor 2026-06-14, budget 3/3. Landed session-1 stack (#80-#82 merged).
  Cycle1 leaves +6 [x] (PR #83). Cycle2 C-* sync engine +6 [x] (PR #84). Cycle3 C-* verbs +7 [x] (PR #85
  pending). parity 80→93 [x] (DONE-equiv 106/115). Engine integration tests: agent_sync_parity.rs (15) +
  agent_command_parity.rs (22). **REMAINING TO FULL DONE (9 rows, all network/engine residue):** 2 [ ]
  = M-22 (resolve_scope file-read fallback, engine path) + S-15 (main→master retry, network); 7 [~] =
  S-07/S-12/S-13 (pub(crate)/network), CFG-03 (remote arm), C-12 (engine remote-reject GAP). PLUS the 13
  [≠] front-end = TASK-0014 (CLI/GUI verbs). **NEXT SESSION:** (a) ONE Engine/network integration cycle
  exercising materialize/download end-to-end → closes S-07/S-15/CFG-03 + M-22; (b) C-12-FIX engine src
  change (resolve_local_config_path → Result, reject remote) + S-12/S-13 pub-seam → closes the rest;
  (c) TASK-0014 front-end. PR-STACK: #80-#82 merged; #83→#84→#85 stacked linear — rebase each onto develop
  as its parent merges (git rebase --onto origin/develop <parent-tip> <branch>; clean so far).
session2_note: SESSION-2 successor 2026-06-14. Landed session-1 PR-stack (#80/#81 merged; #82 rebased onto
  fresh develop 870387f, auto-merge armed). Then cycle 1 = leaves cluster (+6 [x], 311 tests, PR pending,
  STACKED on #82 branch task-0012-parity-pass-3). Remaining 16 [ ]: M-22 (resolve_scope fallback) + S-15
  (main→master retry) = engine/network → fold into the Engine-integration cycle; C-01..C-14 = command
  orchestrators (sync/add/remove/lock/list/clean/init) — verify via crates/engine/tests differential vs
  kasetto command behavior; that SAME engine-integration work also closes the 6 [~] residue
  (S-07/12/13/15,CFG-03,M-24/L-03). NEXT cycle = C-* sync engine (C-01..C-06). THEN TASK-0014 (13 [≠] CLI/GUI).
status: VERIFY-MERGE MODE — resume 2026-06-14. PARITY-VERIFIER PASS, budget 3/3 reached → HAND OFF.
  Absorption structurally complete through the Engine (0 to-merge); remaining DONE work = drive the
  [~]/[ ] parity rows → [x] via verbatim kasetto golden vectors. Session cycles (all PASS, all merged or
  PR-armed): #1 source-resolver (+11 [x], PR #80 MERGED), #2 model + 21-preset table + config-loader +
  SHA-256 lock (+27 [x], PR #81 auto-merge armed), #3 fsops + config_edit (+14 [x], 304 tests, PR pending).
  parity now 74 [x]/6 [~]/22 [ ]/13 [≠] — DONE-equivalent 87/115. 6 [~] residue: S-07/12/13/15
  (pub(crate)/network), CFG-03 (remote arm), M-24/L-03 design-fold — close all via ONE Engine::agent_sync
  integration cycle (exercises materialize/download/sync end-to-end). NOT by faking.
  NEXT clusters (22 [ ] remain): **C-* command business logic** (sync/add/remove/lock/list/clean — the big
  one; exercise via Engine integration tests which ALSO closes the 6 [~] residue), XC-01/02/03
  (error/http/dirs), CP-*/ST-*/P-* leaves. THEN TASK-0014 (the 13 [≠] front-end CLI/GUI verbs).
  PR-STACK NOTE: #80 merged; cycle-2 (#81) rebased onto fresh develop dropping the merged cycle-1 commit;
  cycle-3 stacked on cycle-2 branch — after #81 merges, rebase cycle-3 PR onto develop dropping #81's commit.
  PRIOR NEXT (kept): M-09..M-14/M-17 (21-preset path table), CFG-01..03 (recursive extends loader),
  L-01..06 (SHA-256 asset lock), F-03..F-10+FE-* (fsops/config_edit), C-* command business logic.
  PRIOR (kept for history): strong progress 2026-06-13/14. ALL 3 LEFT-BEHIND ENGINES NOW PORTED:
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
