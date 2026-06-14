#!/usr/bin/env bash
# eject.sh — install the packaged `rust-port` harness into a target (port) repo's .claude/.
# SAFE: copy + scaffold only. Never edits the target's tracked files; prints the .gitignore /
# CLAUDE.md snippets for you to apply.
#
# Usage: bash eject.sh <target-repo-dir>
set -euo pipefail
TARGET="${1:-}"
[ -n "$TARGET" ] || { echo "usage: bash eject.sh <target-repo-dir>" >&2; exit 1; }
[ -d "$TARGET" ] || { echo "error: target dir not found: $TARGET" >&2; exit 1; }
TARGET="$(cd "$TARGET" && pwd)"
PLUGIN="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"  # harness/

SKILLS=(rust-port rust-port-inventory rust-port-translate rust-port-parity rust-port-merge cross-repo-reference icm-memory session-relay-wrap-up session-relay-resume cross-repo-health harness-loop-init harness-evolution)
AGENTS=(rust-port-cartographer rust-port-architect rust-port-porter rust-port-parity-verifier \
        rust-port-merge-integrator rust-port-researcher rust-port-cross-repo-referencer \
        build-health-auditor continuity-steward evolution-steward)

mkdir -p "$TARGET/.claude/skills" "$TARGET/.claude/agents" \
         "$TARGET/.handoff/loop/findings" "$TARGET/.handoff/loop/reports"
for s in "${SKILLS[@]}"; do cp -r "$PLUGIN/skills/$s" "$TARGET/.claude/skills/$s"; echo "  skill  -> .claude/skills/$s"; done
for a in "${AGENTS[@]}"; do cp "$PLUGIN/agents/$a.md" "$TARGET/.claude/agents/$a.md"; echo "  agent  -> .claude/agents/$a.md"; done
echo "  state  -> .handoff/loop/ scaffolded (seed loop_state.md: source_root + rust_target; cartographer seeds parity-ledger.md + symbol-map.md)"

cat <<'SNIP'

── Apply these to the target repo yourself (repo-specific; not edited for you) ──

# .gitignore:
.claude/*
!.claude/agents/
!.claude/skills/
.handoff/loop/*.log
.handoff/loop/ralph-run-*.log

# CLAUDE.md pointer:
## Harness: rust-port (full-parity Rust port loop)
**Trigger:** for porting this project to Rust / "resume the port", use the `/rust-port` skill.
Parity ledger (.handoff/loop/parity-ledger.md) must hit 100% — no feature left behind. Runner:
.claude/skills/rust-port/scripts/ralph-rust-port.sh (SAFE by default).

# .claude/settings.json — DETERMINISTIC pre-session memory priming (recommended).
# This is the MOST IMPORTANT memory layer: it fires at every session start with NO model decision,
# so the agent is primed with prior context (decisions, resolved errors, gotchas) before its first
# token — a missed recall makes the whole session run blind. The `icm-memory` skill is the as-needed
# complement (the model recalls/stores mid-task). Within the meta workspace this is inherited from the
# user-global settings; OUTSIDE it, add this so the priming travels with the harness. Graceful no-op
# when ICM is absent (`command -v icm` guard + `|| true`), so it never blocks session start.
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume",
        "hooks": [
          { "type": "command",
            "command": "command -v icm >/dev/null && icm recall-context \"rust-port resume: prior decisions, resolved errors, parity/merge gotchas for this repo\" --limit 8 2>/dev/null || true" }
        ]
      }
    ]
  }
}

Done. Invoke the ejected harness as: /rust-port
SNIP
