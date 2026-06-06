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

   > **Multi-repo (A2) checkpoint — durability.** When the cycle being handed off ran A2 (>1 target
   > repo), the **only** thing committed at handoff is **one** host `_workspace/HANDOFF.md`
   > (+`backlog.md` +`loop_state.md`) in the envctl host repo — that single commit is the durable
   > resume signal. **Member durability is the per-repo branch tip**, not the host commit: each repo's
   > work was already committed at **its own guardian PASS** in Phase 2-A2 step 6 (N independent
   > commits across the set) — so at handoff there is **no `meta git commit` across members**; the
   > member branches already carry their last-good tips. Each member `_workspace/<repo>/` is
   > **uncommitted in-flight scratch** — the successor resumes a not-yet-PASSed repo from its
   > **branch + last-good commit**, never from that scratch dir. The steward fills the full per-repo
   > table (columns `repo | worktree dir | branch | sub-item/module | sub-status | last-good commit |
   > in-flight phase | open grit claims | verify-on-resume cmd | option-y?`) — see `continuity-steward`
   > for the verbatim layout (host summary block + table + namespaced `## Option-Y wave`).
   >
   > Resume command in the prompt/checkpoint:
   > `/forge-loop resume from _workspace/HANDOFF.md --repos <r1>,<r2>,<r3>`.
   > The RESUME protocol below (2-pre/2/2a/2b) consumes that table to recreate the set, fan baseline
   > gates out, reconcile grit per repo, and stay idempotent across a double-resume.
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
2. **Load the checkpoint.** Read `_workspace/HANDOFF.md` fully.
   - **Single-repo cycle** (no `## Per-repo vector` table / "n/a — single-repo cycle"): verify the
     worktree path + branch match and the tree is clean; run its **Verify-on-resume** commands to
     confirm a sane baseline (e.g. the 3 CI gates / a build) before mutating anything. **Then skip
     2-pre/2/2a/2b** — the multi-repo machinery below is gated on the A2 per-repo table being present;
     the single-branch default flow is unchanged.
   - **A2 cycle** (the checkpoint carries a `## Per-repo vector` per-repo table): run **2-pre → 2 →
     2a → 2b** below, scoped to the table's repos, before continuing the loop.

2-pre. **Set existence + deterministic recreate** (FM-4). Detect the meta worktree set (`worktree
   list` takes **no** name arg):
   ```bash
   meta --json git worktree list | jq -e '.worktrees[]|select(.name=="<slug>")'
   ```
   - **Present** (exit 0) → reuse it as-is (recreate is a no-op; branches already check out their tips).
   - **Absent** (pruned) → recreate **on the recorded branches** from the table's `branch` column —
     branches persist even when worktrees are pruned, so this checks out the existing tip, no clobber:
     ```bash
     meta git worktree create <slug> --repo <r1>:<branch1> --repo <r2>:<branch2> ...
     ```
     **Never** pass a bare `--repo <alias>` — that mints a fresh `<slug>` branch and loses the tip.
   Post-detect/recreate, **assert each repo's HEAD == its recorded branch** (deterministic recreate):
   ```bash
   meta --json git worktree exec <slug> --parallel --include <r1,r2,...> -- git rev-parse --abbrev-ref HEAD
   ```
   For every `.results[]`, `stdout` (trimmed) must equal that repo's HANDOFF `branch`. Any mismatch →
   **ABORT-BLOCKED** (non-deterministic recreate — do not mutate; stop for a human).

2. **Baseline fan-out** (FM-3, two reductions over the set). Run only on the table's repos:
   1. **Cleanliness** — every not-yet-PASSed repo's tree must be clean (PASSed repos: cleanliness-only):
      ```bash
      meta --json git worktree exec <slug> --parallel --include <r1,r2,...> -- git status --porcelain
      ```
      Every repo's porcelain output must be **empty**; any non-empty → **BLOCK that repo** (dirty
      tree, don't mutate it) and carry on with the rest. **NB:** meta **omits** the `stdout` key for an
      empty result, so test `(.stdout // "")`, not `.stdout` — i.e.
      `... | jq -e '.results[] | select((.stdout // "") != "")'` succeeds **only** when some repo is
      dirty (absent key ⇒ clean).
   2. **Per-repo gates** — run each repo's recorded **verify-on-resume cmd** (envctl =
      `.forge/invariants.toml` gates; other repos = their descriptor gates or the generic-Rust
      fallback, PR-2 mechanism) via the same fan-out, e.g.:
      ```bash
      meta --json git worktree exec <slug> --parallel --include <r> -- <verify-on-resume cmd>
      ```
      Every `.results[].exit_code == 0`. Green only when **every not-yet-PASSed repo is clean AND its
      gates pass**; a red gate → BLOCK that repo. PASSed repos are cleanliness-only (their commit
      already landed).

