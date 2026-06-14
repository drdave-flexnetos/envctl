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
cycles_this_session: 1
cycles_total: 1
ledger: parity 0/112 verified · 55 ported [~] · 44 todo [ ] · 13 front-end [≠]  (merge-ledger seeded from this — see merge-ledger.md)
last_item: model/* completion (M-09 finish, M-11..M-20, M-25, M-26, M-27) — ported [~], build-health GREEN
status: VERIFY-MERGE MODE engaged 2026-06-13 (owner: "rerun the kasetto integration via rust-port-merge;
  full feature, nothing left behind"). Harness EJECTED into envctl/.claude (12 skills + 10 agents; FF
  continuity-steward preserved). NEXT: (1) researcher builds reports/research.md reuse map (kasetto unit
  ⟷ what envctl ALREADY provides — CLAUDE.md notes engine already has lock/runtime/doctor/lock--check →
  expect reuse-Y/extend-Y units, NOT all port-fresh) + classify every unit in merge-ledger.md; (2) the
  DUAL GATE per unit: differentially verify vs kasetto X AND assert envctl Y not regressed; (3) left-behind
  sweep (every kasetto unit represented). The 55 [~] re-verify vs kasetto → [x]; the 44 [ ] split into
  reuse-Y (verify envctl's existing symbol) / port-fresh / extend-Y. DONE only at 100% merged + verified.
last_update: 2026-06-13T18:05:00Z

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
