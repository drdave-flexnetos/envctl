# Feature-Forge brief — grit-harness-parallel  (PRIORITY: CRITICAL must-have)

**Spawned by** the dashboard forge-loop final session (2026-06-05) at the user's request:
> "grit is a critical must-have."

**Run this as:** a `feature-forge` cycle (architect → rust-implementer → invariant-guardian).
Work in a fresh worktree (`meta git worktree create grit-harness-parallel`).

## Goal
Adopt grit's `claim → work → done` AST git-lock coordination **in the Feature Forge harness**
so multiple `rust-implementer` agents can run **in parallel across meta member repos with zero
merge conflicts**.

## What already exists (do NOT redo)
- `grit-component` is DONE: `grit` (FlexNetOS/grit, `~/Desktop/meta/grit`) is envctl-managed as a
  declarative manifest component (`grit.toml`), installed box-wide to `~/.cargo/bin` via
  `cargo install --path`. detect/install/verify/fix/remove all wired; `envctl.lock` synced.
- **grit is an external TOOL binary, NOT a crate dep.** It links C (rusqlite bundled) + aws/azure
  SDKs, so it stays outside envctl's no-C trust boundary. The harness must use it via **CLI/bash**,
  never as a Rust dependency. Keep it that way.

## Scope of this feature (the unchecked backlog item)
Adopt grit in the harness skills:
- `grit init` per repo (idempotent).
- **Opt-in parallel mode** in `.claude/skills/{feature-forge,forge-loop}` (the hand-authored,
  git-tracked harness skills — editing these is the sanctioned exception to "skills are
  kasetto-generated"; see envctl/CLAUDE.md "Harness: Feature Forge").
- Function-level claims via `file::symbol`.
- `--queue` for contested symbols.
- `--with-deps` for dependency-aware locks.
- Meta-wide seeding via `meta exec -- grit init`.
- Local SQLite-WAL backend default; Azure/S3 deferred (later).

## Constraints / invariants
- Harness skills are **hand-authored & git-tracked, OUTSIDE kasetto** — edit `.claude/skills/*`
  and `.claude/agents/*` in place and commit. Do NOT route through `kasetto sync`.
- Do NOT make grit a crate/workspace dependency (no-C trust boundary). CLI-only.
- Parallel mode is **opt-in** — the default single-implementer path must keep working unchanged.
- Update the harness "Change history" table in envctl/CLAUDE.md when you change the skills.

## Acceptance criteria
- [ ] `feature-forge`/`forge-loop` skills document + support an opt-in grit parallel mode
      (claim→work→done, `file::symbol`, `--queue`, `--with-deps`).
- [ ] Idempotent `grit init` (per-repo + `meta exec -- grit init`) wired into the flow.
- [ ] Default (non-parallel) path unchanged and still passes a smoke run.
- [ ] No new Rust dep on grit; no-C gate still green.
- [ ] CLAUDE.md harness change-history row added.

## Pointers
- envctl/CLAUDE.md → "Harness: Feature Forge (the construction crew)".
- `.claude/skills/{feature-forge,forge-loop,session-relay}/` and `.claude/agents/*`.
- Backlog origin: `_workspace/backlog.md` line ~156 (grit-harness-parallel).
