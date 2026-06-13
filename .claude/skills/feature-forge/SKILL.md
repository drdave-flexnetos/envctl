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
| Design | `feature-architect` | Plan | no | `.handoff/loop/cycle/01_architect_plan.md` |
| Build | `rust-implementer` | general-purpose | **yes** | code + `.handoff/loop/cycle/02_implementer_log.md` |
| Build (Epic A) | `handoff-kernel-engineer` | general-purpose | **yes** | hf/`.handoff` substrate + `.handoff/loop/cycle/02_implementer_log.md` |
| Verify | `invariant-guardian` | general-purpose | no | `.handoff/loop/cycle/03_guardian_report.md` |
| Continuity | `continuity-steward` | general-purpose | no (writes checkpoint) | `.handoff/loop/HANDOFF.md` |

The implementer follows the **`rust-feature-impl`** skill; the guardian runs that skill's
`references/verification.md` recipe. Conventions come from **`agent-env-config`**. The
`continuity-steward` is used only in **continuous mode** at a session handoff (see below).

**Epic A routing (handoff full-sync / hf kernel).** When the item's scope is building/relocating the
`hf` kernel or seeding the Tier-A `.handoff` layer (backlog Epic A, TASK-0001…0003), the Build phase
uses **`handoff-kernel-engineer`** instead of `rust-implementer` — it follows the **`handoff-sync`**
skill and owns the kernel invariants (single shared ledger at `$META_ROOT/.handoff/ledger.db`,
packets-rendered-never-hand-written, p7-conformance). The guardian additionally verifies those kernel
invariants for Epic-A cycles. This is a **cross-repo** job (`meta/handoff` ↔ envctl), which is why it
is a distinct agent from the envctl-engine-first `rust-implementer`.

**Epic C routing (kasetto absorption / agent-env).** When the item's scope is the agent-env crate
or kasetto absorption (backlog Epic C, TASK-0011…0018), route **all three** crew members at
`rust-feature-impl`'s `references/kasetto-absorption.md` — the no-downgrade playbook (11 kasetto
verbs incl. v3.1 add/remove/lock; the 11→6 verb mapping; drop-mimalloc; SHA-256 agent-asset lock
alongside the untouched FNV-1a component lock; additive/never-clobber MCP merge that preserves
global broker/repowire/weave). The architect plans against it, the implementer builds against it,
and the guardian asserts the no-downgrade checklist. Skipping it silently drops v3.1+ kasetto
features.

## Single feature vs. continuous loop
- **One feature** (default): run Phases 0–4 below once and stop.
- **Continuous / autonomous over a backlog:** drive this same pipeline in a loop via the
  **`forge-loop`** skill (the Ralph loop) — each iteration runs one full cycle on the next backlog
  item, checkpoints, and self-paces. At a per-session **cycle budget**, `forge-loop` invokes
  **`session-relay`**, which spawns `continuity-steward` to write `.handoff/loop/HANDOFF.md`, announces
  the transfer over **weave**, and schedules a **durable-cron** successor session to continue —
  keeping every session short and cheap (the defense against context rot + token burn). When asked
  to "keep building"/"loop"/"run unattended", start with `forge-loop`, not this skill directly.

## Phase 0: Pre-flight (always run first)

1. **Worktree.** This repo lives in the `meta` workspace. Confirm you are in an isolated worktree
   on a clean branch (`git status`), not a stale/dirty `master`. If not, create one
   (`meta git worktree create <slug> --all`, or `git worktree add ../envctl-<slug> -b <slug>`)
   before any mutation.
2. **Context check** — decide the run mode from `.handoff/loop/`:
   - **`.handoff/loop/HANDOFF.md` exists, or the request says "resume"/came from a relay cron/weave
     nudge → Resume:** hand control to the `session-relay` skill's RESUME protocol (read the
     checkpoint + weave inbox, verify baseline, ack, reset the per-session cycle counter), then
     continue the loop via `forge-loop`. Do not start a fresh pipeline.
   - No `.handoff/loop/cycle/` → **Initial run** (full pipeline).
   - `.handoff/loop/cycle/` exists + user asks for a *partial* change ("redo just the implementation",
     "fix the guardian's findings") → **Partial re-run**: re-invoke only the relevant agent(s),
     feeding them the existing artifacts.
   - `.handoff/loop/cycle/` exists + a *new, unrelated* feature → **New run**: archive the old loop
     artifacts to `.handoff/loop/_done/<slug>.<UTC-date>.*` via `git mv` (preserves history, matches
     the existing `_done/` convention), then start fresh.
