# HANDOFF — rust-port (kasetto → envctl-agent-env) · 2026-06-13

**Resume with:** `/harness:rust-port` (it resumes via `session-relay-resume`: read this file, verify
baseline, continue Phase-2 ITERATE at the ledger's next dep-ready unit). This port is the
implementation engine for **forge-loop TASK-0012** (Epic C: kasetto absorption, no downgrade).

## State precedence
Git (committed) > this HANDOFF > `.handoff/loop/rust-port/{parity-ledger,loop_state}.md`. The
**parity-ledger.md is the source of truth** for what's ported/verified.

## Where things are
- **Branch:** `task-0012-agent-env` (off `origin/develop`). **PR #71** open → develop, **auto-merge
  armed (squash)**, BLOCKED until CI green. If #71 merges before you resume: create a FRESH worktree
  off the new `develop` (it will contain the crate + ledger) and continue there. If not merged: keep
  working on `task-0012-agent-env`.
- **Worktree:** `/home/drdave/Desktop/meta/.worktrees/task-0012-agent-env/envctl`.
- **Source of truth (port FROM):** `/home/drdave/Desktop/meta/kasetto` — LOCAL `main` now =
  **pivoshenko/kasetto v3.2.0** (`ec01cca`, Cargo 3.2.0). Old FlexNetOS divergence preserved on local
  branch `flexnetos-divergence-backup-2026-06-13`. ⚠️ **`origin/main` (FlexNetOS/env_manager_agent)
  is STILL v3.0.0+divergent — see "Owner follow-up" below; do not assume the fork is synced.**

## Progress (commits 6ecb270, 8780c85)
- Crate `crates/agent-env` (`envctl-agent-env`) builds GREEN: **78 unit + 1 integration test**,
  clippy `-D warnings`, fmt, **no-c PASS** (mimalloc dropped, flate2 rust_backend, one rustls ring-only).
- **Ledger: 0/112 parity-verified `[x]` · 55 ported `[~]` · 44 todo `[ ]` · 13 front-end `[≠]`.**
  Ported `[~]`: foundational (config/extend/source/hash/lock) + ALL of `model/*` (21-preset
  path/target table, 4 MCP + 5 command formats, sync-result types).

## NEXT (in order)
1. **PARITY-VERIFIER pass (do FIRST):** upgrade the 55 `[~]` → `[x]` by differential-testing each
   against kasetto v3.2.0 (source is Rust → port kasetto's own unit tests + add fixtures; PASS only
   on byte-identical behavior). Cardinal rule: never fake `[x]`. Any mismatch → leave `[~]`/`[!]` with
   the exact diff. This is the biggest unproven block.
2. **Continue Phase-2 ITERATE** on the 44 `[ ]`, dep-ready first: XC-03 (dirs/XDG), XC-04 (now_unix),
   then fsops (F-03 copy_dir, F-05 resolve_path, F-06 select_targets, F-07..F-10 destination/target
   resolution + scope_root/relativize), then `config_edit.rs` (FE-* — the 811-line comment-preserving
   mutation engine for add/remove), the **MCP additive/never-clobber merge** (#1 no-downgrade risk —
   MUST preserve global broker/repowire/weave servers), the 5 command-format transforms, and the
   command business logic (sync/add/remove/lock/list/clean — logic only; CLI verbs = TASK-0014).
3. **DONE gate:** cartographer left-behind sweep (re-scan kasetto, zero unrepresented units) + all
   rows `[x]`/`[≠]` + workspace green. Then Engine wiring (TASK-0013) and CLI verbs (TASK-0014) follow
   as separate forge-loop items.

## Invariants (every cycle)
no C / no mimalloc / one rustls ring-only / no-c.sh green · non-printing library · forbid(unsafe_code)
· build/test/clippy/fmt green · **never weaken the parity gate, never stub to pass.**

## Verify-on-resume baseline
`rtk proxy cargo test -p envctl-agent-env` (expect 78+) · `bash ci/gates/no-c.sh` (PASS). If red → NEEDS-HUMAN.

## Owner follow-up (NOT loop-auto; needs explicit go-ahead)
**kasetto fork (FlexNetOS/env_manager_agent) `origin/main` reconciliation.** Owner authorized "sync
with pivoshenko, accept all kasetto changes." The LOCAL source-up (the part the port needs) is DONE.
The remaining piece is rewriting the FORK's `main` to match upstream v3.2.0 — a **force-push that
drops 305 FlexNetOS-only commits** (history rewrite, GitHub blast radius). Deferred for an explicit
owner OK on the destructive push (backup branch `flexnetos-divergence-backup-2026-06-13` exists; push
it to origin first, then `git push --force-with-lease origin main`). Until done, the fork remote is
stale vs the local checkout.
