# Skill: session-resume

## Purpose

Rehydrate current project state for a new agent session with minimal context load.

## Trigger phrases

- resume
- continue
- pick up
- what next
- recover session

## Steps

1. Run `hf resume --json` (from $META_ROOT — the orchestration home; never inside this member repo).
2. Read `.handoff/context/capsule.json` (who this repo is + next_command).
3. Read `.handoff/packets/latest.md` (compiled by `hf fleet render envctl` from the FLEET ledger).
4. Read `.handoff/loop/HANDOFF.md` + `loop_state.md` + `backlog.md` (the forge-loop cold-start package).
5. Check the latest drift report.
6. Print the exact next command.

## Hard rule

Do not edit files during this skill. Ledger-mutating verbs run at $META_ROOT only (P7 / ADR-0004 §3).
