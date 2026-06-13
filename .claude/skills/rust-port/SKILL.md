---
name: rust-port
description: >-
  Packaged Rust-port harness (invoked as /harness:rust-port). Runs an autonomous, resumable loop
  that performs a FULL-FEATURE, NO-DOWNGRADE port of a source project (TypeScript/Python/etc.) to
  idiomatic Rust — no feature logic left behind. ALWAYS use for: "port <project> to Rust", "rust
  port", "rewrite in Rust", "full-parity Rust port", "port meta/Archon to Rust", AND follow-ups —
  "resume", "continue the port", "run it again", "re-run", "redo only the <unit/phase>", "based on
  the previous result", "what's left to port". Also ejectable: "install/eject the rust-port harness
  into <repo>". ALSO does port-and-MERGE: "port <X> to Rust and merge into <Y>", "merge the rust code
  into <repo>", "reconcile the port with <repo>" — ports X then integrates each verified unit into a
  destination repo Y (re-verified in Y). Runs an automated 3-model workflow (opus on gates/hard design,
  sonnet on structured work, haiku on mechanical). Drives a Ralph loop over a parity ledger: one unit
  per cycle, differential parity test, commit per cycle, hand off at budget. DONE only at 100% parity
  (+ 100% merged when a destination repo is set) — nothing left behind.
---

# rust-port — full-feature, no-downgrade Rust port harness  (`/harness:rust-port`)

Leader skill of the **rust-port** packaged harness (in the `harness` plugin). It ports a source
project to **idiomatic Rust with zero capability loss**: every module, behavior, error path, and
edge case in the source is inventoried, ported, and **differentially verified** against the source
before it counts as done. The guarantee is **no feature logic left behind** — enforced structurally
by a parity ledger that must reach 100%, not by good intentions.

It is **packaged + runnable + ejectable**: run in place via `/harness:rust-port`, or eject into the
target/port repo's `.claude/` (see §Eject). Built on the FlexNetOS autonomous-operation pattern:
**truth lives on disk**, every cycle commits, any restart resumes cold with zero loss.

## Execution mode — Hybrid (sub-agent + file-based), and why

Single-orchestrator with specialist sub-agents, coordinated **file-based** under `.handoff/loop/`
(durable) + return-values. Not a live team: team state dies at the self-restart boundary, and this
loop's premise is that state (the parity ledger) survives a fresh process. Per phase:

| Phase | Mode | Shape |
|-------|------|-------|
| Discover / inventory | Sub-agent | cartographer → ledger+symbol-map; architect → target design; researcher → X⟷Y reuse map; cross-repo-referencer → reference map (parallel-capable) |
| Port (per cycle) | Sub-agent, sequential | one unit → porter → build-health → parity-verifier |
| Merge (per cycle, when `dest_repo` Y set) | Sub-agent, sequential | merge-integrator → build-health(Y) → parity-verifier re-verify in Y |
| Handoff | Sub-agent | continuity-steward writes HANDOFF.md |

### Model tiering — the automated 3-model workflow (opus / sonnet / haiku)

`Agent` calls are tiered by task, NOT all-opus — but **every no-downgrade GATE runs at `opus`** and is
never tiered down. Tiering a *worker* down is safe **because the opus parity gate catches any worker
downgrade** (a sonnet porter that drops a branch is FAILed and bounced back, never shipped) — so
tiering is a cost/speed lever, never a correctness downgrade.

- **opus** (gates + hard design): `rust-port-architect`, `rust-port-parity-verifier`,
  `rust-port-merge-integrator`, `evolution-steward`, and the `rust-port-cartographer` **pre-DONE
  left-behind sweep** (the completeness gate — overridden to opus even though the agent defaults lower).
- **sonnet** (structured work): `rust-port-cartographer` (inventory), `rust-port-porter`,
  `rust-port-researcher`, `continuity-steward`.
- **haiku** (mechanical): `build-health-auditor`, `rust-port-cross-repo-referencer`.

