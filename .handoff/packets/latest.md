# Handoff Packet (latest) — handoff.packet.v2

> Compiled by `hf fleet render envctl` from the FLEET ledger (meta/.handoff) + this repo's git-text capsule/cards. Not rendered from a per-repo ledger (ADR-0004 §3).

## 1. North Star (envctl)
envctl owns and contains the meta environment: every FlexNetOS tool/dotfile/.local/bin resolves inside meta; user-global ($HOME/.local, ~/.claude) holds ONLY symlinks into meta; envctl exports META_ROOT (resolved from the .meta.yaml marker, like meta_core's META_DATA_DIR) so no config hardcodes paths; secrets are held and auto-injected. Heal not harm; never downgrade; never delete (archive).

## 2. State Precedence
Git > FLEET ledger (meta/.handoff/ledger.db) > tasks/*.task.json > this packet.

## 3. Progress
Done: 0/0.  FLEET tamper-evident events verified: 0.

## 4. Remaining
- (no open cards)

