---
name: harness-evolution
description: >-
  How to turn a harness run into harness upgrades — evaluate the run, mine generalizable lessons,
  route each to the right target (skill / agent / orchestrator / description / script), and apply
  (low-risk, in-scope) or propose (structural) the change, fail-closed. ALWAYS use at the end of any
  harness run (DONE or HAND OFF) and on "evaluate the run", "retro", "what did we learn", "improve
  the harness", "upgrade the harness", "apply lessons", "why did the loop struggle". The harness's
  self-improvement method (automates Phase 7). Used by the evolution-steward agent.
---

# Harness Evolution

A harness gets better only if each run teaches the next one. This skill is the method the
`evolution-steward` runs at every run boundary: **evaluate → mine lessons → route → apply/propose →
record**. Done well it compounds; done carelessly it overfits or weakens the harness — so the rules
below are about *safe* improvement.

## 1. Evaluate the run (from durable artifacts, not memory)

Reconstruct the run from `.handoff/loop/` (`loop_state.md`, the backlog/ledger, `findings/*.md`,
`HANDOFF.md`, commit log) and score four axes — write to `.handoff/loop/evaluation.md`:

- **Friction** — wasted cycles, retries, items that bounced backward, places an agent had to guess
  because an instruction was ambiguous.
- **Gate quality** — did the QA/parity/verify gate catch real defects? Did any defect slip past it
  (caught later)? Did it false-block correct work? A gate that both missed a real bug *and*
  false-blocked is the highest-value upgrade target.
- **Coverage** — anything left behind, deferred, or silently capped vs. the inventory/backlog.
- **Human walls** — every `NEEDS-HUMAN` / manual intervention: was it a genuine wall or an avoidable
  gap the harness could close?

## 2. Mine generalizable lessons

Root-cause each friction point to the **class** of problem, not the instance. The test: *would this
lesson help a future run on a different input?* If it only helps the exact case you just saw, it's
overfit — re-generalize or drop it.

> Bad (overfit): "porter left `token.ts:refresh` half-ported." Good (general): "porters drop error
> branches under time pressure → the translate skill needs a per-branch no-stub checklist + the
> orchestrator should size units smaller so a unit fits one cycle."

A lesson seen **once** is *noted* in the ledger; the **second** recurrence of the same class →
upgrade now (the recurrence counter is why the ledger is append-only across runs).

## 3. Route each lesson to a target

| Lesson type | Target | Typical edit |
|-------------|--------|--------------|
| Output too shallow / wrong | the agent's **skill** body | add a criterion, a worked example, a checklist |
| Missing capability / fuzzy role | the **agent** def | sharpen the role; or *propose* a new agent |
| Wrong phase order / missing step / dead data path | the **orchestrator** | reorder / add a step / fix the bus |
| Skill didn't trigger when it should have | the skill **description** | add the missing trigger phrasing |
| Same helper written by hand repeatedly | the skill's **`scripts/`** | bundle it once |

## 4. Apply or propose — fail-closed

**Auto-apply** only *low-risk, in-scope* edits: tighten an instruction, add an example/trigger/
checklist item, bundle a repeated script, fix a stale reference.

**Propose for owner approval** (write `.handoff/loop/proposed-upgrades.md`, don't apply):
add/remove/merge an agent, reorder phases, change team composition, or touch any gate/guard.

Hard rules:
- **Never weaken a gate.** You may only strengthen QA/parity/validate/DONE criteria. "Loosen the
  check so cycles pass" is a defect; refuse it and record why.
- **Scope law.** Upgrade only the harness that ran. Cross-harness lessons (e.g. a shared agent) are
  *proposed* to those harnesses, never force-applied.
- **Standard flow.** Every applied change goes feature branch → PR → auto-merge with a CLAUDE.md
  change-history row. Never an uncommitted live mutation; never mid-cycle.
- **Smaller is safer.** Minimal upgrade that fixes the root cause beats a rewrite.

## 5. Record (durable memory)

- Append every lesson to the durable **lessons ledger** (`harness/LESSONS.md` in the plugin, or
  `LESSONS.md` at the repo root when ejected) — append-only; bump the recurrence counter; set status
  `noted` / `applied` / `proposed`.
- Add a CLAUDE.md change-history row for each applied upgrade (date · change · target · reason=the lesson).

## Lessons ledger row format

```
| <date> | <harness> | <lesson (class, generalized)> | <evidence: cycle/finding> | <recurrence> | <routed-to> | <status> |
```