3. **Scope the request.** If it's a one-line question or a trivial typo, answer/do it directly —
   don't spin up the crew for something that doesn't need it.

**hf-aware context check.** When `hf` is on PATH, the context check is hf-aware: pick the next item
via `hf resume --json` (its dep-DAG `next_task_id`/`next_command` picker) rather than re-deriving
order from the markdown backlog; the markdown-checkbox read is the fallback only when `hf` is
absent. (Per-cycle hf verb details are owned by `forge-loop` — do not duplicate them here.)

## Phase 1: Design (feature-architect)

Spawn `feature-architect` with the verbatim request. It reads the code (code-intelligence, not
grep), the relevant `docs/`, and verifies external APIs against primary sources. The `Plan` agent
type is **read-only and cannot Write**, so the architect **returns** the plan as text and **you
(the orchestrator) persist it** to `.handoff/loop/cycle/01_architect_plan.md`. Read its leading
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
- **1 repo & >3 independent modules → intra-repo pipeline.** Model it with the `Workflow` tool:
  `pipeline(modules, implement, verify)` — as the implementer finishes a module the guardian
  verifies *that module* while the next starts, so a late no-C violation is caught after one
  crate, not five. grit AST-locks (`file::symbol`) only come into play if the modules share files.
- **>1 target repo → A2 cross-repo fan-out (Phase 2-A2 below).** One coordinated worktree set,
  one implementer per repo run concurrently, per-repo guardian gates.

**Escape hatch:** `FORGE_PARALLEL=0` forces the sequential single-crew path regardless of scale
(and `FORGE_PARALLEL` *unset* leaves today's behavior intact — there is no opt-*in* required for
the default). If no `## Target repos` section is present, treat it as 1 repo ≤3 modules and run
sequentially.

**hf-aware routing + the kasetto meta-source-up-then-absorb case (Epic C).** Phase-1.5 routing is
hf-aware: when `hf` is present, honor the dep order it reports. The kasetto absorption case that
spans envctl + `meta/kasetto` (sync the meta kasetto source UP first) is an **intra-cycle ORDERED
A2** — not concurrent: sync the meta/kasetto **source up to ≥3.1.0 FIRST, guardian-gated**, and only
then does envctl absorb (the envctl-absorb sub-item is `blocked_by` the source-up sub-item).
Namespace the per-repo artifacts under `.handoff/loop/{kasetto,envctl}/`. (The forge-loop owns the
per-cycle verb sequence — do not duplicate it here.)

## Phase 2: Build (rust-implementer)

Spawn `rust-implementer` with the plan path. It implements engine-first, wires CLI+GUI to parity,
adds tests, keeps the inner build loop green, and writes `.handoff/loop/cycle/02_implementer_log.md` with
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
2. **Namespace the artifacts per repo.** Use `.handoff/loop/<repo>/` for each repo's
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
5. **Per-repo guardian gate (orchestrator-owned).** Spawn one `invariant-guardian` per repo:
   - **envctl** → the full gate: the 3 CI gates (`no-c`/`shape`/`enable`) **plus** `fmt` /
     `clippy` / `test`.
   - **non-envctl Rust repo** → no envctl gate set exists, so **degrade** to `fmt` / `clippy` /
     `test` and flag the missing invariant contract (PR-1 demonstrated scope = envctl-style Rust
     repos; portable per-repo gate descriptors are staged to PR-2).
6. **Commit/merge/PR — harness-owned, gated.** Only after a repo's guardian PASSes does the
   orchestrator commit that repo (area-prefixed subject) → **N commits / N PRs** (meta keeps
   independent histories; there is no single cross-repo commit). **Never** call grit `done`.
7. **Aggregate.**
   `meta --json git worktree exec <slug> --parallel --include <r1,r2> -- <verify>`
   returns structured per-repo `{directory, exit_code, stdout, summary}`; reduce the N exit codes
   to a pass/fail roll-up.
8. **Synthesize per repo.** Summarize each repo's result and preserve every `.handoff/loop/<repo>/`
   audit trail (don't delete on success).

## Phase 3: Verify (invariant-guardian)

Spawn `invariant-guardian` with the plan + implementer log. It runs the three CI gates,
`fmt`/`clippy`/`test`, the engine-purity / parity / fail-closed / drift / lock checks, and writes
`.handoff/loop/cycle/03_guardian_report.md` with verdict **PASS / PASS-WITH-NOTES / FAIL**.

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

**File-based** via the `.handoff/loop/cycle/` folder at the worktree root, naming `NN_agent_artifact.md`
(`01_architect_plan.md`, `02_implementer_log.md`, `03_guardian_report.md`). Pass artifact **paths**
to each agent, not their full contents. The code itself is the implementer's primary output (in
the worktree); `.handoff/loop/` is the audit trail — preserve it, don't delete it on success.
**Return-value-based** for each agent's headline verdict (the one-line status it returns to you) —
and note that the **architect (`Plan` type) is read-only**, so it returns its plan as text and you
persist `.handoff/loop/cycle/01_architect_plan.md` for it; the implementer and guardian (`general-purpose`) write their
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

