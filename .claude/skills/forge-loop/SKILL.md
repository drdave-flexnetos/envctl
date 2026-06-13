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

## State precedence (pin this — agents never re-rank it)
**Git > `.handoff/ledger.db` > `tasks/*.task.json` > `active.md` > packet.** The two markdown loop
surfaces (`.handoff/loop/HANDOFF.md`, `.handoff/loop/backlog.md`) rank **below** all of these —
never treat HANDOFF.md or backlog.md as higher precedence than Git or the witnessed ledger. When the
ledger (via `hf`) and a markdown view disagree, **the ledger wins** and the markdown is corrected.

## Durable state (the loop's memory)
All under the worktree's `.handoff/loop/` (the audit trail; preserve it):
- **`.handoff/loop/backlog.md`** — the human markdown VIEW. An ordered checklist of work items, each
  `- [ ] <TASK-####>: <one-line goal>` (→ `- [x]` when its cycle PASSES; `- [!]` blocked;
  `- [!!]` SUPERVISED/CRITICAL — never auto-run, see below). Sub-notes indented beneath carry
  dependency hints. **No dependency edges live here** — it is a view, not the ordering authority.
- **`handoff.task.v1` cards** (`.handoff/tasks/*.task.json`) — the structured surface with
  `dependencies`/`blocked_by` + a `status` enum (`backlog|active|claimed|blocked|checkpointed|review|done`).
  **After Epic-A TASK-0002 mints the cards, the CARDS own ordering** and `backlog.md` is just a view;
  **before** cards exist, parse deps from the markdown sub-notes. **Never tick a box in `backlog.md`
  that disagrees with the card's `status`** — the card (ledger-replayed) is authoritative.
- **`.handoff/loop/loop_state.md`** — the ledger: `cycles_this_session`, `cycles_total`,
  `cycle_budget`, `session_started` (UTC, passed in — never call Date.now), `last_item`, `status`.
- **Per-cycle artifacts** — `.handoff/loop/cycle/01_architect_plan.md` /
  `.handoff/loop/cycle/02_implementer_log.md` / `.handoff/loop/cycle/03_guardian_report.md`
  for the item currently in flight (same as a single feature-forge run).
- **Sentinels** under `.handoff/loop/`: `DONE`, `NEEDS-HUMAN`, `STOP` (read/write semantics below).

## `hf`-aware vs markdown picking & checkpointing
**IF `hf` is on PATH _and_ the ledger-residency guard holds** (ledger = `$META_ROOT/.handoff/ledger.db`;
run every ledger-touching verb from `$META_ROOT` — see the `handoff-sync` skill): delegate
next-item selection and checkpointing to the kernel (real verbs below). **ELSE** fall back to the
markdown-checkbox + sub-note dependency parsing path. Re-run the residency fail-closed check before
each hf call; on failure, drop to the markdown path for that cycle.

**Per-cycle verb sequence (the REAL shipped `hf` verbs — there is NO `hf drift` and NO `hf policy`):**
- **Pick / resume:** `hf resume --json` from `$META_ROOT` — read `next_task_id` + `next_command`
  (the kernel's `next_safe` dependency-DAG picker). That is the next item; do not re-derive ordering
  from markdown when hf is present.
- **Cycle start:** `hf claim <TASK-####>` (witnessed claim; mesh-coordinated so two sessions can't
  grab the same task).
- **Mid-cycle:** `hf checkpoint --auto` (routine boundary) or `hf checkpoint --note "<reason>"`
  (notable state) — appends a witnessed ledger event. **Claim/checkpoint alone NEVER mark done.**
- **Cycle PASS (terminal Done only):** `hf done <TASK-####> --pr <N>` — the single verb that marks a
  task Done in the ledger; pass the merged PR number.
- After `hf done`, `hf handoff` re-renders `.handoff/packets/latest.md` + `.handoff/active.md`.
Markdown-fallback equivalents: pick = top unchecked unblocked `- [ ]` (deps from sub-notes);
"done" = tick `- [x]` in `backlog.md`.

If `.handoff/loop/backlog.md` does not exist, create it first from the user's request (a roadmap, a
doc, or an explicit list), then start the loop. Keep items small and independent — one Engine
capability or one component per item — so a cycle fits comfortably under the budget.

## One iteration (the loop body)
1. **Read state.** `.handoff/loop/backlog.md` + `.handoff/loop/loop_state.md`. Confirm the worktree is
   clean (`git status`) and on the loop branch.
2. **Phase-0 stop checks (read ALL THREE sentinels first, in order):**
   - `.handoff/loop/STOP` present → **halt immediately**, no re-fire (human kill switch; takes
     priority over everything).
   - `.handoff/loop/NEEDS-HUMAN` present → stop and surface for a human; do not auto-pick around it.
   - `.handoff/loop/DONE` present **OR** completion confirmed (see below) → **DONE**: report, no re-fire.
     *Completion is confirmed when* `hf resume --json` reports `next_command: "done"` (hf present) **or**
     all cards are `status: done` / all `backlog.md` items are `- [x]`/`- [!]` (hf absent).
   - `cycles_this_session >= cycle_budget` → **HAND OFF**: invoke the `session-relay` skill and stop
     (do not re-fire from this session). This is the cycle-budget trigger.
