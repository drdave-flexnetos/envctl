---
name: session-relay
description: "Hand off a long-running harness loop (the feature loop `forge-loop` OR the environment loop `env-install-loop`) to a FRESH session before context rot / token burn degrade it, and resume from a handoff. ALWAYS use when: a loop's cycle budget is reached, the user says 'hand off', 'transfer the session', 'continue in a new session', 'pass the baton', or a session needs to 'resume from handoff'/'pick up the loop'/'resume the env install'. Coordinates the transfer over weave (cross-identity heartbeat) and schedules a best-effort successor cron — the committed checkpoint is the real resume signal. Do NOT use for one-off features or normal in-session work."
---

# Session Relay (weave handoff + cron successor)

You carry a harness loop (feature or environment) across session boundaries with **zero loss**. A single session
degrades as context grows (rot) and gets expensive (token burn); the loop's defense is to run as a
chain of short sessions, each handing a durable checkpoint to the next. This skill is that handoff —
and the resume on the other side. It has two entry points: **HAND OFF** and **RESUME**.

## State precedence (pin this)
**Git > `.handoff/ledger.db` > `tasks/*.task.json` > `active.md` > packet.** The loop's
`HANDOFF.md`/`backlog.md` rank **below** all of these — never treat them as higher precedence than
Git or the ledger.

## `hf`-aware checkpoint (preferred when the kernel is built)
**IF `hf` is on PATH _and_ the ledger-residency guard holds** (ledger = `$META_ROOT/.handoff/ledger.db`;
run hf from `$META_ROOT` — see the `handoff-sync` skill): the **canonical checkpoint is
`hf checkpoint`** (witnessed ledger event) and the **canonical packet is `hf handoff` →
`.handoff/packets/latest.md`** (`handoff.packet.v2`, auto-writes `.handoff/active.md`). The
`continuity-steward`'s `.handoff/loop/HANDOFF.md` becomes a **non-authoritative human companion**
that LINKS the rendered packet and must **not** duplicate the kernel-owned State-Precedence /
Next-Command fields. Run every ledger-touching verb from `$META_ROOT` and re-run the residency
fail-closed check (`test ! -e .handoff/ledger.db && ! git ls-files .handoff | grep -q ledger.db`);
if it fails, drop to the ELSE branch.
**ELSE (`hf` absent / guard unsatisfied):** the hand-written `.handoff/loop/HANDOFF.md` (below) is
the checkpoint of record. The relay logic is otherwise identical — only the checkpoint mechanism
swaps.

## Substrates (verify before using)
- **Checkpoint:** `.handoff/loop/HANDOFF.md`, produced by the `continuity-steward` agent — the cold-start
  resume package (the hand-written fallback). When `hf` is present, the **authoritative** payload is
  the hf-rendered `.handoff/packets/latest.md`, and HANDOFF.md is the companion that links it.
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
  `.handoff/loop/HANDOFF.md`** (any new session resumes from it) — optionally escalate to `RemoteTrigger`
  (claude.ai routine) when an unattended cloud successor is wanted.

---

## HAND OFF (current session, at cycle budget)

Run these in order; each step is durable before the next, so a crash mid-handoff is recoverable.