**Escalation:** a tiered worker that hits reasoning beyond its tier (intricate concurrency, an
ambiguous reconciliation) says so and the orchestrator re-runs it at the next tier up — never guess.
Shared agents (`build-health-auditor`/`continuity-steward`/`evolution-steward`) keep their `opus`
frontmatter; this orchestrator overrides their model per-call (scope law — don't edit shared defaults).

## Agents (in the plugin's shared `harness/agents/` pool)

| Agent | Owns | Shared? |
|-------|------|---------|
| `rust-port-cartographer` | exhaustive source inventory + parity ledger + left-behind sweep | specialist |
| `rust-port-architect` | Rust target layout + idiom map + dependency equivalents | specialist |
| `rust-port-porter` | full (no-stub) idiomatic port of one unit | specialist |
| `rust-port-parity-verifier` | differential parity proof (source vs Rust, + re-verify in Y) | specialist |
| `rust-port-merge-integrator` | merge a verified unit into destination repo Y (no-downgrade) | specialist |
| `rust-port-researcher` | deep research/discovery of X **and** Y (the reuse map) | specialist |
| `rust-port-cross-repo-referencer` | cross-repo reference + blast-radius map (X⟷Y⟷substrates) | specialist |
| `build-health-auditor` | cargo build/clippy/test green gate | shared |
| `continuity-steward` | cold-start HANDOFF.md at budget | shared |
| `evolution-steward` | evaluates each run, mines lessons, upgrades the harness (runs last) | shared |

Skills: `rust-port-inventory`, `rust-port-translate`, `rust-port-parity`, `rust-port-merge`,
`cross-repo-reference`, `cross-repo-health`, `icm-memory`, `session-relay-wrap-up`, `session-relay-resume`,
`harness-loop-init`, `harness-evolution` (research reuses `code-research-map`/`-analyze` + `deep-research`).

**Persistent memory (any agent, as needed):** every agent has the **`icm-memory`** skill available to
**recall** relevant prior context before acting and **store** durable memory when it learns something
(a decision, a resolved conflict, a parity gotcha, a completed unit). It is *not* a forced step — the
lead delegates *which* agent uses it *when* at runtime (graceful no-op where ICM is absent, so the
harness stays portable). This complements the loop-boundary `session-relay-wrap-up`/`-resume` ICM
store/recall with finer, in-cycle agent memory.

## Agent runtime (the declarative execution contract)

The consolidated, per-agent runtime spec for this harness. It is the single source of truth for *how*
each agent is run (it subsumes the scattered facts — the `model: "opus"` rule, the phase/mode table
above, the retry/fail-closed rule, the self-pace cadence). **This table is the declarative contract
the `harness-agent-rs` runtime (ADR-0001) will consume to execute the harness**, so every cell is
meant to be precise and machine-translatable — model, phase, concurrency, precondition, timeout/retry,
I/O files, and gate-role are the fields a runtime needs to schedule, isolate, and gate each run. Keep
it in sync with the agent defs in `harness/agents/` and the per-row contracts they declare.

| Agent | Model | Runs-in (phase) | Concurrency | Trigger / precondition | Timeout & retry (fail-closed) | Inputs (reads) | Outputs (writes) | Gate-role |
|-------|-------|-----------------|-------------|------------------------|-------------------------------|----------------|------------------|-----------|
| `rust-port-cartographer` | sonnet · **opus** (pre-DONE sweep = gate) | P1 DISCOVER (seed); DONE gate (left-behind sweep) | parallel-capable with `rust-port-architect` at DISCOVER; sequential at the sweep | initial run or new source/scope; and once more pre-DONE | retry once; on 2nd failure → `- [!]` blocked row + continue, never fake coverage; empty symbol harvest of non-empty source → `NEEDS-HUMAN` | source root; prior `parity-ledger.md`/`symbol-map.md` | `parity-ledger.md` + `symbol-map.md` (authoritative), `reports/inventory.md` | **completeness critic** — its two-grain sweep (zero unlisted units/symbols, zero `- [ ]`/`- [~]`, zero rollup violations) is a hard DONE precondition |
| `rust-port-architect` | opus | P1 DISCOVER (layout); P2 ITERATE (only when a unit needs a new structural decision) | parallel-capable with cartographer at DISCOVER; on-demand, single-flight per unit otherwise | DISCOVER, or a porter/orchestrator structural question | retry once; unresolved equivalent → record options + surface to orchestrator, never pick/drop silently | source root; `parity-ledger.md` | `target-architecture.md` (layout + idiom map + dep table + port-and-map decisions) | advisory — establishes the no-downgrade idiom/dep + reimplement-vs-map-onto mapping the porter must follow; not a pass/fail gate |
| `rust-port-porter` | sonnet (escalate hard units → opus) | P2 ITERATE (the per-cycle worker) | **sequential, exactly one per cycle** (the one-specialist-per-cycle rule) | a picked unit whose `deps:` are all `- [x]` | retry once; if not finished → unit `- [~]`/`- [!]` + the specific `symbol-map.md` rows `- [ ]`/`- [!]`, never `- [x]` | unit's ledger + symbol rows; `target-architecture.md`; source file; `rust-port-translate` | Rust source + tests in the target crate; unit + symbol rows → `- [~]` | produces the artifact under test; its claim is **never self-certified** (verifier + auditor gate it) |
| `build-health-auditor` | haiku (per-call override; shared frontmatter stays opus) | P1 DISCOVER (skeleton baseline); P2 ITERATE (post-port compile gate); MERGE (Y green) | sequential, after the porter in a cycle | a freshly ported unit, or the DISCOVER baseline / verify-on-resume | retry once; environmental failure (toolchain/network) → `skip` w/ reason, never silent pass | target repo set; the porter's new code; (merge) Y + Y's test suite | `findings/health.md`; `baseline.md`; (merge) `findings/y-regression.md` (Y baseline captured at DISCOVER) | **green-build gate** — `cargo build` + `clippy -D warnings` (+ `test`) must pass; precondition for the parity gate; captures Y's regression baseline for the dual gate |
| `rust-port-parity-verifier` | opus | P2 ITERATE (the per-cycle gate) | sequential, after build-health-auditor | a unit that compiles + passes clippy | retry once; can't run one side → `INCONCLUSIVE`, unit stays open (never pass on faith) | unit's ledger + symbol rows; source unit; Rust impl; `target-architecture.md` | `findings/parity.md` (verdict + diff); per-symbol `- [x]` in `symbol-map.md`; golden fixtures | **the no-downgrade gate** — only a `PASS` (every contract branch matches **and** all the unit's symbols `- [x]`/`- [≠]`) lets the orchestrator mark `- [x]` |
| `rust-port-merge-integrator` | opus | MERGE (per cycle, when `dest_repo` Y set) | sequential, in the Y worktree (`dest_worktree`/`dest_branch`); one per cycle | a verified/`reuse-Y`/`map-onto` unit + a `dest_repo` Y | retry once; unresolvable conflict / substrate can't express a behavior → `- [!]`/`- [≠]` + route up, never drop a side; any gate fail → `reset --hard` + release locks + `- [~]` (atomic) | ported Rust (if any); `merge-ledger.md`; `reports/{research,cross-repo-refs}.md`; Y worktree | merged Rust on `dest_branch`; `merge-ledger.md`; `findings/merge.md` | **bidirectional no-downgrade gate** — re-verify vs X **+ Y-not-regressed** + Y green; reuse>duplicate-never-by-narrowing; atomic (commit iff all pass) |
| `rust-port-researcher` | sonnet (escalate ambiguous verdicts → opus) | P1 DISCOVER; per-unit on demand | parallel-capable at DISCOVER | initial run / a unit needing external or Y-context research | retry once; inconclusive → record open question + evidence, never assume | source X, dest Y, docs/web, `git-kb code` | `reports/research.md` (X-needs ⟷ Y-provides reuse map) | advisory — feeds reuse-vs-reimplement + map-onto-Y; read-only, not a pass/fail gate |
| `rust-port-cross-repo-referencer` | haiku (escalate reconciliation judgment → higher tier) | P1 DISCOVER (seed map); MERGE (refresh touched symbols) | parallel-capable at DISCOVER; per merge cycle otherwise | a merge run (`dest_repo` Y set) | retry once; empty graph for non-empty repo → re-index; still empty → `INCONCLUSIVE`, never "no references" | X, Y, substrate repos; `symbol-map.md`; `.meta.yaml`; `git-kb code`, `meta` | `reports/cross-repo-refs.md` (per-symbol blast radius + lock scope) | advisory — supplies blast radius + contract-compat flags the merge-integrator gates on; mechanical collection |
| `continuity-steward` | sonnet (per-call override; shared frontmatter stays opus) | P3 HAND OFF (at budget) | sequential, single-flight at the budget boundary | `cycles_this_session >= cycle_budget` (or STOP) | retry once; missing `baseline.md` → reconstruct verify-on-resume block + note it | `parity-ledger.md`, `symbol-map.md`, `loop_state.md`, `baseline.md`, session commit list | `HANDOFF.md` (state + pointers, the authoritative resume signal) | continuity gate — writes the cold-resume contract; no fake DONE may substitute for it |
| `evolution-steward` | opus | Phase E (runs **last** — at DONE full retro, at HAND OFF lightweight) | sequential, single-flight at the run boundary (never mid-cycle) | end of run (DONE or HAND OFF) | retry once; thin artifacts → evaluate what exists + record the gap as its own lesson | `.handoff/loop/` artifacts; CLAUDE.md change history; `LESSONS.md` | `evaluation.md`; `proposed-upgrades.md`; lessons-ledger rows; applied PR edits | **gate-strengthener only** — may evaluate and *strengthen* the parity/DONE gate, never weaken it (scope law) |

**Loop-level runtime (the schedule the runtime drives):**

- **Self-pacing** — after each committed cycle the orchestrator re-enters via `ScheduleWakeup` (P2 step 6); at HAND OFF/DONE it stops with **no** `ScheduleWakeup` and lets exactly one terminal sentinel (`DONE` / `NEEDS-HUMAN` / `HANDOFF.md`) drive the external runner.
- **Cycle budget** — `cycle_budget` (default `3`, in `loop_state.template.md`) cycles per session; `cycles_this_session` resets to `0` on RESUME. Hitting the budget routes to P3 HAND OFF, not a stop-and-ask.
- **Context budget** — a session runs cycles continuously to a ~50% context budget rather than stopping per item; only a genuine wall (`NEEDS-HUMAN`) or the cycle budget halts it.
- **Commit per cycle** — every ITERATE cycle commits one unit with its `.handoff/loop/` state (`port(<crate>): <unit> — parity verified`); truth lives on disk so any restart resumes cold with zero loss.
- **One specialist per cycle** — exactly one specialist (the porter) runs per ITERATE cycle, gated by the auditor then the verifier; the architect runs only on a structural question. This bounds coordination to a single sequential chain per cycle and is what makes the loop machine-schedulable.

## Phase 0: Context check (initial / resume / partial)

- `.handoff/loop/HANDOFF.md` exists + user says resume/continue → **RESUME** via
  `session-relay-resume` (ICM recall → weave inbox scan → read committed HANDOFF → verify-on-resume
  baseline, fail-closed → broadcast `relay:resumed` → reset `cycles_this_session=0`), continue at the
  ledger's next `- [ ]`/`- [~]` unit.
- `.handoff/loop/` exists + user asks to redo one unit/phase → **PARTIAL**: re-run only that unit.
- `.handoff/loop/` exists + new source/scope → **NEW RUN**: move old to `.handoff/loop_prev/`.
- absent → **INITIAL**.

The orchestrator must know the **source root** (the project being ported) and the **Rust target
crate/dir**, and — for a **port-and-merge** run — the **destination repo Y** (`dest_repo`). Ask once if
not given (default `dest_repo: none` = port-only); record all in `loop_state.md`. When `dest_repo` is
set, the ITERATE cycle gains the MERGE step and DONE adds the merge conditions, and the orchestrator also
records `dest_branch`/`dest_worktree`/`dest_base` (Y is a separate repo — all Y writes happen in a
per-task **worktree** on a **feature branch**, never on Y's main; see `rust-port-merge` §Y git discipline).
**On RESUME of a merge run:** fetch Y, rebase `dest_branch` onto `dest_base`, re-index Y, and re-run the
cross-repo-referencer over the merged set — any `- [x]` merged unit whose Y blast-radius drifted drops to
`- [~]` for re-verification (Y is mutable; a merge proven against an old Y isn't proven against the new Y).

## Phase 1: DISCOVER (initial run)

1. Seed `.handoff/loop/loop_state.md` (template in `scripts/`) with source root + Rust target + UTC start.
2. `rust-port-cartographer` → `.handoff/loop/parity-ledger.md` (every source unit, all `- [ ]`)
   **and `.handoff/loop/symbol-map.md`** (every source symbol — fn/type/method/field/const/variant/
   trait/CLI flag/route — harvested deterministically via `git kb code symbols --json --limit -1`,
   all `- [ ]`, each `unit:`-tagged). See `references/symbol-map.md`.
3. `rust-port-researcher` → `.handoff/loop/reports/research.md` (deep research of X **and**, when
   `dest_repo` Y is set, Y — the X-needs ⟷ Y-provides **reuse map** so the port maps onto what Y
   already has instead of duplicating it). Reuses `code-research-*` + `deep-research`.
4. `rust-port-architect` → `.handoff/loop/target-architecture.md` (crate layout, idiom map, deps, and
   — informed by the reuse map — the per-unit port-and-map / reuse-vs-reimplement decisions). **When Y
   is set, also record each unit's merge `class`** (`port-fresh`/`extend-Y`/`reuse-Y`/`map-onto-substrate`)
   from the reuse map — this drives ITERATE (reuse-Y/map-onto skip the fresh port). See `references/merge-ledger.md`.
5. **When `dest_repo` Y is set:** create the Y **worktree** on a fresh **feature branch**
   (`dest_worktree` on `dest_branch` off `dest_base`); `rust-port-cross-repo-referencer` →
   `.handoff/loop/reports/cross-repo-refs.md` (cross-repo reference + blast-radius map across
   X⟷Y⟷substrates) and seed `.handoff/loop/merge-ledger.md` (one `- [ ]` row per unit, `class`-tagged).
6. `build-health-auditor` → confirm the Rust target skeleton builds (baseline) **and, when Y is set,
   that Y builds AND is runnable, and that every substrate the map-onto decisions target
   (`hf`/`weave`/`grit`/`icm`) is actually present in Y's Cargo workspace** → `.handoff/loop/baseline.md`.
   **When Y is set, also capture Y's own behavioral baseline** (Y's existing test suite + golden fixtures
   for the symbols in each unit's Y blast-radius) → `.handoff/loop/findings/y-regression.md` — the
   reference the merge's dual no-downgrade gate diffs against.
   **Also confirm the SOURCE is runnable** — the differential parity-verifier's hard precondition is
   that it can *execute the source* (its `source_toolchain`: bun/node/python). Smoke-run the source
   (or its test suite) once here; if the source — or, for a merge, **Y** or a **named substrate** — can't
   be executed/found, the gate can never produce a `PASS` (every unit `INCONCLUSIVE`), so record
   `.handoff/loop/NEEDS-HUMAN` now and stop — fail fast at DISCOVER, not per-unit forever.
7. Order the ledger by dependency (leaf modules / pure functions first; entrypoints last). See
   `references/parity-ledger.md`. Commit ledger + symbol-map + state + architecture + research +
   (when Y set) cross-repo-refs + merge-ledger.

## Phase 2: ITERATE (one unit per cycle)

1. Read `loop_state.md` + `parity-ledger.md`.
2. Stop checks: no `- [ ]`/`- [~]` left → go to **DONE gate**; `cycles_this_session >= cycle_budget`
   → **HAND OFF**; `.handoff/loop/STOP` present → stop.
3. Pick the top unported unit whose dependencies are `- [x]`.
4. **Branch on the unit's merge `class`** (port-only runs treat every unit as `port-fresh`):
   - `port-fresh` / `extend-Y` → **Architect** (only if a new structural decision) → **porter** ports it
     FULLY (no stubs, every branch — see `rust-port-translate`) → **build-health-auditor** (compiles + clippy).
   - `reuse-Y` / `map-onto-substrate` → **skip the porter** (porting then discarding what Y/substrate
     already provides is the wasted work the classification removes). Go straight to the parity gate,
     which differentially verifies **Y's existing symbol (or the substrate) against source X**. If it
     diverges, Y was only *partial* → reclassify `extend-Y` and port the missing behavior.
5. **Parity gate** — `rust-port-parity-verifier` runs the differential test (source vs Rust over the
   unit's whole contract, **exercising every symbol of the unit** in `symbol-map.md`). A unit `PASS`
   requires every contract behavior to match **and all the unit's symbols to be `- [x]`/`- [≠]`**
   (rollup rule) → mark the unit `- [x]`. Any unverified symbol or divergence → leave `- [~]`/`- [!]`
   with the exact missing behavior + symbol id; do NOT commit a fake `- [x]`. **A dropped symbol or
   downgrade never passes the gate.**
6. **MERGE step (only when `dest_repo` Y is set)** — for the unit just marked `- [x]`, **in the Y
   worktree (`dest_worktree` on `dest_branch`)**: `rust-port-cross-repo-referencer` refreshes the touched
   symbols' references → `rust-port-merge-integrator` integrates the unit into Y (class-driven landing,
   dup-scan before a new module, reuse>duplicate-never-by-narrowing, grit symbol-lock, breaking-contract
   *resolved* not just flagged — see `rust-port-merge`) → `build-health-auditor` (Y compiles/clippy/test)
   → `rust-port-parity-verifier` **re-runs the differential gate in Y's context** (still matches source X,
   every symbol) **AND the Y-regression diff** against Y's captured baseline (Y's own behavior preserved).
   **Atomic:** only a re-`PASS` **+ Y green + Y-not-regressed** commits the unit's Y changes to
   `dest_branch` and marks `- [x]` in `merge-ledger.md`; any failure → `git -C <dest_worktree> reset
   --hard`, release grit locks, keep `- [~]`. (Skip this step entirely when `dest_repo: none`.)
7. Write ledger(s) back, bump counters, **commit** the `.handoff/loop/` state (`port(<crate>): <unit> —
   parity verified`, or `port+merge(<crate>): <unit> — verified in Y` when merged). A merge cycle thus
   makes **two commits**: the port-repo state commit and the Y-branch commit (step 6). Self-pace
   (`ScheduleWakeup`).

## Phase 3: HAND OFF (at budget)

Invoke **`session-relay-wrap-up`** — the full wrap-up: stop-checks → Phase E lightweight retro
(`evolution-steward`) → persist durable memory to ICM → `continuity-steward` writes+commits
`.handoff/loop/HANDOFF.md` → weave `relay:handoff` heartbeat → best-effort cron successor → stop
(prefer `hf checkpoint`/`hf handoff` when the kernel is reachable). The committed HANDOFF.md is the
resume signal; a fresh session re-enters via `session-relay-resume`.

## Phase E: Evaluate & evolve (runs last — at DONE and at HAND OFF)

Invoke `evolution-steward` (`model: "opus"`, skill `harness-evolution`): evaluate the run (friction,
**gate quality** — did the parity gate miss a downgrade or false-block?, coverage, human walls),
mine generalizable lessons into the lessons ledger, and upgrade the harness — auto-applying only
low-risk in-scope edits via the standard PR flow (with a change-history row), proposing structural
changes in `.handoff/loop/proposed-upgrades.md`. It may only ever *strengthen* the parity/DONE gate,
never weaken it, and stewards only this harness (scope law). Lightweight at HAND OFF, full at DONE.

## DONE gate (no-downgrade, evidence-backed)

Write `.handoff/loop/DONE` only when ALL hold:
- **Left-behind sweep passes at BOTH grains** — `rust-port-cartographer` re-scans the source and
  finds zero *units* missing from the ledger and zero `- [ ]`/`- [~]` unit rows; **then re-harvests
  the full source *symbol* set (`git kb code symbols --json --limit -1`, same visibility filter) and
  finds zero symbols missing from `symbol-map.md`, zero `- [ ]`/`- [~]`/`- [!]` symbol rows, and zero
  rollup violations (every `- [x]` unit has 100% `- [x]`/`- [≠]` symbols).** A zero/empty symbol
  harvest of a non-empty source is fail-closed (`NEEDS-HUMAN`), never a vacuous `0/0` pass. (The
  completeness critic, unit + symbol.)
- Every unit is `- [x]` (parity-verified) or an explicit `- [≠]` intentional-divergence with owner
  approval — **and every symbol in `symbol-map.md` is `- [x]`/`- [≠]`** (symbols X/Y = Y/Y).
- `cargo build` + `cargo clippy -D warnings` + `cargo test` all green.
- The parity trail in `.handoff/loop/findings/parity.md` shows a passing differential test per unit.
- **When `dest_repo` Y is set (merge in scope):** the **merge ledger is 100%** — every ported unit is
  `- [x]` merged + **re-verified in Y** (or owner-approved `- [≠]`); a merge left-behind sweep finds no
  ported-but-unmerged unit; **Y's** `cargo build`/`clippy`/`test` are green; **Y is not regressed** (the
  `findings/y-regression.md` diff is clean — the dual no-downgrade gate); and no contract Y's consumers
  depend on was broken (or resolved per `rust-port-merge` §breaking-contract). Then **open the PR from
  `dest_branch` → `dest_base` with auto-merge armed**. See `references/merge-ledger.md`.
Record the evidence (unit counts, **symbol counts X/Y**, both sweep results, **and merge counts X/Y +
the Y-green + Y-not-regressed results + the Y PR link when merging**) inside `DONE`. After writing `DONE`, run **Phase E**
(full retro) so the completed port feeds the harness's evolution.

## Data transfer & error handling

- File bus: `.handoff/loop/{parity-ledger,symbol-map,merge-ledger,target-architecture,baseline,loop_state,HANDOFF}.md`,
  `findings/{parity,merge,y-regression}.md`, `reports/{inventory,research,cross-repo-refs}.md`.
- **Retry once; never fake completion.** Specialist errors → `- [!]` with reason, continue other
  units. Parity FAIL → unit stays open. Human wall (needs network creds to run source, etc.) →
  `.handoff/loop/NEEDS-HUMAN`, stop. Conflicting behavior readings → keep both, verifier adjudicates.
- **The cardinal rule:** never weaken the parity gate, stub a unit, or drop a branch to make a cycle
  pass. A red parity test is honest; a fake green defeats the harness's entire purpose.

## Team size

10 agents (Large): 7 specialists (`rust-port-cartographer`, `-architect`, `-porter`,
`-parity-verifier`, `-merge-integrator`, `-researcher`, `-cross-repo-referencer`) + 3 shared
(`build-health-auditor`, `continuity-steward`, `evolution-steward`). Still **one specialist per cycle**
(porter in a port cycle; merge-integrator in the appended merge step), so coordination stays bounded —
see the **Agent runtime** table above for the full per-agent execution contract + the 3-model tiering.

## Eject

`bash scripts/eject.sh <target-repo>` copies this harness (skills + the 10 agents) into the port
repo's `.claude/` and scaffolds `.handoff/loop/`. See `references/eject.md`. Invoke as `/rust-port`
once ejected.

## Test Scenarios

**Happy path:** Port `meta/Archon` (TS/Bun) to Rust. DISCOVER inventories 600+ units into the ledger;
architect maps packages→crates, express→axum, prisma→sqlx, sets tokio+thiserror. Cycle N: porter
ports `auth-service/token.ts` fully (all 4 error branches) → builds → verifier runs source & Rust
over happy + 4 error inputs, outputs match → `- [x]`, commit. … At 100%, cartographer's sweep finds
nothing left → tests green → write `DONE`.

**Error path (attempted downgrade):** Porter ports a streaming handler as a synchronous one "for
now." Build is green, but the parity-verifier feeds a streaming input and the Rust returns all-at-once
→ `FAIL` (expected: incremental chunks; actual: single buffer). Unit stays `- [~]` with the exact
diff; the cardinal rule blocks commit. Next cycle the porter implements the streaming version.

**Error path (runtime-construct downgrade):** Porter maps Archon's `dag-executor.ts` parallel layers
onto a sequential `for` loop, and a `loop`-until-signal node onto a fixed-count loop. Build is green,
but the parity-verifier feeds a parallel workload (Rust runs layers serially → ordering/timing diff)
and a cancellation input (Rust never aborts → stuck) → `FAIL`. The runtime contract (concurrency
degree, loop-until-signal, cancellation point) is part of the ledger row per
`references/runtime-constructs.md`, so it stays `- [~]` until the executor runs layers concurrently
and honors the stop signal. Mapping onto a substrate (e.g. run-state onto `hf`) is verified the same
way — a mapped unit is differentially tested, never trusted.

**Error path (intra-unit symbol drop):** Porter ports `Config` but omits one field and one enum
variant. `cargo build` is green and the happy-path differential PASSes, but the verifier exercises
every `symbol-map.md` row for the unit: the dropped field's row stays `- [ ]` and the missing variant
`FAIL`s → by the rollup rule the unit can't reach `- [x]`. The pre-DONE symbol sweep would also catch
it as an unmapped/unverified symbol. The dropped symbol cannot hide behind the module compiling.

**Port-and-merge happy path:** Port `meta/Archon` (TS/Bun) to Rust **and merge into `harness-agent-rs`**
(`dest_repo` Y). DISCOVER: researcher's reuse map finds Y already provides durable run-state via `hf`
and messaging via `weave`, so the architect marks those units MAP-ONTO (not reimplement);
cross-repo-referencer seeds the reference map + merge-ledger. Cycle N (haiku/sonnet/opus tiered): porter
(sonnet) ports `dag-executor` layer logic → verifier (opus) PASS standalone → merge-integrator (opus)
lands it as a new `harness-agent-rs` module, grit-locks the touched Y symbols, wires it → build-health
(haiku) Y green → verifier (opus) **re-verifies in Y** → `- [x]` merged, commit. … At 100% port + 100%
merge, both sweeps clean, Y green → `DONE`.

**Error path (merge downgrade):** merge-integrator maps a streaming run-event unit onto an existing Y
helper that buffers. Y compiles, but the **re-verification in Y** feeds a streaming input → Y emits one
buffer (source streamed) → `FAIL`. The unit stays `- [~]` in the merge ledger (a standalone PASS does
not close a merge row); reuse-by-narrowing is rejected — next cycle it extends the Y helper to stream.

## References
- `references/parity-ledger.md` — unit ledger schema + dependency ordering + the no-downgrade legend.
- `references/merge-ledger.md` — destination-repo merge ledger schema + landing decisions + merge DONE gate.
- `references/symbol-map.md` — per-symbol map schema + deterministic harvest + the unit-rollup rule.
- `references/runtime-constructs.md` — port-and-map decision table for agent-runtime / orchestration
  subsystems (reimplement vs map-onto `hf`/`weave`/`grit`/`icm`/provider-CLI; no behavior dropped).
- `references/eject.md` — install into the port repo.
- `scripts/loop_state.template.md` · `scripts/eject.sh` · `scripts/ralph-rust-port.sh` (SAFE runner).