3. **Pick** the next item:
   - **hf present:** take `next_task_id` from `hf resume --json` (the `next_safe` DAG picker) and
     `hf claim <TASK-####>`.
   - **hf absent:** the top unchecked unblocked `- [ ]` item, honoring deps parsed from sub-notes.
   - **`- [!!]` SUPERVISED/CRITICAL refusal:** if the picked item is marked `- [!!]` (e.g. the
     rtk-hook install, a live n8n/smoke test), the loop **REFUSES to auto-run it** — write
     `.handoff/loop/NEEDS-HUMAN` (with the item id + why it needs a human), do **not** claim/build it,
     and stop. Never auto-run a supervised item.
4. **Run one Feature Forge cycle** on it via the `feature-forge` orchestrator: architect → implementer
   → guardian, with the same routing/loop caps and `.handoff/loop/cycle/` artifacts. Mid-cycle, when
   hf is present, emit `hf checkpoint --auto` (or `--note`) at notable boundaries (claim/checkpoint
   **never** mark done). Commit on PASS / PASS-WITH-NOTES (area-prefixed subject). On cycle **PASS**,
   mark the task **terminal Done** with `hf done <TASK-####> --pr <N>` (hf present) — the only verb
   that marks done — then `hf handoff` to re-render the packet; in the markdown fallback, tick
   `- [x]`. On an **unrecoverable guardian FAIL or NEEDS-DECISION the loop can't route around**, write
   `.handoff/loop/NEEDS-HUMAN` and mark the item `- [!]` blocked with a one-line reason, then move to
   the next item (don't thrash).

   > **Multi-repo cycle (A2):** if the architect's plan for this item lists **>1 target repo**,
   > step 4's build runs the A2 shape (feature-forge **Phase 1.5 → Phase 2-A2**): one coordinated
   > meta worktree set, N implementers, per-repo guardian gates. The loop itself is unchanged —
   > still **one backlog item per cycle**, A2 is just the internal shape of that cycle's build.
   > Run `grit gc` per repo before each wave and keep heartbeat hygiene (the implementers refresh
   > their own TTLs). The cycle completes only when **all** target repos reach guardian PASS (or
   > are marked `- [!]` blocked). Cycle-budget counting and the `session-relay` handoff are
   > unchanged — an A2 cycle is still one cycle against the per-session budget.
5. **Write state back:** tick the markdown VIEW (`- [x]` done / `- [!]` blocked) **only if it agrees
   with the card status** — when hf is present the card (ledger-replayed via `hf done`) is
   authoritative; reconcile the box to the card, never the reverse (use `hf sync-cards` to re-derive
   cards if they look stale). Increment `cycles_this_session` and `cycles_total`, update `last_item`
   and `status` in `loop_state.md`, and append a one-line progress note. Commit the `.handoff/` update
   (text only — never a `ledger.db`).
6. **Re-fire** to continue the loop (see Self-pacing).

## Parallel mode (opt-in grit git-lock coordination)

When looping over items that span multiple meta repos, activate with `USE_GRIT=1`:

1. Before the first implementer: `for repo in $(meta project list --json | jq -r '.[].name'); do cd /home/drdave/Desktop/meta/$repo && grit init -y; done` (idempotent).
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
If invoked to **resume** (a `.handoff/loop/HANDOFF.md` exists, or weave inbox / the successor cron
prompt says so): follow `session-relay`'s resume protocol first (read HANDOFF + ack via weave +
reset `cycles_this_session`), then run the iteration body normally. **When hf is present, the
authoritative resume read is `hf resume --json`** (`next_task_id`/`next_command`) from `$META_ROOT`,
not the markdown — HANDOFF.md is the companion. When hf is absent, resume from the backlog's current
item per the markdown.

## Stop conditions & sentinel write semantics (end the loop — no re-fire)
Sentinels live under `.handoff/loop/`. **Phase-0 reads all three (STOP, NEEDS-HUMAN, DONE) before
picking;** write them as follows:
- **DONE** — write `.handoff/loop/DONE` only when completion is *confirmed*: `hf resume --json`
  reports `next_command: "done"` (hf present) **or** all cards are `done` / all backlog items
  `- [x]`/`- [!]` (hf absent). Then report the DONE summary, no re-fire.
- **NEEDS-HUMAN** — write `.handoff/loop/NEEDS-HUMAN` on (a) an unroutable guardian **FAIL** /
  NEEDS-DECISION, **or** (b) encountering any `- [!!]` SUPERVISED/CRITICAL item (rtk-hook, live
  smoke). Stop and surface for a human; do not auto-pick around it.
- **STOP** — `.handoff/loop/STOP` is the human kill switch: when present, **halt re-fire
  immediately**, ahead of all other checks.
- **Cycle budget reached** → hand off (session-relay), then stop.
- **A hard blocker the loop can't route around** (dirty/ambiguous worktree, repeated guardian FAIL on
  the same item) → write NEEDS-HUMAN, stop and report; don't burn cycles spinning.
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
