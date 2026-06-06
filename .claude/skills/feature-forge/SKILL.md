---
name: feature-forge
description: "The envctl Feature Forge orchestrator — the construction crew that designs, implements, and invariant-verifies a feature/upgrade end-to-end. ALWAYS use for any request to add/build/implement/design/upgrade/extend/refactor an envctl feature, Engine method, CLI/GUI surface, secrets-stack capability, or manifest component — and for FOLLOW-UPS: 're-run', 'run it again', 'revise the design', 'redo just the implementation', 'the guardian found X, fix it', 'improve the result', 'based on the previous plan'. Drives the feature-architect → rust-implementer → invariant-guardian pipeline. For CONTINUOUS/autonomous runs over a backlog ('keep building', 'loop on the roadmap', 'run until done', 'unattended') use the `forge-loop` skill; for cross-session handoff/'transfer'/'resume from handoff' use `session-relay`. Do NOT use for pure environment/toolchain install (use env-toolchain-install), drift/lock/doctor checks (use env-stabilize), or naming/convention questions (use agent-env-config)."
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
| Continuity | `continuity-steward` | general-purpose | no (writes checkpoint) | `_workspace/HANDOFF.md` |

The implementer follows the **`rust-feature-impl`** skill; the guardian runs that skill's
`references/verification.md` recipe. Conventions come from **`agent-env-config`**. The
`continuity-steward` is used only in **continuous mode** at a session handoff (see below).

## Single feature vs. continuous loop
- **One feature** (default): run Phases 0–4 below once and stop.
- **Continuous / autonomous over a backlog:** drive this same pipeline in a loop via the
  **`forge-loop`** skill (the Ralph loop) — each iteration runs one full cycle on the next backlog
  item, checkpoints, and self-paces. At a per-session **cycle budget**, `forge-loop` invokes
  **`session-relay`**, which spawns `continuity-steward` to write `_workspace/HANDOFF.md`, announces
  the transfer over **weave**, and schedules a **durable-cron** successor session to continue —
  keeping every session short and cheap (the defense against context rot + token burn). When asked
  to "keep building"/"loop"/"run unattended", start with `forge-loop`, not this skill directly.

## Phase 0: Pre-flight (always run first)

1. **Worktree.** This repo lives in the `meta` workspace. Confirm you are in an isolated worktree
   on a clean branch (`git status`), not a stale/dirty `master`. If not, create one
   (`meta git worktree create <slug> --all`, or `git worktree add ../envctl-<slug> -b <slug>`)
   before any mutation.
2. **Context check** — decide the run mode from `_workspace/`:
   - **`_workspace/HANDOFF.md` exists, or the request says "resume"/came from a relay cron/weave
     nudge → Resume:** hand control to the `session-relay` skill's RESUME protocol (read the
     checkpoint + weave inbox, verify baseline, ack, reset the per-session cycle counter), then
     continue the loop via `forge-loop`. Do not start a fresh pipeline.
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
- **GO** → proceed to Phase 1.5 (path selection), then build.

## Phase 1.5: Path selection (scale auto-trigger)

Between design and build, read the architect's **`## Target repos`** section (and its per-repo
module count) and route by scale. This is the auto-trigger — the orchestrator picks the build
shape; the default is unchanged.

- **1 repo & ≤3 modules → sequential single-crew (DEFAULT, unchanged).** Run Phase 2 once as
  today: one implementer, one guardian, in this worktree. This is the path for the overwhelming
  majority of features — nothing about it changes.
- **1 repo & >3 independent modules → intra-repo pipeline (or Option Y on shared files).** Split by
  whether the modules **share files**:
  - **Files disjoint** → existing intra-repo `Workflow.pipeline` (**Option X**, unchanged):
    `pipeline(modules, implement, verify)` — as the implementer finishes a module the guardian
    verifies *that module* while the next starts, so a late no-C violation is caught after one crate,
    not five. (No file collision, so no AST lock is needed.)
  - **Shared files + `FORGE_OPTION_Y=1`** → **Phase 2-Y** (intra-repo serialized merge, opt-in): real
    multi-writer parallelism on shared files — grit does its serialized AST-lock rebase+merge, gated by
    the guardian and `done`-called by the orchestrator (below).
  - **Shared files + default (`FORGE_OPTION_Y` unset)** → today's behavior: writers serialize on grit's
    AST `file::symbol` lock-queue under the single-crew / pipeline path (**Option X**) — unchanged.
- **>1 target repo → A2 cross-repo fan-out (Phase 2-A2 below).** One coordinated worktree set,
  one implementer per repo run concurrently, per-repo guardian gates. (Option Y is intra-repo only —
  irrelevant cross-repo.)

**Escape hatch:** `FORGE_PARALLEL=0` forces the sequential single-crew path regardless of scale
(and `FORGE_PARALLEL` *unset* leaves today's behavior intact — there is no opt-*in* required for
the default). If no `## Target repos` section is present, treat it as 1 repo ≤3 modules and run
sequentially.

