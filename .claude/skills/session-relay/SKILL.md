---
name: session-relay
description: "Hand off a long-running Feature Forge loop to a FRESH session before context rot / token burn degrade it, and resume from a handoff. ALWAYS use when: the forge-loop cycle budget is reached, the user says 'hand off', 'transfer the session', 'continue in a new session', 'pass the baton', or a session needs to 'resume from handoff'/'pick up the loop'. Coordinates the transfer over weave and schedules the successor with a durable local cron. Do NOT use for one-off features or normal in-session work."
---

# Session Relay (weave handoff + durable-cron successor)

You carry the Feature Forge loop across session boundaries with **zero loss**. A single session
degrades as context grows (rot) and gets expensive (token burn); the loop's defense is to run as a
chain of short sessions, each handing a durable checkpoint to the next. This skill is that handoff —
and the resume on the other side. It has two entry points: **HAND OFF** and **RESUME**.

## Substrates (verify before using)
- **Checkpoint:** `_workspace/HANDOFF.md`, produced by the `continuity-steward` agent — the cold-start
  resume package. This is the real payload; everything else just points at it.
- **weave** (`weave_whoami`/`weave_send`/`weave_inbox`/`weave_reply`) — the coordination + audit
  channel between sessions. Confirm identity with `weave_whoami` first. On this host the session
  identity is stable (e.g. `envctl`), so the *successor* reads the handoff from its weave **inbox**;
  the message is the durable, ordered signal that a transfer is pending. (`injectable=false` here →
  it arrives on the successor's next turn, which is exactly when it starts. That's fine.)
- **durable cron** (`CronCreate {durable: true}`) — schedules the successor run; persists to
  `.claude/scheduled_tasks.json` and survives restarts, so the loop continues on this machine even
  across a Claude restart. No cloud cost. (Note the 7-day auto-expiry on recurring jobs; the relay
  uses a **one-shot** resume, re-created each handoff, so expiry doesn't bite.)

---

## HAND OFF (current session, at cycle budget)

Run these in order; each step is durable before the next, so a crash mid-handoff is recoverable.

1. **Produce the checkpoint.** Spawn the `continuity-steward` agent (general-purpose, opus), passing
   the current UTC timestamp (you supply it — agents/scripts can't read the clock), the worktree
   path + branch, and the loop ledger. It writes `_workspace/HANDOFF.md` and returns
   `HANDOFF READY` / `HANDOFF INCOMPLETE`. If INCOMPLETE, fix the gap (or note it) before continuing —
   never hand off a checkpoint you know is wrong.
2. **Commit it.** `git add _workspace/ && git commit` (subject `loop: handoff checkpoint @ <ts>`).
   The successor resumes from committed state, so the checkpoint must be in git, not just on disk.
   Push if the loop runs against a shared remote.
3. **Announce over weave.** `weave_send` to the successor (the same session identity) **and** record
   it for observers:
   - `to`: the session identity from `weave_whoami` (the successor inherits it); optionally also a
     broadcast `to: "all"` so mesh peers can observe the relay.
   - `subject`: `forge-relay:handoff`
   - `body`: the `_workspace/HANDOFF.md` path, the worktree path + branch, the resume command, and
     the one-line backlog status (e.g. "4/7 done, resume at item 5"). Keep it pointer-sized; the
     detail lives in the checkpoint.
4. **Schedule the successor.** `CronCreate { durable: true, recurring: false }` with a near-future
   one-shot time and a `prompt` that re-enters the loop in RESUME mode, e.g.:
   `"/forge-loop resume from _workspace/HANDOFF.md (branch harness-loop) — read the handoff + weave
   inbox first"`. One-shot avoids the 7-day recurring expiry and double-runs; the next handoff
   creates the next one-shot.
5. **Stop this session's loop.** Do **not** issue another `ScheduleWakeup`. Report: handed off after
   N cycles, checkpoint committed, successor scheduled for <time>, backlog at X/Y.

> If the user chose **handoff-only** (no auto-spawn): do steps 1-3 and skip 4 — a human or an
> already-running peer picks it up from the weave announce + committed checkpoint.

## RESUME (successor session, on start / cron fire / weave nudge)

1. **Confirm identity & read the signal.** `weave_whoami`, then `weave_inbox` — find the
   `forge-relay:handoff` message; it points at `_workspace/HANDOFF.md`.
2. **Load the checkpoint.** Read `_workspace/HANDOFF.md` fully. Verify the worktree path + branch
   match and the tree is clean; run its **Verify-on-resume** commands to confirm a sane baseline
   (e.g. the 3 CI gates / a build) before mutating anything.
3. **Acknowledge.** `weave_reply` to the handoff message: `RESUMED @ <ts>, baseline verified,
   continuing at item <N>` (closes the coordination loop; gives observers a heartbeat).
4. **Reset the ledger.** In `_workspace/loop_state.md` set `cycles_this_session = 0` (the budget is
   per-session); keep `cycles_total`. 
5. **Continue the loop.** Re-enter `forge-loop`'s iteration body at the backlog's current item. The
   successor is now the active session and will itself hand off at the next budget.

---

## Error handling
- **Steward INCOMPLETE / contradictory state:** do not auto-spawn a successor onto a bad checkpoint.
  Commit the partial checkpoint with its gaps flagged, weave-announce as `forge-relay:handoff-degraded`,
  and stop for a human — a clean stop beats a confident-but-wrong resume.
- **weave unavailable** (`weave_doctor` fails): the committed `HANDOFF.md` + durable cron are the
  fallback path of record — the successor can resume from the file alone. Note the missing channel in
  the handoff body and proceed; weave is coordination, not the payload.
- **Duplicate/again resume:** if a successor finds `cycles_this_session` already reset and the top
  item in progress with fresh commits, another session may have resumed — re-check the inbox and the
  latest commit; if so, stand down rather than double-building.

## Test Scenarios
**Happy path:** forge-loop hits budget=3 → spawn continuity-steward → `HANDOFF.md` written + committed
(`loop: handoff checkpoint`) → `weave_send subject=forge-relay:handoff` with the pointer →
`CronCreate{durable:true,recurring:false}` one-shot resume in ~2 min → session stops. Cron fires: new
session reads inbox + HANDOFF, runs the 3 gates green, `weave_reply RESUMED`, resets the session
counter, continues at item 4.

**Error path:** steward returns `HANDOFF INCOMPLETE` (backlog says item 4 done, but no commit exists
for it). Relay does NOT schedule a successor; it commits the checkpoint with the contradiction
flagged, sends `forge-relay:handoff-degraded` to `all`, and stops for human review — preventing a
successor from re-doing or skipping item 4.