## Parallel mode (opt-in grit git-lock coordination)

When multiple `rust-implementer` agents must write across the same meta workspace, use **grit**
(opt-in, not the default). The default single-implementer path is unchanged.

To activate: set `USE_GRIT=1` before spawning implementers. This adds a pre-lock / post-unlock step
to every implementer spawn.

### Activation prerequisites

1. **grit installed box-wide** (DONE — FlexNetOS/grit via `cargo install --path`, in `~/.cargo/bin`).
   - If not yet on PATH: `meta exec -- grit init` to seed all meta repos idempotently.
2. **Opt-in only:** the skill works identically when `USE_GRIT` is unset — verify with a smoke run
   in non-parallel mode after changes land.

### How parallel mode modifies the build phase (Phase 2)

When `USE_GRIT=1`:

1. **Before** spawning any implementer, initialize grit per target repo:
   ```bash
   for repo in . meta_cli loop_lib; do cd /home/drdave/Desktop/meta/$repo && grit init -y; done
   ```
   (`grit init` is idempotent — safe to run repeatedly.)

2. **Per implementer spawn**, before writing any code:
   ```bash
   grit claim file::symbol --with-deps  # e.g. "crates/engine/src/lib.rs::Engine::dashboard"
   ```
   If the symbol is already claimed → `grit claim file::symbol --queue` (enqueue, waits for turn).

3. **After** the implementer's commit (only if claims succeeded):
   ```bash
   grit done file::symbol  # release the lock; other implementers queued on this symbol proceed
   ```

4. **Cross-repo writes:** use `--with-deps` to transitively claim a symbol and all its dependents.
   For writes spanning multiple repos, use `grit claim --with-deps file::symbol`.

### Constraints (always enforced)

- grit is an **external TOOL binary**, NOT a crate dependency. It stays outside envctl's no-C trust boundary.
- Parallel mode is **opt-in** via `USE_GRIT=1`. Default path (no env var) unchanged.
- CLI-only usage: `grit` runs via bash/subprocess from the orchestrator or implementer agent.
- If `grit` binary is absent, skip parallel gracefully with a warning: *"grit not on PATH — falling back to single-implementer"*.

### Acceptance criteria for this change

- [x] `feature-forge`/`forge-loop` skills document + support opt-in grit parallel mode.
- [x] Idempotent `grit init` (per-repo + `meta exec -- grit init`) wired into the flow.
- [x] Default (non-parallel) path unchanged and still passes a smoke run.
- [ ] No new Rust dep on grit; no-C gate still green. (verified after commit)
- [ ] CLAUDE.md harness change-history row added.

## Test Scenarios

**Happy path:** "Add an `envctl auto-fix --dry-run` summary line that counts components needing
repair." → Pre-flight: in worktree, no `.handoff/loop/cycle/` → Initial run. Architect: engine-first plan
adding an `Engine` count method + `Event`, both front-ends render it, no invariant at risk → GO.
Implementer: adds the engine method + CLI/GUI wiring + a unit test, build GREEN. Guardian: all
three gates PASS, parity confirmed (CLI + GUI both call the new method), fail-closed N/A
(read-only) → PASS. Synthesis: summarize + commit `engine: add component-repair count to auto-fix
summary`.

**Parallel path:** "Implement dashboard KDL renderer AND secrets-engine vault migration in parallel."
→ Pre-flight: in worktree, `USE_GRIT=1`, no `.handoff/loop/cycle/` → Initial run with parallel mode. Architect: engine-first
plan identifying two independent Engine methods (dashboard KDL + vault migration). Implementer 1:
`grit init` meta repos → `grit claim file::symbol crates/engine/src/dashboard.rs::render` → writes
KDL renderer. Implementer 2: `grit claim file::symbol crates/secrets-engine/src/vault.rs::migrate`
→ writes migration. Both commit. Guardian: all three gates PASS, no-C green, parity confirmed → PASS.
Synthesis: summarize + commit `engine: dashboard KDL renderer + secrets-engine vault migration`.

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
