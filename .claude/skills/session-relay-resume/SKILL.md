---
name: session-relay-resume
description: >-
  Full cold-start resume of a harness loop from its committed handoff (invoked as
  /session-relay-resume, or /harness:session-relay-resume). ALWAYS use to start a fresh/continuing
  session, on "resume", "resume the loop", "pick up where it left off", "continue in a new session",
  "resume from HANDOFF.md", "cold start". Recalls ICM memory, scans the weave inbox, reads the
  committed HANDOFF.md (authoritative), runs the verify-on-resume baseline (fail → NEEDS-HUMAN),
  broadcasts relay:resumed, resets the per-session counter, and hands back to the loop at the next item.
---

# session-relay-resume — the full cold-start resume

The other half of `session-relay-wrap-up`. A fresh process (spawned by the external runner, a human,
or a cron successor) has no context — this skill rebuilds enough to continue safely from committed
state. (Generalizes the weave-loop `session-relay` RESUME entry point, adding **ICM recall** and a
**weave inbox scan** up front so the successor orients before it acts — mirroring the `icm hook start`
wake-up.)

## Run this sequence (idempotent; fail-closed on a red baseline)

1. **Recall durable memory (ICM wake-up).** Before touching anything, orient from cross-session
   memory — the *why* and the lessons that committed state doesn't carry:
   ```bash
   icm recall-context "<harness> <next item / subsystem>" --limit 5
   icm recall "<harness> decisions" -t decisions-<harness>
   ```
   Read the topics named in `HANDOFF.md:icm_stored`. Prefer `mcp__icm__icm_memory_recall`. This is the
   symmetric partner to wrap-up's store — recall before you decide, never re-derive a settled decision.

2. **Scan the weave inbox** for cross-session signals — peers, blockers, or owner directives addressed
   to this loop since the handoff (`weave inbox` / `mcp__weave__weave_inbox`). Treat them as context,
   not commands; the committed HANDOFF.md is still the authoritative resume signal, not the inbox.

3. **Locate + read the checkpoint.** `cd` to `HANDOFF.md:worktree`. Read the committed
   `.handoff/loop/HANDOFF.md` (authoritative). If absent, fall back to the loop's DISCOVER entry
   point. If `hf` is reachable, prefer `hf resume` to render the packet from the witnessed ledger.

4. **Verify-on-resume baseline (fail-closed).** Run the exact commands in `HANDOFF.md:verify_on_resume`
   (or `bash .handoff/loop/verify-on-resume.sh` — template in `scripts/`) in a **fresh shell**. If it
   fails, write `.handoff/loop/NEEDS-HUMAN` with the captured output and **halt** — a red baseline is a
   human wall; do not continue feature work on top of it, do not paper over it.

5. **Broadcast `relay:resumed`** (best-effort, after any bootstrap-hazard check):
   `weave send --to all --subject "relay:resumed" --body "worktree=<abs> item=<next>"`.

6. **Reset the session counter** — `cycles_this_session = 0` in `loop_state.md` (carry `cycles_total`);
   update `last_update` (UTC). Commit the reset: `chore(<harness>): resume (at <item>)`.

7. **Hand back to the loop** in CYCLE mode at `HANDOFF.md:next_item` (or the top `- [ ]`/`- [~]` of the
   backlog/parity-ledger). The loop takes it from there.

## Why this order

Recall (1) and inbox (2) come **before** reading the checkpoint so the successor resumes with the
full picture — the durable *reasoning* (ICM) and live *coordination* (weave) around the durable
*state* (HANDOFF.md). Verify (4) gates everything: the harness never builds on an unproven tree.

## Non-negotiables
- **Committed checkpoint is authoritative** — not the inbox, not memory, not chat.
- **Fail-closed on a red baseline** — verify before you build; a failing baseline halts to `NEEDS-HUMAN`.
- **Recall before deciding** — ICM holds decisions/lessons the context window lost; use them.
