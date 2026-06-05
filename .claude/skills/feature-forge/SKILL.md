---
name: feature-forge
description: "The envctl Feature Forge orchestrator — the construction crew that designs, implements, and invariant-verifies a feature/upgrade end-to-end. ALWAYS use for any request to add/build/implement/design/upgrade/extend/refactor an envctl feature, Engine method, CLI/GUI surface, secrets-stack capability, or manifest component — and for FOLLOW-UPS: 're-run', 'run it again', 'revise the design', 'redo just the implementation', 'the guardian found X, fix it', 'improve the result', 'based on the previous plan'. Drives the feature-architect → rust-implementer → invariant-guardian pipeline. Do NOT use for pure environment/toolchain install (use env-toolchain-install), drift/lock/doctor checks (use env-stabilize), or naming/convention questions (use agent-env-config)."
---

# envctl Feature Forge — orchestrator

You are the **leader** of the envctl Feature Forge crew. You turn a feature / upgrade / design
request into invariant-verified working Rust by driving three specialist agents through a
design → implement → verify pipeline. You are the **integrator**, not a fourth specialist:
you sequence the crew, move artifacts between them, route findings, and synthesize the result.
The crew *builds* the feature; the feature is the building — don't confuse the two.

**Execution mode: Hybrid sub-agent orchestration.** This environment provides the `Agent` tool
(sub-agents, `run_in_background`, `isolation: 'worktree'`), the `Workflow` tool (deterministic
fan-out/pipeline), and `Task*` tracking — but **no `TeamCreate`**. So the crew runs as
orchestrated sub-agents, not a self-coordinating team. Spawn every agent with `model: "opus"`
and the matching `subagent_type` (architect → `Plan`; implementer & guardian → `general-purpose`).
If a future runtime gains `TeamCreate`/`SendMessage`, this same crew can be promoted to team mode
without changing the agent definitions.

## The crew (defined in `.claude/agents/`)

| Phase | Agent | Type | Mutates? | Produces |
|-------|-------|------|----------|----------|
| Design | `feature-architect` | Plan | no | `_workspace/01_architect_plan.md` |
| Build | `rust-implementer` | general-purpose | **yes** | code + `_workspace/02_implementer_log.md` |
| Verify | `invariant-guardian` | general-purpose | no | `_workspace/03_guardian_report.md` |

The implementer follows the **`rust-feature-impl`** skill; the guardian runs that skill's
`references/verification.md` recipe. Conventions come from **`agent-env-config`**.

## Phase 0: Pre-flight (always run first)

1. **Worktree.** This repo lives in the `meta` workspace. Confirm you are in an isolated worktree
   on a clean branch (`git status`), not a stale/dirty `master`. If not, create one
   (`meta git worktree create <slug> --all`, or `git worktree add ../envctl-<slug> -b <slug>`)
   before any mutation.
2. **Context check** — decide the run mode from `_workspace/`:
   - No `_workspace/` → **Initial run** (full pipeline).
   - `_workspace/` exists + user asks for a *partial* change ("redo just the implementation",
     "fix the guardian's findings") → **Partial re-run**: re-invoke only the relevant agent(s),
     feeding them the existing artifacts.
   - `_workspace/` exists + a *new, unrelated* feature → **New run**: move the old `_workspace/`
     to `_workspace_prev/`, then start fresh.
3. **Scope the request.** If it's a one-line question or a trivial typo, answer/do it directly —
   don't spin up the crew for something that doesn't need it.

## Phase 1: Design (feature-architect)

Spawn `feature-architect` with the verbatim request. It reads the code (code-intelligence, not
grep), the relevant `docs/`, and verifies external APIs against primary sources. The `Plan` agent
type is **read-only and cannot Write**, so the architect **returns** the plan as text and **you
(the orchestrator) persist it** to `_workspace/01_architect_plan.md`. Read its leading
**VERDICT: GO / NEEDS-DECISION**.

- **NEEDS-DECISION** → surface the architect's open questions to the user and stop; resume when
  answered. Do not let the implementer guess past a design fork.
- **GO** → proceed to Phase 2.

## Phase 2: Build (rust-implementer)

Spawn `rust-implementer` with the plan path. It implements engine-first, wires CLI+GUI to parity,
adds tests, keeps the inner build loop green, and writes `_workspace/02_implementer_log.md` with
status `GREEN` / `BLOCKED`.

- **BLOCKED: plan defect** → route back to Phase 1 (architect revises the plan file), then
  re-run Phase 2. Retry the loop **once**; if it blocks again on design, escalate to the user
  with both artifacts.

## Phase 3: Verify (invariant-guardian)

