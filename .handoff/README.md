# .handoff — continuity layer (full)

This repo is a member of the FlexNetOS meta workspace. This directory is its continuity layer
(META-ORG-POLICY.md **P7**; design: handoff ADR-0003 + ADR-0004).

- `context/capsule.json` — who this repo is and what's next (census-derived; keep accurate).
- State precedence: **Git > FLEET ledger > task cards**. The FLEET ledger lives at
  **`meta/.handoff/ledger.db`** (the witnessed orchestration-home ledger envctl writes to via `$META_ROOT`;
  distinct from the kernel's own `meta/handoff/.handoff/ledger.db`). **No binary state in THIS directory** —
  git-committed text only (ADR-0004 §3 / P7; enforced by `.gitignore: .handoff/**/ledger.db` + `hf fleet status`).
- `tasks/` — execution cards minted from kb planning tasks (`hf task mint --from-kb`, ADR-0003). Empty until
  kb task docs exist for envctl; the packet degrades to "(no open cards)".
- `packets/latest.md` — resume packet **compiled** by `hf fleet render envctl` from the FLEET ledger + this
  repo's git-text capsule/cards (ADR-0004 §4). Rendered, never hand-written.
- `hooks/hooks.toml`, `policies/rules.toml`, `skills/` — OPTIONAL autonomous-loop descriptors (ADR-0004 §2);
  declarative text the kernel/harness reads. Ledger-mutating verbs they name run at `$META_ROOT`, never here.
- `loop/` — autonomous-loop state (the **active** agenticOS-consolidation forge-loop; Epics A–E). Migrated
  here from the deprecated `_workspace/` (HARNESS-UPGRADE-KIT v2 / ADR-0004); history preserved via `git mv`.
  Cold-start from `loop/HANDOFF.md` + `loop_state.md` + `backlog.md`.
- Planning lives on the kb board (`/kb-board`); cards here are derived views synced at checkpoint.
