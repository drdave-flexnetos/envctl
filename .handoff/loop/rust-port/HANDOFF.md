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

## Owner follow-up — DONE 2026-06-13
**kasetto fork reconciliation COMPLETE.** Owner: "run the force push; all original code used for the
port, only our change was the agent builder." Executed: fork RENAMED `env_manager_agent` →
**`FlexNetOS/kasetto`**; full-repo git bundle archived
(`meta/.archives/kasetto-full-pre-v320-sync-2026-06-13.bundle`, verified complete); divergence-backup
branch pushed to the remote; `origin/main` force-pushed (`--force-with-lease`) `f2a50b7...ec01cca` =
upstream **v3.2.0** (now 0/0 in sync); v3.2.0 tag pushed; remote retargeted to the canonical URL;
`.meta.yaml` updated via meta PR #31. Fork == upstream == local. Nothing pending here.
# HANDOFF — rust-port-MERGE (kasetto → envctl) · VERIFY/MERGE mode · 2026-06-13

**Resume with:** `/harness:rust-port-merge` (or `/rust-port` — the harness is now ejected into
`envctl/.claude`). Reads state from **`.handoff/loop/rust-port/`** (namespaced — NOT the flat
`.handoff/loop/`, which is the forge-loop's). This completes **forge-loop TASK-0012** (Epic C:
kasetto absorption, no downgrade).

## State precedence
Git (committed) > this HANDOFF > `.handoff/loop/rust-port/{merge-ledger,parity-ledger,loop_state}.md`.
In verify-merge mode the **merge-ledger.md is authoritative**.

## Where things are
- **Merged to develop (PR #71):** `crates/agent-env` (seed + model/* port) + the rust-port DISCOVER state.
- **PR #72 (branch `task-0012-verify-merge`, off develop):** the rust-port-merge harness eject + the
  verify-merge classification. Auto-merge armed. If merged before you resume → fresh worktree off the
  new develop (has everything); else continue on `task-0012-verify-merge`.
- **X (source):** `meta/kasetto` = pivoshenko **v3.2.0** (`ec01cca`); fork synced (origin/main = v3.2.0,
  0/0; divergence on `flexnetos-divergence-backup-2026-06-13` + bundle `.archives/`).
- **Y (dest):** envctl. The harness lives in `.claude/` (12 skills + 10 agents).
- ⚠️ Stray remote branch `task-0012-agent-env` was recreated by a post-merge push — **harmless, delete it**
  (its content is in develop via #71's squash + #72). Auto-mode blocked the deletion; do it manually.

## Merge ledger (authoritative): 115 rows
- **55 `[~]`** — agent-env's already-merged foundational+model surface (config/extend/source/hash/lock/
  agent/report). MERGED into Y, **re-verification vs kasetto X pending**.
- **47 `[ ]`** — to merge: fsops (copy/dirs/settings/select_targets/resolve_*), `config_edit.rs` mutation
  engine, command business logic, AND the **3 left-behind engines** the verify sweep caught:
  - **MC-01/MC-02** — `src/mcps/*` additive never-clobber MCP merge (4 formats; **#1 no-downgrade risk:
    preserve global broker/repowire/weave**).
  - **PR-01** — `src/prompts/*` 5 command-format transforms.
- **13 `[≠]`** — front-end (ui/banner/colors + clap wiring); envctl owns rendering (TASK-0014).
- Researcher confirmed **0 duplications**, **reuse-Y=0** (engine's lock/runtime/doctor ≠ agent-env surface).

## NEXT (verify-merge ITERATE — the dual gate, per unit)
1. **Parity-verifier pass on the 55 `[~]`** — differentially verify each agent-env symbol vs kasetto v3.2.0
   (the source is Rust → run/port kasetto's own tests + fixtures). PASS → `- [x]`. (A representative sample
   already matched in `/verify`: SHA-256 vs `sha256sum`, the 4-host resolver, config parse, 21-agent table.)
2. **Port the 47 `[ ]`** dep-ready first; the MCP additive merge (MC-01) is the priority no-downgrade unit.
3. **Dual gate every cycle:** verify vs kasetto X **AND** assert envctl Y not regressed
   (`findings/y-regression.md`); Y stays green (build/clippy/test/no-c).
4. **DONE** only at 100% merged+verified `[x]`/`[≠]` + left-behind sweep clean + Y green.

## Gates (non-negotiable)
no C / no mimalloc / one rustls ring-only / no-c.sh green · non-printing library · forbid(unsafe_code) ·
**never weaken the parity gate, never stub to pass, never narrow-to-fit Y** (the cardinal rule).

## Verify-on-resume baseline
`rtk proxy cargo test -p envctl-agent-env` (78+) · `bash ci/gates/no-c.sh` (PASS). Red → NEEDS-HUMAN.