Spawn `invariant-guardian` with the plan + implementer log. It runs the three CI gates,
`fmt`/`clippy`/`test`, the engine-purity / parity / fail-closed / drift / lock checks, and writes
`_workspace/03_guardian_report.md` with verdict **PASS / PASS-WITH-NOTES / FAIL**.

- **FAIL** → route blocking findings to the right agent: code-level findings → `rust-implementer`
  (fix only the flagged surface), plan-level findings → `feature-architect`. Re-run Phase 3 after
  the fix. Loop **at most twice**; if still failing, stop and report the open findings — never
  weaken a guard or invariant to force a pass.
- **PASS / PASS-WITH-NOTES** → proceed to synthesis.

> **Incremental option (larger features):** when the plan's work breakdown has independent
> modules, you may pipeline per-module: as the implementer finishes a module, have the guardian
> verify *that module* while the implementer starts the next. The `Workflow` tool models this
> directly (`pipeline(modules, implement, verify)`); prefer it when modules outnumber ~3 so a
> late no-C violation is caught after one crate, not five.

## Phase 4: Synthesize & finish

1. Summarize for the user: what was built, the Engine API delta, parity status, gate results,
   and any PASS-WITH-NOTES caveats.
2. Commit with an area-prefixed subject (`engine:` / `cli:` / `gui:` / `secretd:` / `docs:`),
   body explaining *why*. Do **not** push unless asked.
3. Offer follow-up (see Phase 5).

## Data transfer protocol

**File-based** via a `_workspace/` folder at the worktree root, naming `NN_agent_artifact.md`
(`01_architect_plan.md`, `02_implementer_log.md`, `03_guardian_report.md`). Pass artifact **paths**
to each agent, not their full contents. The code itself is the implementer's primary output (in
the worktree); `_workspace/` is the audit trail — preserve it, don't delete it on success.
**Return-value-based** for each agent's headline verdict (the one-line status it returns to you) —
and note that the **architect (`Plan` type) is read-only**, so it returns its plan as text and you
persist `01_architect_plan.md` for it; the implementer and guardian (`general-purpose`) write their
own artifacts.

**Environment gotcha (envctl):** the shell hook rewrites `cargo`/`git` to **rtk**, which
*summarizes* output and can corrupt exit codes and fmt/clippy diagnostics. For any verification
where precise output matters, use `rtk proxy <cmd>` (raw passthrough) or redirect to a file and
read it; capture exit codes with `; echo "exit=$?"` immediately after the command.

## Error handling

- **Agent error / no output:** retry once. If it fails again, proceed without that result, note
  the omission explicitly in the synthesis, and never fabricate the missing artifact.
- **Conflicting verdicts** (implementer GREEN but guardian FAIL): the **guardian wins** — it runs
  the real gates; GREEN is a claim, the gate output is evidence.
- **Loop caps:** design↔build retry once; build↔verify retry twice. Past the cap, stop and hand
  the open artifacts to the user rather than thrashing.
- Never resolve a failure by weakening an invariant, silencing a lint broadly, or adding a banned
  dep. Report the wall.

## Test Scenarios

**Happy path:** "Add an `envctl auto-fix --dry-run` summary line that counts components needing
repair." → Pre-flight: in worktree, no `_workspace/` → Initial run. Architect: engine-first plan
adding an `Engine` count method + `Event`, both front-ends render it, no invariant at risk → GO.
Implementer: adds the engine method + CLI/GUI wiring + a unit test, build GREEN. Guardian: all
three gates PASS, parity confirmed (CLI + GUI both call the new method), fail-closed N/A
(read-only) → PASS. Synthesis: summarize + commit `engine: add component-repair count to auto-fix
summary`.

**Error path:** Same request, but the implementer log returns `BLOCKED`: the count needs a new
dep that pulls a C SQLite. → Orchestrator does NOT let it proceed; routes back to the architect,
who revises the plan to compute the count from the existing pure-Rust engine state instead. Phase
2 re-runs GREEN. Guardian's `no-c.sh` PASSES because no banned dep was added. Demonstrates the
fail-closed routing and the loop cap.

## Phase 5: Follow-up & evolution

After a run, offer: "Anything to improve in the result, or in the crew/workflow itself?" Route
feedback per the harness rules — output-quality issues to the relevant skill, role gaps to an
agent definition, ordering issues to this orchestrator, missing triggers to a description. Record
every harness change in the `CLAUDE.md` change-history table. The whole harness is hand-authored
and git-tracked under `.claude/` (skills in `.claude/skills/`, agents in `.claude/agents/`) —
edit those files in place and commit; it is intentionally outside the kasetto pipeline.
