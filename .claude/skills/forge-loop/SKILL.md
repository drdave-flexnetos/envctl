---
name: forge-loop
description: "Run the envctl Feature Forge crew CONTINUOUSLY over a backlog — the Ralph loop. ALWAYS use when asked to: work through a backlog/list of features autonomously, 'keep building', 'loop on the roadmap', run Feature Forge 'until done'/'on repeat'/'unattended', or 'resume the loop' from a handoff. Each iteration does the next undone backlog item via the full architect→implementer→guardian cycle, checkpoints, and self-paces. At the per-session cycle budget it triggers session-relay to hand off to a fresh session. Do NOT use for a single one-off feature (use feature-forge directly) or for environment/install tasks."
---

# Feature Forge Loop (Ralph)

You run the Feature Forge crew as a **self-perpetuating loop** over a durable backlog, instead of
one feature at a time. The design is deliberately simple — the *Ralph* pattern: durable state on
disk, each iteration reads it, does the next undone thing, writes the result back, and re-fires.
The loop's intelligence lives in the **backlog file and checkpoints**, not in conversation memory —
that is exactly what lets a fresh session pick the loop up with zero loss (see `session-relay`).

## Why this shape
Conversation context rots and token cost climbs the longer a single session runs. A loop that
keeps all its truth in durable files (backlog + checkpoints) can be carried across many short, cheap
sessions instead of one long, expensive, degrading one. So: **never hold loop state only in your
head — write it down every iteration.**

## Durable state (the loop's memory)
All under the worktree's `_workspace/` (the audit trail; preserve it):
- **`_workspace/backlog.md`** — the source of truth. An ordered checklist of work items. Each item:
  `- [ ] <id>: <one-line goal>` (→ `- [x]` when its cycle PASSES). Sub-notes indented beneath.
- **`_workspace/loop_state.md`** — the ledger: `cycles_this_session`, `cycles_total`,
  `cycle_budget`, `session_started` (UTC, passed in — never call Date.now), `last_item`, `status`.
- **Per-cycle artifacts** — `01_architect_plan.md` / `02_implementer_log.md` / `03_guardian_report.md`
  for the item currently in flight (same as a single feature-forge run).

If `_workspace/backlog.md` does not exist, create it first from the user's request (a roadmap, a
doc, or an explicit list), then start the loop. Keep items small and independent — one Engine
capability or one component per item — so a cycle fits comfortably under the budget.

## Parallel mode (opt-in grit git-lock coordination)

When looping over items that span multiple meta repos, activate with `USE_GRIT=1`:

1. Before the first implementer: `for repo in $(meta list-projects --names); do cd /home/drdave/Desktop/meta/$repo && grit init -y; done` (idempotent).
2. Each implementer claims symbols via `grit claim file::symbol --with-deps` before writing, `grit done` after commit.
3. Contested symbols auto-queue (`grit claim --queue`).

Parallel mode is **opt-in** — the default single-implementer path is unchanged. See `feature-forge/SKILL.md` for full details on the parallel protocol (claim→work→done, `--queue`, `--with-deps`, CLI-only constraints).

## Self-pacing (how the loop re-fires)
- Default: **dynamic /loop** — use `ScheduleWakeup` to re-enter this skill for the next iteration,
  passing the same `/forge-loop …` prompt verbatim so the next firing repeats the body. Pick the
  delay by what you're waiting on; for back-to-back build iterations a short warm-cache delay
  (≤270s) is fine. When you HAND OFF or finish, **omit** the ScheduleWakeup call to end the loop.
- Alternative: a fixed interval (`/loop <interval> /forge-loop …`) when the user wants paced runs.
- A cycle counts only when a Feature Forge cycle **completes** (PASS/PASS-WITH-NOTES/blocked) — a
  re-fire that does no work (e.g. waiting) does not increment the ledger.

## Cycle budget (the handoff trigger)
The per-session budget is **cycles-only** (no token-meter guessing): default **3** completed cycles
per session unless the user sets another (`/forge-loop budget=N …`). Record it in `loop_state.md`.
When `cycles_this_session` reaches it, you do **not** start another cycle — you invoke
`session-relay`, which checkpoints + announces + schedules the successor, then you stop. The
successor resets `cycles_this_session` to 0 and continues where the backlog left off. This keeps
every session short, cheap, and well below context rot — by construction, not by measurement.

## Resume (entering mid-loop from a handoff)
If invoked to **resume** (a `_workspace/HANDOFF.md` exists, or weave inbox / the successor cron
prompt says so): follow `session-relay`'s resume protocol first (read HANDOFF + ack via weave +
reset `cycles_this_session`), then run the iteration body normally from the backlog's current item.

## Stop conditions (end the loop — no re-fire)
- Backlog complete (all items `- [x]`/`- [!]`) → DONE summary.
- Cycle budget reached → hand off (session-relay), then stop.
- A hard blocker the loop can't route around (e.g. dirty/ambiguous worktree, repeated guardian
  FAIL on the same item) → stop and report; don't burn cycles spinning.
- The user interrupts.

## Test Scenarios
**Happy path:** `/forge-loop budget=3` with a 7-item backlog. Iterations 1-3 each complete a feature
(architect→implementer→guardian PASS, committed), ticking items and incrementing the ledger. After
cycle 3, the stop check trips the budget → `session-relay` writes HANDOFF, weave-announces, schedules
a durable-cron successor, and this session stops. The successor fires, resets the session counter,
and continues at item 4.

**Error path:** Iteration 2's item needs a banned C dep (guardian FAIL the architect can't route
around). The loop marks item `- [!] blocked: needs C SQLite — out of bounds`, commits the backlog
update, and proceeds to item 3 rather than thrashing. The blocked item surfaces in the DONE/HANDOFF
summary for a human decision.
