# Loop state — rust-port (kasetto → envctl agent-env, Epic C absorption)
session_started: 2026-06-13T17:23:35Z
loop: rust-port
branch: task-0012-agent-env
worktree: /home/drdave/Desktop/meta/.worktrees/task-0012-agent-env/envctl
source_root: /home/drdave/Desktop/meta/kasetto            # pivoshenko/kasetto v3.2.0 (Cargo 3.2.0, ec01cca)
source_toolchain: rust                                    # source is itself Rust (kasetto) → parity = behavior, not language translation
rust_target: crates/agent-env (package envctl-agent-env)  # + later engine/cli wiring (TASK-0013/0014)
cycle_budget: 3
cycles_this_session: 1
cycles_total: 1
ledger: parity 0/112 verified · 55 ported [~] · 44 todo [ ] · 13 front-end [≠]
last_item: model/* completion (M-09 finish, M-11..M-20, M-25, M-26, M-27) — ported [~], build-health GREEN
status: DISCOVER complete + 1 port cycle done. Seed (6ecb270) + model/* port committed. NEXT (resume):
  run the PARITY-VERIFIER pass to upgrade the 55 [~] (foundational config/extend/source/hash/lock +
  model/*) to [x] via differential test vs kasetto v3.2.0, THEN continue Phase-2 ITERATE on the 44 [ ]
  (top dep-ready: XC-03 dirs, XC-04 util, then fsops F-03..F-10, config_edit FE-*, MCP merge, commands).
last_update: 2026-06-13T17:40:00Z

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
