---
name: session-relay
description: "Hand off a long-running harness loop (the feature loop `forge-loop` OR the environment loop `env-install-loop`) to a FRESH session before context rot / token burn degrade it, and resume from a handoff. ALWAYS use when: a loop's cycle budget is reached, the user says 'hand off', 'transfer the session', 'continue in a new session', 'pass the baton', or a session needs to 'resume from handoff'/'pick up the loop'/'resume the env install'. Coordinates the transfer over weave (cross-identity heartbeat) and schedules a best-effort successor cron — the committed checkpoint is the real resume signal. Do NOT use for one-off features or normal in-session work."
---

# Session Relay (weave handoff + cron successor)

You carry a harness loop (feature or environment) across session boundaries with **zero loss**. A single session
degrades as context grows (rot) and gets expensive (token burn); the loop's defense is to run as a
chain of short sessions, each handing a durable checkpoint to the next. This skill is that handoff —
and the resume on the other side. It has two entry points: **HAND OFF** and **RESUME**.

## Substrates (verify before using)
- **Checkpoint:** `_workspace/HANDOFF.md`, produced by the `continuity-steward` agent — the cold-start
  resume package. This is the real payload; everything else just points at it.
- **weave** (`weave_whoami`/`weave_send`/`weave_inbox`/`weave_reply`) — the **cross-identity**
  coordination + audit channel. **Smoke-test finding:** a message addressed to your *own* identity
  does **not** appear in your own inbox — and the same-machine successor inherits the same identity
  (e.g. `envctl`), so weave-inbox is **not** the resume signal for a same-identity handoff. Use weave
  for what it actually does well: a **broadcast `to: "all"`** (or to a distinct peer / a human) as an
  *observable heartbeat* (`forge-relay:handoff` / `forge-relay:resumed`) so the mesh and operators can
  watch the relay. The **actual resume signal is the committed checkpoint + the cron prompt** (below).
- **cron** (`CronCreate {recurring: false}`) — schedules the successor run on this machine when the
  REPL is next idle, and the prompt itself carries the resume instruction. **Smoke-test finding:**
  `durable: true` is **not honored in this runtime** — jobs report `[session-only]` and nothing is
  written to `.claude/scheduled_tasks.json`, so a cron successor does **not** survive a Claude
  restart; it only fires while this session is alive. Treat cron as **best-effort**. For genuinely
  out-of-process / survives-restart continuation, the durable signal is the **committed
  `_workspace/HANDOFF.md`** (any new session resumes from it) — optionally escalate to `RemoteTrigger`
  (claude.ai routine) when an unattended cloud successor is wanted.

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
3. **Broadcast the heartbeat over weave.** `weave_send` `to: "all"` (cross-identity observers — do
   **not** address your own identity; it won't reach the successor's inbox):
   - `subject`: `forge-relay:handoff`
   - `body`: the `_workspace/HANDOFF.md` path, worktree path + branch, the resume command, and the
     one-line backlog status (e.g. "4/7 done, resume at item 5"). Pointer-sized; detail lives in the
     committed checkpoint. This is observation/audit, not the resume signal.
4. **Schedule the successor.** `CronCreate { recurring: false }` with a near-future one-shot time and
   a `prompt` that re-enters the loop in RESUME mode **and self-describes the resume** (the prompt is
   the real signal — don't rely on inbox), e.g.:
   `"/forge-loop resume from _workspace/HANDOFF.md (branch <branch>, worktree <path>) — read the
   committed handoff checkpoint, verify baseline, then continue at the backlog's next item"`.
   One-shot avoids double-runs; the next handoff creates the next one-shot. **Caveat (verified):**
   `durable` is not honored here — this fires only while the session stays alive. If the loop must
   survive a restart, rely on the committed `HANDOFF.md` (a human or `RemoteTrigger` resumes from it).
5. **Stop this session's loop.** Do **not** issue another `ScheduleWakeup`. Report: handed off after
   N cycles, checkpoint committed (hash), heartbeat broadcast, successor scheduled for <time>
   (best-effort), backlog at X/Y.

> If the user chose **handoff-only** (no auto-spawn): do steps 1-3 and skip 4 — a human or an
> already-running peer picks it up from the weave announce + committed checkpoint.

## RESUME (successor session, on start / cron fire / weave nudge)

1. **Take the signal from the prompt/checkpoint, not the inbox.** Your resume instruction comes from
   the cron prompt (or a human) and the **committed `_workspace/HANDOFF.md`** — that is the
   authoritative signal. `weave_inbox` will **not** contain a same-identity handoff (a self-addressed
   message isn't in your own inbox), so don't depend on it; check it only for *cross-identity* notes
   from peers/operators. Run `weave_whoami` to confirm identity.
2. **Load the checkpoint.** Read `_workspace/HANDOFF.md` fully. Verify the worktree path + branch
   match and the tree is clean; run its **Verify-on-resume** commands to confirm a sane baseline
   (e.g. the 3 CI gates / a build) before mutating anything.
3. **Acknowledge.** Broadcast a heartbeat: `weave_send to: "all" subject: forge-relay:resumed` —
   `RESUMED @ <ts>, baseline verified, continuing at item <N>` (gives the mesh/operators a visible
   heartbeat). Use `weave_reply` only if a *cross-identity* peer sent a directly-addressed note.
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
**Happy path** (verified by smoke test): forge-loop hits the budget → spawn continuity-steward →
`HANDOFF.md` written + committed (`loop: handoff checkpoint`) → `weave_send to:"all"
subject=forge-relay:handoff` (heartbeat) → `CronCreate{recurring:false}` one-shot resume whose prompt
self-describes the resume → session stops. The successor (cron prompt or a human reading the committed
`HANDOFF.md`) verifies baseline green, broadcasts `forge-relay:resumed`, resets the session counter,
and continues at the next item. (Smoke notes: `durable` was reported session-only here, and the
self-identity handoff message did not appear in the successor's own inbox — so the committed
checkpoint + cron prompt are the signal, weave is the observable heartbeat.)

**Error path:** steward returns `HANDOFF INCOMPLETE` (backlog says item 4 done, but no commit exists
for it). Relay does NOT schedule a successor; it commits the checkpoint with the contradiction
flagged, sends `forge-relay:handoff-degraded` to `all`, and stops for human review — preventing a
successor from re-doing or skipping item 4.
