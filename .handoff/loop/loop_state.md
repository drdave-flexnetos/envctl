# Loop state — env-ownership + Phase-2 tool relocation

session_started: 2026-06-12
loop: runaway-containment Phase-2 (env-ownership build-out -> tool relocation)
branch: fix/dashboard-auto-claude-opt-in   # envctl working branch carrying the mission commits
worktree: /home/drdave/Desktop/meta/envctl  (main checkout)
status: ACTIVE — backlog seeded, not yet started
mission: runaway-session containment (2026-06-12). See context/capsule.json next_command.

## Done before this loop (shipped + verified)
- ICM recursion root cause fixed + live: detect_provider clamps Claude->None inside Claude Code;
  spawn-site guard; extract-pending flock. icm 9da001d (main), 232 tests. Proven live.
- SessionEnd hook re-enabled safely (envctl 757707c); tool-hooks de-hardcoded to bare names
  via PATH (envctl 2bf6a28). Settings source-of-truth = envctl/home/.claude (mirrors verified).
- .handoff repaired: real envctl capsule, _workspace migrated here (P7/ADR-0004), envctl d131148.

## Next safe step
- Phase 0 of backlog.md: build `envctl env` (export META_ROOT from .meta.yaml marker). This
  unblocks healing the 3 hardcoded settings refs and the per-machine symlink regeneration that
  the relocation depends on.

## Order
Phase 0 (env-ownership) -> meta-mcp (proof) -> kasetto (after source-sync to 3.1.0) ->
rtk SUPERVISED last (hook-critical + downgrade). git-kb/forge are NOT targets (external/vendor).

## Gates (non-negotiable)
- never-downgrade (sync meta source UP first) · archive-first (never delete) · build+verify
  before swap · rollback on failure · verify env health each slice · rtk = supervised only.

## needs_human / supervised
- rtk + rtk-monitor relocation (owner-flagged critical; on the live rtk hook path).
- Decision: bring GitKB into meta as a `.meta.yaml` project (git-kb is currently external)?
- Old loop's remaining item (dashboard GUI smoke-test) — see _done/, HUMAN-ONLY.

last_update: 2026-06-12
