# Loop state — envctl agenticOS consolidation (Epics A–E)

# --- forge-loop ledger (schema fields the loop reads in Phase-0) ---
session_started: 2026-06-13
loop: agenticOS-consolidation (.handoff/loop/backlog.md, Epics A–E; design = .handoff/decisions/ADR-0001)
branch: develop   # work happens in FRESH worktrees off develop -> PR -> auto-promote to master
worktree: (per-cycle: meta/.worktrees/<slug>/envctl off develop)
cycle_budget: 3
cycles_this_session: 0
cycles_total: 0
last_item: (none yet — backlog reconciled, loop not yet started this session)
status: HANDED-OFF 2026-06-13 — checkpoint at .handoff/loop/HANDOFF.md; successor resumes via
  forge-loop at TASK-0001 (build hf) in a fresh worktree off develop. cycles_this_session resets to 0 on resume.

## Next safe step
- Epic A TASK-0001 (P0): build & install the `hf` kernel from `meta/handoff` (markdown-fallback pick;
  hf not yet on PATH). Run via the `handoff-kernel-engineer` agent + `handoff-sync` skill. This
  unlocks hf-rendered packets + witnessed checkpoints for every later cycle, and TASK-0002 (mint
  cards) which then becomes the ordering authority.

## Order (dependency-aware; cards own ordering once TASK-0002 mints them)
Epic A: TASK-0001 (build hf) -> TASK-0002 (seed Tier-A + mint cards) -> TASK-0003 (p7 gate).
Epic C: TASK-0012 (crates/agent-env) gates TASK-0013..0018.
Epic B: TASK-0005 healed (settings tmpl on develop); TASK-0008 meta-mcp (proof) before others.
SUPERVISED (never auto-run): TASK-0010 was `- [!!]` (now DONE by a human session — see backlog).

## Gates (non-negotiable)
- never-downgrade (sync meta source UP first) · archive-first (never delete) · build+verify before
  swap · rollback on failure · ledger-residency ($META_ROOT only, no per-repo ledger.db) ·
  packets-rendered-never-hand-written · `- [!!]` items refuse auto-run -> NEEDS-HUMAN.

## needs_human / supervised
- Decision: bring GitKB into meta as a `.meta.yaml` project (git-kb currently external)?
- Old dashboard-forge-loop GUI smoke-test (loop/_done/, HUMAN-ONLY).

last_update: 2026-06-13
