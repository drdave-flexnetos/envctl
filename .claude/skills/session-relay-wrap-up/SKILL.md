---
name: session-relay-wrap-up
description: >-
  Full end-of-session wrap-up + handoff for a harness loop (invoked as /session-relay-wrap-up, or
  /harness:session-relay-wrap-up). ALWAYS use to close a session at cycle budget, on STOP, or when
  the owner says "wrap up", "wrap up the session", "hand off", "checkpoint and stop", "prep handoff",
  "close out". Runs the retro, persists durable memory to ICM, writes + commits the authoritative
  HANDOFF.md, broadcasts the weave heartbeat, arms a best-effort successor, then stops. The committed
  HANDOFF.md is the resume signal — weave is only the heartbeat.
---

# session-relay-wrap-up — the full wrap-up + handoff

The clean way to end a loop session so the next one resumes cold with zero loss. It composes the
harness's continuity primitives into one ordered, idempotent sequence. Pairs with
`session-relay-resume`. (Generalizes the weave-loop `session-relay` HAND OFF entry point, adding the
**Phase E retro** and explicit **ICM persistence** so lessons and decisions survive the boundary, not
just the loop state.)

## Run this sequence (each step idempotent; stop on a terminal sentinel)

1. **Stop-checks first.** If `.handoff/loop/STOP` or `.handoff/loop/NEEDS-HUMAN` already exists, the
   run already terminated — log it and exit without re-handing-off.

2. **Phase E retro** — invoke `evolution-steward` (skill `harness-evolution`) for the lightweight
   retro: evaluate the session (friction / gate quality / coverage / human walls), append lessons to
   `LESSONS.md`, and write any `proposed-upgrades.md`. Capturing lessons *now* is why they survive the
   budget boundary. (Defer *applying* structural upgrades — wrap-up only records.)

3. **Persist durable memory to ICM** (the store half, symmetric to resume's recall — mirrors the
   `icm hook end` / `icm-memory` discipline). Store on the triggers that fired this session, before
   committing:
   ```bash
   icm store -t decisions-<harness> -c "<design decision + why>"            -i high   -k "kw1,kw2"
   icm store -t errors-resolved     -c "<what broke + the fix>"             -i high   -k "kw1,kw2"
   icm store -t context-<harness>   -c "<session summary: units done, next>" -i high   -k "kw1,kw2"
   ```
   Prefer the MCP tools (`mcp__icm__icm_memory_store`) when available. Do NOT store ephemeral state
   (build logs, git status) — that lives in `.handoff/loop/`. ICM holds the *why* and the lessons.

4. **Write the checkpoint** — spawn `continuity-steward` with the worktree, the in-flight cycle, and
   the orchestrator pipeline state. It writes the cold-start `.handoff/loop/HANDOFF.md` (layout below)
   in one pass, keeping the orchestrator's context lean. Overwrite — the steward body is authoritative.
   If the meta handoff kernel (`hf`) is reachable, prefer `hf checkpoint` / `hf handoff` to render the
   packet from the witnessed ledger; the file-based form is the fallback.

5. **Commit** — `chore(<harness>): handoff (at <item>)`, including `HANDOFF.md` + `.handoff/loop/`
   state + any wrap-up edits. **A fresh process must resume from this commit alone** — this is the
   real payload.

6. **Weave heartbeat (best-effort)** — broadcast `to:"all"`:
   `weave send --to all --subject "relay:handoff" --body "worktree=<abs> item=<next> reason=<budget|stop>"`.
   Bootstrap-hazard guard: if *this harness's own messaging code* is in the diff this cycle, skip the
   heartbeat and log the skip — the committed file is the truth.

7. **Best-effort one-shot successor** — `CronCreate {recurring:false}` ~3 min out, self-describing:
   `"/session-relay-resume from .handoff/loop/HANDOFF.md (worktree=<abs>, model=opus)"`. Session-only
   in this runtime; the committed HANDOFF.md is the survives-restart signal (a human or the external
   runner resumes from it).

8. **Stop** — no `ScheduleWakeup`. The next runner iteration spawns a fresh `claude -p` (the `/new`
   effect) which enters `session-relay-resume`.

## What `HANDOFF.md` must contain (cold-start test)

A successor given ONLY this file + the repo must resume correctly. Required:
```markdown
# HANDOFF — <harness>
closed_utc: <UTC>           branch: <branch>      worktree: <abs path>
cycle_budget: <n>           cycles_total: <N>     cycles_this_session: <n>
last_item: <id>             next_item: <id>       orchestrator_phase: <phase>
last_agent: <name>          gate_status: <PASS|FAIL|n/a>   pr_url: <url|(none)>
landed_this_session:
  - <sha> <subject>
findings: <pointers into .handoff/loop/findings/*.md — do not inline>
decisions_and_dead_ends: <what would otherwise be re-litigated/re-tried>
icm_stored: <topics written this session, so resume recalls them>
verify_on_resume: <the exact commands the successor runs FIRST to confirm green>
resume_command: /session-relay-resume from .handoff/loop/HANDOFF.md
```

## Non-negotiables
- **Write state down, then commit** — never hold the plan only in context.
- **The committed HANDOFF.md (or `hf` packet) is authoritative** — not the weave inbox (a self-
  addressed message doesn't land in your own inbox; a same-machine successor shares your identity).
- **Capture lessons + store memory before stopping** — the retro and ICM store are part of wrap-up,
  not optional afterthoughts; they're what makes the *next* session smarter, not just unblocked.
