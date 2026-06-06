# Feature-Forge brief — node-via-bun manifest fix  (bun-first, CONSERVATIVE)

**Spawned by** the dashboard forge-loop final session (2026-06-05) at the user's request:
> "bun is always the go-to | but bun must not break something that we have to spend a lot of
>  time refactoring right now."

**Run this as:** a `feature-forge` cycle (architect → rust-implementer → invariant-guardian).
Work in a fresh worktree (`meta git worktree create node-via-bun-fix`).

## Goal
Make `envctl doctor` / `auto-detect` go **truthfully green** for the JS-runtime story —
**without breaking the healthy `group-ai-clis` stack and without a heavy refactor.**
This is a **manifest-level truth-telling fix**, not a runtime migration.

## Guiding principle (user)
- **Bun is the default / go-to JS runtime.** Keep it that way; do NOT rip bun out.
- **But do not cascade.** Any change that would force a reset of the healthy ai-clis stack, or
  trigger a large refactor right now, is OUT OF BOUNDS. Prefer the smallest manifest change that
  tells the truth.

## Facts (research-resolved 2026-06-05 — do NOT re-derive, build on these)
- Bun's `node` shim **cannot** do `node --version` BY DESIGN (it only runs `node <script>`), so the
  `node-via-bun` component's verify hook can **never** pass on bun 1.3.x/1.4. Not a regression.
- **n8n cannot run on Bun at all** (isolated-vm needs V8; Bun uses JSC). n8n requires REAL Node
  20–24. Real node **v22.22.3** is installed at `~/.local/bin/node` — correct & in-range. Keep it.
- The symlink `~/.bun/bin/node -> bun` is **inert** (real node precedes it on PATH).
- `envctl reset node-via-bun` is **REFUSED** by the fail-closed guard — `group-ai-clis` declares a
  live reverse-dep on it; removing would cascade the healthy ai-clis stack. Do NOT force it.

## Solution direction (pick the least-invasive; architect decides)
Make the manifest reflect reality so doctor is green, with no cascade:
- **(a)** Mark `node-via-bun` *not-applicable* when a real Node in n8n's range is present
  (component reports detected/healthy-by-policy instead of a failing version check), **or**
- **(b)** Add a `node-real` component (owns the real-node-for-n8n requirement) and **drop/redirect**
  the `group-ai-clis → node-via-bun` edge so the guard no longer blocks and nothing healthy
  cascades — bun stays primary, real node coexists only for n8n's V8 need.

Bun remains the default runtime in either case; real node is the narrow n8n carve-out.

## Constraints / invariants
- Manifest component change → **re-lock `envctl.lock`** (currently 49 comps) and keep it clean.
- no-C trust boundary unchanged; `ci/gates/{no-c,shape,enable}.sh` must stay green.
- Destructive/guard semantics preserved (fail-closed). Don't weaken a guard to "fix" doctor.
- Pure-Rust / TOML-component idiom; no language drift.

## Acceptance criteria
- [ ] `envctl doctor` + `auto-detect --json` report the JS-runtime story truthfully green
      (no spurious node-via-bun drift) with bun as the default runtime.
- [ ] The healthy `group-ai-clis` stack is untouched — no reset, no cascade.
- [ ] Real node v22 retained for n8n; bun retained as default.
- [ ] `lock --check` clean; no-c/shape/enable gates green; build clean.
- [ ] Decision (a vs b) recorded with rationale in the plan + a CLAUDE.md/docs note.

## Pointers
- Manifest: `manifest/*.toml` (node-via-bun lives in the ai-clis group; `group-ai-clis` aggregator).
- Backlog origin: `_workspace/backlog.md` lines ~51–64 (node-via-bun TABLED, with the research).
