# forge-loop cycle 2 — TASK-0002 (seed Tier-A) → BLOCKED / NEEDS-DECISION

- **Loop:** agenticOS-consolidation · **Worktree:** `task-0002-seed-tier-a` off develop (7dd2443)
- **Date:** 2026-06-13 · **Verdict:** BLOCKED (NEEDS-DECISION) — also blocks TASK-0003

## Investigation (architect)
Goal: seed envctl's Tier-A `.handoff` (render `active.md`/`packets/latest.md`, mint
`handoff.task.v1` cards) with events in the **shared** `$META_ROOT/.handoff/ledger.db` only, no
per-repo ledger. Read the shipped kernel source (`meta/handoff/hf/src/{main.rs,kb.rs}`).

## Finding (source-proven)
The shipped `hf` is **strictly CWD-relative** — `const HF=".handoff"`, no `--ledger` flag, no
`HANDOFF_DIR`/`HANDOFF_LEDGER` env. `ledger_path()`/`tasks_dir()`/`packet_path()` all resolve under
`<CWD>/.handoff`. Three constraints are mutually exclusive:
1. Residency (ADR-0004) → run from `$META_ROOT` (ledger = `meta/.handoff/ledger.db`).
2. `hf task mint --from-kb` resolves `.kb` as `current_dir().parent()/.kb` → needs CWD=child-repo
   (envctl) → that run creates the FORBIDDEN `envctl/.handoff/ledger.db`.
3. `hf seed` writes the kernel's own 22 `HFTASK-####` cards, not the envctl backlog; `hf handoff`
   renders packets into `<CWD>/.handoff`.

→ No shipped path renders envctl's Tier-A layer against the shared ledger without violating the #1
non-negotiable invariant. The capability gap is in the **kernel** (`meta/handoff`), out of envctl
scope. TASK-0003 (p7 gate) depends on a seeded layer → blocked with it.

## Disposition
- TASK-0002 `[!]` blocked; TASK-0003 `[!]` blocked (depends on 0002).
- Decision record: `.handoff/decisions/FINDING-0002-hf-ledger-residency-vs-repo-tier-a.md` —
  3 options (A: add `--ledger`/`HANDOFF_DIR` + meta-root `.kb` resolution [recommended];
  B: rescope Tier-A to shared-ledger-only; C: seed kernel ledger at meta root). Owner/kernel call.
- Loop proceeds (no thrash): Epic A stalled → next unblocked pick = Epic C TASK-0012.

## State written back
- backlog: TASK-0002 `[ ]`→`[!]`, TASK-0003 `[ ]`→`[!]`, each with reason + FINDING-0002 ref.
- loop_state: cycles_this_session 1→2, cycles_total 1→2, last_item=TASK-0002 (blocked), next
  pick TASK-0012; progress log + Next-safe-step updated.
