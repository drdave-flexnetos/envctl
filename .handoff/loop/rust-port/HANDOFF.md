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
