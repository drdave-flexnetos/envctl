# .handoff — continuity layer (full)

This repo is a member of the FlexNetOS meta workspace. This directory is its continuity layer
(META-ORG-POLICY.md **P7**; design: handoff ADR-0003 + ADR-0004).

- `context/capsule.json` — who this repo is and what's next (census-derived; keep accurate).
- State precedence: **Git > witnessed ledger > task cards**. The fleet ledger lives at
  `meta/handoff/.handoff/ledger.db` — no binary state in this directory, git-committed text only.
- `tasks/` — execution cards minted from kb planning tasks (`hf task mint --from-kb`, ADR-0003).
- `packets/` — resume packets (`hf handoff`).
- `loop/` — autonomous-loop state (forge-loop / env-install-loop). Migrated here from the
  deprecated `_workspace/` (HARNESS-UPGRADE-KIT v2 / ADR-0004): history preserved via `git mv`,
  new loop state lands here. The current loop is TERMINAL-DONE; see `loop/loop_state.md`.
- Planning lives on the kb board (`/kb-board`); cards here are derived views synced at checkpoint.
