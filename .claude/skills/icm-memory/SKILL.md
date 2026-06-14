---
name: icm-memory
description: >-
  Query (recall) and persist (store) durable cross-session memory via ICM — the harness's persistent
  memory any agent can call AS NEEDED. ALWAYS use to RECALL relevant prior context before non-trivial
  work (past decisions, resolved errors, gotchas, user preferences for this project/repo) and to STORE
  durable memory on the trigger events (a decision made, an error resolved, a significant unit/task
  completed, a preference learned). Triggers on "recall", "remember", "what did we decide/learn", "have
  we seen this before", "store this", "persist this decision". Graceful no-op if ICM isn't installed —
  so the harness stays portable to repos without it. Recall before you work; store when you learn.
---

# ICM Memory — the harness's persistent, cross-session memory

ICM (Infinite Context Memory) is the durable memory that survives a session/agent boundary. This skill
is the **capability** any harness agent calls *at its own discretion* — recall relevant context before
acting, store a durable fact when it learns one. It is **not** a forced step: the lead/orchestrator
delegates *when* to use it at runtime, the same way it delegates any tool. The point is that the
ability is always available, so the harness is memory-aware and reusable **anywhere** it's ejected.

## Always recall before non-trivial work

Before starting a unit of work, pull what's already known so you don't re-derive or contradict it:

```bash
icm recall "<short query of what you're about to do>"            # search memories
icm recall "<query>" -t "decisions-<project>"                    # filter by topic
icm recall-context "<query>" --limit 5                           # formatted for prompt injection
```

MCP equivalent (when the `icm` MCP server is connected): `mcp__icm__icm_memory_recall`. Recall **only
what's relevant** to the task — a targeted query, not a dump. Use a returned memory as *background that
was true when written*; if it names a file/flag/symbol, verify it still exists before relying on it.

> **Two layers — this skill is the second.** The *most important* recall is **deterministic and
> up-front**: a `SessionStart` hook that runs `icm recall-context` at every session start (no model
> decision), so the agent is primed before its first token — a missed recall makes the *whole* session
> run blind. (Eject prints that hook snippet; within the meta workspace it's inherited from the
> user-global settings.) This skill is the **as-needed complement** the model calls *mid-task* for a
> targeted recall the up-front priming didn't cover, and for all stores. Priming-up-front > store-at-end.

## Store when you learn something durable (the trigger events)

Store **immediately** when any of these happen — before moving on, not "later":

| Trigger | Topic | Importance |
|---------|-------|-----------|
| **Error / blocker resolved** | `errors-resolved` | high |
| **Architecture / design decision made** | `decisions-<project>` | high |
| **User preference / correction learned** | `preferences` | critical |
| **Significant unit/task completed** (a ported+verified unit, a merge landed, a phase done) | `context-<project>` | high |
| **~20 tool-calls without a store** | `context-<project>` (progress summary) | medium |

```bash
icm store -t <topic> -c "<concise, self-contained fact + why it matters>" -i <critical|high|medium|low> -k "kw1,kw2,kw3"
```

MCP equivalent: `mcp__icm__icm_memory_store`. Write the fact so a future agent with **zero context** can
use it: state the decision/finding **and the why**, name the evidence (file:line / PR / commit), and
keep it one self-contained fact per store. Other useful commands: `icm update <id> -c "..."` (edit in
place), `icm recall ... ` then refine; `icm topics` / `icm health` for hygiene.

## Do NOT store

Trivial details, anything already captured in `CLAUDE.md`/the ledger/git history, or ephemeral state
(build logs, `git status`, scratch). Memory is signal, not a transcript — overstoring buries the
signal. Before storing, check for an existing memory that already covers it and **update** that rather
than duplicating.

## Portability — graceful no-op when ICM is absent

So the harness stays usable in repos without ICM, **degrade silently** rather than fail: if
`command -v icm` is empty *and* the `icm` MCP tools aren't available, skip recall/store for this run and
note once that durable memory is unavailable here — never block a unit of work on a missing optional
memory backend. (When ICM *is* present, using it is expected per the triggers above.)

## How agents use it (delegated at runtime, not hard-wired)

Each agent decides, in the moment, whether memory helps the task in front of it — e.g. a researcher
recalls prior research on this repo before re-deriving it and stores its reuse-map conclusions; a
porter/merge-integrator recalls a prior decision/conflict-resolution for a symbol and stores a resolved
gotcha; a verifier stores a parity divergence worth remembering. The orchestrator makes this skill
available to the team; **which** agent calls it **when** is a runtime delegation, not a fixed step — that
flexibility is the point.