2a. **grit reconcile across the whole A2 cycle** (FM-5, deadlock-safe). For each **not-yet-PASSed**
   repo in the table (sub-status ≠ PASS), reconcile that repo's grit state — this generalizes the
   PR-3 Option-Y reconciliation to the full cycle. Per repo:
   - **Reap + read live truth:** `grit gc` (reap the dead session's claims) → `grit status` (live
     truth — never infer from memory).
   - **Idempotency gate:** if the repo's **branch tip advanced past the recorded last-good commit**,
     it was already built by a prior successor → **skip / mark PASS** (do not redo).
   - **Option-X repo** (no `## Option-Y wave` sub-table for this repo, `option-y? = n/a`): **release
     strays** (`grit release -a <id>` for any live claim from the dead session) and **re-fan from the
     in-flight phase** — re-claim **fresh** from the table's in-flight module/`file::symbol`s and
     re-run that repo's Phase 2-A2 from its recorded in-flight phase.
   - **Option-Y repo** (has a `## Option-Y wave` sub-table, `option-y? = yes`): you own the `grit done`
     calls the dead session never reached. After `grit gc` → `grit status`, **per agent id** in that
     repo's sub-table:
     - **Already-merged** (`merged-via-done? = yes`) → **verify** the reworded merge commit exists on
       the task branch (area-prefixed subject, `Merged via grit Option Y`); nothing to redo.
     - **Gated PASS but unmerged** (guardian `PASS`, `.grit/worktrees/<id>` present) → keep-and-finish:
       assert `test "$(git rev-parse --abbrev-ref HEAD)" = "<task-branch>"`, then `grit done -a <id>`,
       then **reword** the resulting merge commit (area-prefixed subject, per Phase 2-Y).
     - **Work-incomplete / worktree gone / any guardian FAIL** → `grit release -a <id>`, **re-claim
       fresh** from the table's `file::symbol`s, and **redo** the module (edit-in-`.grit/worktrees/<id>`
       → gate → `done` → reword).
   **Never restore live locks; never force a conflicting `done`** — if a `grit done` conflicts (the
   task-branch HEAD does not advance to a `grit: merge agent/<id>` commit), surface it as **BLOCKED**
   and stop, don't force (there is no force/steal path). A **stale `.grit/merge.lock` self-heals**
   (grit checks `kill -0` + 30s mtime) — **don't delete it.**

2b. **Idempotent double-resume** (FM-7). Cron is best-effort and a human may resume the same
   checkpoint, so two successors can race. **Per repo**, compare the repo's **current branch tip** to
   the recorded **last-good commit**:
   - **Advanced** (tip ≠ last-good, moved forward) → **skip** — another successor already built this
     repo; do not redo it.
   - **Equal** → eligible; proceed with 2a's reconcile/rebuild for that repo.
   This is the whole-cycle form of the stand-down guard carried from this skill's Error handling: if
   *every* not-yet-PASSed repo has already advanced (cycle fully built by a peer) **and**
   `cycles_this_session` is reset, **stand down** rather than double-building.
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
- **A2 non-deterministic recreate (FM-4) → ABORT-BLOCKED:** in RESUME 2-pre, if a recreated/reused
  set's per-repo `git rev-parse --abbrev-ref HEAD` does **not** match the HANDOFF `branch` column for
  any repo (e.g. a bare `--repo alias` minted a fresh `<slug>` branch, or the recorded branch is
  gone), do **not** mutate that set — abort and stop for a human. A wrong-branch resume would build on
  the wrong tip.
- **A2 conflicting grit `done` (FM-5) → BLOCKED:** in RESUME 2a, if a `grit done` for an Option-Y
  writer does not advance the task-branch HEAD to a `grit: merge agent/<id>` commit, surface that repo
  as BLOCKED and stop reconciling it — never force/steal, never delete `.grit/merge.lock` (it
  self-heals). The other repos' reconcile continues.

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