1. **Produce the checkpoint.**
   - **`hf`-aware path (preferred):** from `$META_ROOT`, run `hf checkpoint --note "<cycle boundary>"`
     (witnessed ledger event) then `hf handoff` (renders `.handoff/packets/latest.md` +
     `.handoff/active.md`, `handoff.packet.v2`). Then spawn `continuity-steward` to write the
     **companion** `.handoff/loop/HANDOFF.md` that LINKS the rendered packet (no duplication of
     kernel fields). Re-run the residency fail-closed check first; on failure use the fallback.
   - **Fallback (`hf` absent):** spawn the `continuity-steward` agent (general-purpose, opus), passing
     the current UTC timestamp (you supply it — agents/scripts can't read the clock), the worktree
     path + branch, and the loop ledger. It writes `.handoff/loop/HANDOFF.md` and returns
     `HANDOFF READY` / `HANDOFF INCOMPLETE`. If INCOMPLETE, fix the gap (or note it) before continuing —
     never hand off a checkpoint you know is wrong.
2. **Commit it.** `git add .handoff/ && git commit` (subject `loop: handoff checkpoint @ <ts>`).
   Commit the rendered packet/active + the companion HANDOFF.md — **never** a `ledger.db` (it lives
   once at `$META_ROOT/.handoff/ledger.db`, gitignored). The successor resumes from committed state,
   so the checkpoint must be in git, not just on disk. Push if the loop runs against a shared remote.

   > **Multi-repo (A2) checkpoint.** When the cycle being handed off ran A2 (>1 target repo), the
   > meta-level checkpoint is still **one** committed `.handoff/loop/HANDOFF.md`, but it carries a
   > **per-repo state table** (the steward fills it; see `continuity-steward`):
   >
   > | repo | worktree dir | branch | sub-item/module | last-good commit | in-flight phase | open grit claims | verify-on-resume cmd |
   > |------|--------------|--------|-----------------|------------------|-----------------|------------------|----------------------|
   >
   > Resume command in the prompt/checkpoint:
   > `/forge-loop resume from .handoff/loop/HANDOFF.md --repos <r1>,<r2>`.
   >
   > **PR-1 scope = the schema + a 2-repo demo.** Full N-branch resume correctness — live grit-lock
   > reconciliation and dependency-ordered re-fan-out — is **staged to PR-4**. For PR-1, resume does
   > **not** attempt to restore live locks: it runs `grit gc` per repo (reaping the dead session's
   > claims) and **re-claims fresh** from the table's in-flight module, then continues.
3. **Broadcast the heartbeat over weave.** `weave_send` `to: "all"` (cross-identity observers — do
   **not** address your own identity; it won't reach the successor's inbox):
   - `subject`: `forge-relay:handoff`
   - `body`: the `.handoff/loop/HANDOFF.md` path, worktree path + branch, the resume command, and the
     one-line backlog status (e.g. "4/7 done, resume at item 5"). Pointer-sized; detail lives in the
     committed checkpoint. This is observation/audit, not the resume signal.
4. **Schedule the successor.** `CronCreate { recurring: false }` with a near-future one-shot time and
   a `prompt` that re-enters the loop in RESUME mode **and self-describes the resume** (the prompt is
   the real signal — don't rely on inbox), e.g.:
   `"/forge-loop resume from .handoff/loop/HANDOFF.md (branch <branch>, worktree <path>) — read the
   committed handoff checkpoint, verify baseline, then continue at the backlog's next item"`.
   One-shot avoids double-runs; the next handoff creates the next one-shot. **Caveat (verified):**
   `durable` is not honored here — this fires only while the session stays alive. If the loop must
   survive a restart, rely on the committed `.handoff/loop/HANDOFF.md` (a human or `RemoteTrigger` resumes from it).
5. **Stop this session's loop.** Do **not** issue another `ScheduleWakeup`. Report: handed off after
   N cycles, checkpoint committed (hash), heartbeat broadcast, successor scheduled for <time>
   (best-effort), backlog at X/Y.

> If the user chose **handoff-only** (no auto-spawn): do steps 1-3 and skip 4 — a human or an
> already-running peer picks it up from the weave announce + committed checkpoint.

## RESUME (successor session, on start / cron fire / weave nudge)

1. **Take the signal from the prompt/checkpoint, not the inbox.** Your resume instruction comes from
   the cron prompt (or a human) and the **committed `.handoff/loop/HANDOFF.md`** — that is the
   authoritative signal. `weave_inbox` will **not** contain a same-identity handoff (a self-addressed
   message isn't in your own inbox), so don't depend on it; check it only for *cross-identity* notes
   from peers/operators. Run `weave_whoami` to confirm identity.

   > **Legacy-path fallback (read-only).** A successor scheduled *before* this migration may still
   > target the old `_workspace/` path. If `.handoff/loop/HANDOFF.md` is absent, read the legacy
   > `_workspace/{HANDOFF.md,DONE,NEEDS-HUMAN,STOP}` as a **read-only** fallback to recover that
   > in-flight mission. **Never write the legacy path** — once recovered, re-emit all new state under
   > `.handoff/loop/`.
2. **Load the checkpoint.** **`hf`-aware path:** the authoritative resume read is
   `hf resume --json` from `$META_ROOT` (`handoff.packet.v2`; gives `next_task_id`/`next_command`)
   and the rendered `.handoff/packets/latest.md`; `.handoff/loop/HANDOFF.md` is the companion for
   loop-only context (worktree set, A2 table). **Fallback (`hf` absent):** read
   `.handoff/loop/HANDOFF.md` fully — it is the checkpoint. Either way, verify the worktree path +
   branch match and the tree is clean; run the **Verify-on-resume** commands to confirm a sane
   baseline (e.g. the 3 CI gates / a build) before mutating anything.
3. **Acknowledge.** Broadcast a heartbeat: `weave_send to: "all" subject: forge-relay:resumed` —
   `RESUMED @ <ts>, baseline verified, continuing at item <N>` (gives the mesh/operators a visible
   heartbeat). Use `weave_reply` only if a *cross-identity* peer sent a directly-addressed note.
4. **Reset the ledger.** In `.handoff/loop/loop_state.md` set `cycles_this_session = 0` (the budget is
   per-session); keep `cycles_total`. 
5. **Continue the loop.** Re-enter `forge-loop`'s iteration body at the backlog's current item. The
   successor is now the active session and will itself hand off at the next budget.

---

## Error handling
- **Steward INCOMPLETE / contradictory state:** do not auto-spawn a successor onto a bad checkpoint.
  Commit the partial checkpoint with its gaps flagged, weave-announce as `forge-relay:handoff-degraded`,
  and stop for a human — a clean stop beats a confident-but-wrong resume.
- **weave unavailable** (`weave_doctor` fails): the committed `.handoff/loop/HANDOFF.md` + durable cron are the
  fallback path of record — the successor can resume from the file alone. Note the missing channel in
  the handoff body and proceed; weave is coordination, not the payload.
- **Duplicate/again resume:** if a successor finds `cycles_this_session` already reset and the top
  item in progress with fresh commits, another session may have resumed — re-check the inbox and the
  latest commit; if so, stand down rather than double-building.

## Test Scenarios
**Happy path** (verified by smoke test): forge-loop hits the budget → spawn continuity-steward →
`.handoff/loop/HANDOFF.md` written + committed (`loop: handoff checkpoint`) → `weave_send to:"all"
subject=forge-relay:handoff` (heartbeat) → `CronCreate{recurring:false}` one-shot resume whose prompt
self-describes the resume → session stops. The successor (cron prompt or a human reading the committed
`.handoff/loop/HANDOFF.md`) verifies baseline green, broadcasts `forge-relay:resumed`, resets the session counter,
and continues at the next item. (Smoke notes: `durable` was reported session-only here, and the
self-identity handoff message did not appear in the successor's own inbox — so the committed
checkpoint + cron prompt are the signal, weave is the observable heartbeat.)

**Error path:** steward returns `HANDOFF INCOMPLETE` (backlog says item 4 done, but no commit exists
for it). Relay does NOT schedule a successor; it commits the checkpoint with the contradiction
flagged, sends `forge-relay:handoff-degraded` to `all`, and stops for human review — preventing a
successor from re-doing or skipping item 4.