**Precedence:** `FORGE_PARALLEL=0` **overrides** `FORGE_OPTION_Y` — no parallelism means no Option Y
(it routes to sequential single-crew). `FORGE_OPTION_Y` is **unset by default**, so **Option Y is never
auto-selected**; it is reachable only on the explicit opt-in above (shared files + `FORGE_OPTION_Y=1`).

## Phase 2: Build (rust-implementer)

Spawn `rust-implementer` with the plan path. It implements engine-first, wires CLI+GUI to parity,
adds tests, keeps the inner build loop green, and writes `_workspace/02_implementer_log.md` with
status `GREEN` / `BLOCKED`.

- **BLOCKED: plan defect** → route back to Phase 1 (architect revises the plan file), then
  re-run Phase 2. Retry the loop **once**; if it blocks again on design, escalate to the user
  with both artifacts.

## Phase 2-A2: Cross-repo parallel build

Run this **instead of** Phase 2 when Phase 1.5 routed to A2 (>1 target repo, `FORGE_PARALLEL`
not `0`). The three-owner split: **meta** owns the cross-repo worktree set (one independent
branch per repo → cross-repo edits can't conflict by construction), **grit** owns intra-repo
`file::symbol` locks (Option X — locks only), the **orchestrator** owns the guardian gate (only
it commits/merges/PRs, only after that repo's guardian PASSes — never `grit done`).

1. **Create the coordinated worktree set.**
   `meta git worktree create <slug> --repo <r1> --repo <r2> [--ephemeral --ttl 2d]`
   (repos are meta **aliases**, one `--repo` per repo; `--ephemeral --ttl 2d` self-cleans). The
   set lands at `.worktrees/<slug>/<repo>/`, one branch per repo.
2. **Namespace the artifacts per repo.** Use `_workspace/<repo>/` for each repo's
   `01_architect_plan.md` / `02_implementer_log.md` / `03_guardian_report.md` — the only
   structural change to the artifact protocol (it is flat in the sequential path).
3. **Init grit per repo (locks only).** Seed the whole worktree set in one shot with
   `meta git worktree exec <slug> --include <r1,r2> -- grit init` (or `grit init` in each repo
   worktree individually) — `grit init` is **idempotent** (a re-run just re-indexes symbols, exit
   0), so seeding is safe to repeat. For a one-time, box-wide seed of grit into **every** meta
   member repo (so the symbol index exists workspace-wide), use `meta exec -- grit init`. Then
   `grit gc` per repo (reap any dead claims). Option X: grit is used only for
   `init/claim/release/heartbeat/gc/status/queue` — never `done`/`session`/`worktree`.
4. **Spawn N implementers, one per repo, concurrently.**
   `Agent(general-purpose, model: opus, run_in_background: true, isolation: 'worktree')` pointed
   at `.worktrees/<slug>/<repo>/`, grit id `forge-<repo>`. Each runs the existing
   `rust-implementer` in its **Parallel mode** (claim → heartbeat → release → STOP at WORK; never
   `grit done`). They build in parallel and stop at green-and-released — they do not commit.
5. **Per-repo guardian gate (orchestrator-owned, descriptor-driven).** Spawn one `invariant-guardian`
   per repo. The guardian reads that repo's **invariant-contract descriptor** to learn which gates to
   run — gates are **data, not hardcoded**:
   - **Descriptor present** (`<repo-root>/.forge/invariants.toml`) → run its `[[gate]]` list **in
     order**; map results to the repo verdict per the descriptor's `required` flags (all pass → PASS;
     a `required` gate fails/errors → FAIL, fail-closed; only advisory gates fail → PASS-WITH-NOTES).
     envctl ships its own descriptor encoding exactly the 3 CI gates (`no-c`/`shape`/`enable`) **plus**
     `fmt`/`clippy`/`test`, so its behavior is identical.
   - **No descriptor** → run the **generic-Rust fallback** (`cargo fmt --all -- --check`,
     `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, all required) and emit a
     `NOTE: no .forge/invariants.toml — generic-Rust fallback used` (preserving the prior "flag the
     missing invariant contract" behavior). A repo adopts the full contract by adding its own
     `.forge/invariants.toml`; no edit to this harness or to other repos is needed.
6. **Commit/merge/PR — harness-owned, gated.** Only after a repo's guardian PASSes does the
   orchestrator commit that repo (area-prefixed subject) → **N commits / N PRs** (meta keeps
   independent histories; there is no single cross-repo commit). **Never** call grit `done`.
7. **Aggregate.**
   `meta --json git worktree exec <slug> --parallel --include <r1,r2> -- <verify>`
   returns structured per-repo `{directory, exit_code, stdout, summary}`; reduce the N exit codes
   to a pass/fail roll-up.
8. **Synthesize per repo.** Summarize each repo's result and preserve every `_workspace/<repo>/`
   audit trail (don't delete on success).

> **Cross-session resume of an in-flight A2 cycle** (N-branch checkpoint, meta-set recreate, grit
> reconcile, idempotent double-resume) is owned by **`session-relay` RESUME** (2-pre/2/2a/2b) +
> **`continuity-steward`** — see the per-repo HANDOFF table for the durable state. No behavior change
> to this phase; it just hands the in-flight per-repo vector to those skills at a budget handoff.

## Phase 2-Y: intra-repo serialized merge (opt-in, Option Y)

Run this **instead of** Phase 2 when Phase 1.5 routed to Option Y (**1 repo**, >3 modules that **share
files**, `FORGE_OPTION_Y=1`, `FORGE_PARALLEL` not `0`). This is the only path that gives real intra-repo
multi-writer parallelism on shared files.

**The inversion (vs Option X).** grit owns the **serialized rebase+merge** itself — `grit done -a <id>`
acquires `.grit/merge.lock` (serializes all merges; self-heals via `kill -0` + 30s mtime), rebases
`agent/<id>` onto the current branch, and `merge --no-ff agent/<id>` into the checked-out task branch.
So the **guardian gate is inverted to sit BETWEEN work and merge**: the orchestrator gates each
writer's pre-merge `.grit/worktrees/<id>` checkout, and **only the orchestrator calls `grit done`**
(after PASS) and then rewords the merge commit — **the implementer never calls `done`**.

**The wave loop (5 steps):**

1. **Init.** `grit init` (idempotent — a re-run just re-indexes, exit 0) + `grit gc` (reap dead claims).
2. **Spawn N implementers, NO isolation.**
   `Agent(general-purpose, model: opus, run_in_background: true)` — **without** `isolation: 'worktree'`
   (Option Y and the meta-worktree shapes are mutually exclusive). Each has grit id
   `forge-envctl-<module>`, claims its **disjoint** `file::symbol`s
   (`grit claim -a forge-envctl-<module> -i "<goal>" <file::symbol>... --with-deps --queue`), then
   **`cd .grit/worktrees/forge-envctl-<module>/`** (the `agent/<id>` branch grit created) and does all
   edits + builds **there**. It heartbeats long steps, releases (`grit release -a forge-envctl-<module>`),
   and **STOPs at WORK — it never calls `grit done`** (its Parallel-mode Option Y variant).
3. **Per-writer guardian gate.** Each GREEN+released writer → spawn one `invariant-guardian` pointed at
   that writer's `.grit/worktrees/forge-envctl-<module>/` checkout (it runs envctl's
   `.forge/invariants.toml` gate list against that pre-merge tree).
4. **PASS → assert branch → `grit done` → reword.** Before each merge, **assert the task branch is
   checked out** in the main envctl worktree:
   ```bash
   test "$(git rev-parse --abbrev-ref HEAD)" = "<task-branch>" || { echo "ABORT: wrong branch for grit done"; exit 1; }
   ```
   (Never use `grit session` — it `checkout -b grit/<name>`.) Then merge and **reword** (the `--no-ff`
   merge commit is HEAD after `done`):
   ```bash
   grit done -a forge-envctl-<module>
   git commit --amend -m "<area>: <module subject>

   Merged via grit Option Y (serialized AST-lock merge of agent/<id>).
   <why>

   Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
   ```
   The reword is **mandatory** (the inner `grit: agent <id>` / `grit: merge agent/<id>` commits stay as
   audit). A guardian **FAIL** → do **not** `done`; route the module back to the implementer (fix only
   the flagged surface) or BLOCK — the `agent/<id>` branch stays unmerged.
5. **Final cumulative guardian.** After all writers are merged-or-BLOCKED, run a **full
   `invariant-guardian` on the cumulative task branch** (the combined tree, not just one writer's
   pre-merge checkout) to confirm the merged result still PASSes every gate.

**Rules carried by this path:**
- **Implementers edit in `.grit/worktrees/<id>`, spawned WITHOUT `isolation:'worktree'`** — the grit
  worktree is the only working tree; do not create a meta worktree.
- **Conflict ⇒ BLOCKED, never forced.** `grit done` on a conflict does `merge --abort`, then still
  **reaps** the `agent/<id>` branch + worktree and releases the locks — so the agent's work survives
  only as a **recoverable dangling commit** (find it via `git reflog` / `git fsck --lost-found`), NOT
  as a live `agent/<id>` branch. **Detect** the conflict: the task-branch HEAD **did not advance** to a
  `grit: merge agent/<id>` commit. On detection, surface **BLOCKED** and stop — there is **no force or
  steal path**, and forcing would corrupt the serialized history. Recovery is re-claim + redo that
  module (the dangling commit is salvage, not a resume point).

Spawn `invariant-guardian` with the plan + implementer log. It runs the three CI gates,
`fmt`/`clippy`/`test`, the engine-purity / parity / fail-closed / drift / lock checks, and writes
`_workspace/03_guardian_report.md` with verdict **PASS / PASS-WITH-NOTES / FAIL**.

- **FAIL** → route blocking findings to the right agent: code-level findings → `rust-implementer`
  (fix only the flagged surface), plan-level findings → `feature-architect`. Re-run Phase 3 after
  the fix. Loop **at most twice**; if still failing, stop and report the open findings — never
  weaken a guard or invariant to force a pass.
- **PASS / PASS-WITH-NOTES** → proceed to synthesis.

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
