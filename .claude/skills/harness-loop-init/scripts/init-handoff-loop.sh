#!/usr/bin/env bash
# init-handoff-loop.sh — idempotently create a harness loop's durable state dir (.handoff/loop/).
# The first action of a loop harness, before DISCOVER. Safe to re-run: creates what's missing, never
# clobbers an in-flight backlog/ledger/HANDOFF. SAFE/read-mostly (only writes scaffolding).
#
# Usage: bash init-handoff-loop.sh [TARGET_DIR] [LOOP_STATE_TEMPLATE]
set -euo pipefail
TARGET="${1:-$(pwd)}"
TEMPLATE="${2:-}"
[ -d "$TARGET" ] || { echo "error: target dir not found: $TARGET" >&2; exit 1; }
TARGET="$(cd "$TARGET" && pwd)"
WS="$TARGET/.handoff/loop"

mkdir -p "$WS/findings" "$WS/reports"
[ -f "$WS/findings/.gitkeep" ] || : > "$WS/findings/.gitkeep"
[ -f "$WS/reports/.gitkeep" ]  || : > "$WS/reports/.gitkeep"
echo "  dir    -> .handoff/loop/{findings,reports}"

# Seed loop_state.md only if absent (never clobber live state).
if [ -f "$WS/loop_state.md" ]; then
  echo "  state  -> loop_state.md exists (left as-is)"
elif [ -n "$TEMPLATE" ] && [ -f "$TEMPLATE" ]; then
  cp "$TEMPLATE" "$WS/loop_state.md"; echo "  state  -> loop_state.md seeded from template"
else
  cat > "$WS/loop_state.md" <<'STATE'
# Loop state
session_started: <UTC>     # you supply it; the runtime can't read the clock
loop: <loop-name>
branch: <branch>
worktree: <abs path>
cycle_budget: 3
cycles_this_session: 0     # reset to 0 on RESUME
cycles_total: 0
last_item: (none — init only)
status: INIT — durable state created; awaiting DISCOVER
last_update: <UTC>
STATE
  echo "  state  -> loop_state.md seeded (generic skeleton)"
fi

# Write the state-contract README only if absent.
if [ -f "$WS/README.md" ]; then
  echo "  doc    -> README.md exists (left as-is)"
else
  cat > "$WS/README.md" <<'DOC'
# `.handoff/loop/` — durable loop state (the source of truth)

This loop keeps ALL its state here, on disk, committed every cycle, so a fresh process resumes cold
with zero loss. Created by `harness-loop-init`.

| File | Role |
|------|------|
| `loop_state.md` | the ledger (counters, budget, current item) |
| `backlog.md` / `parity-ledger.md` / `research-ledger.md` | the work list (per harness) |
| `baseline.md` | verify-on-resume command block |
| `HANDOFF.md` | written by `session-relay-wrap-up`; the authoritative cold-resume signal |
| `findings/` · `reports/` | per-agent findings + synthesized outputs |
| `DONE` / `NEEDS-HUMAN` / `STOP` | terminal sentinels (read by the external runner) |

**Committed every cycle:** everything here **except** `*.log` / `ralph-run-*.log` (gitignored).
DOC
  echo "  doc    -> README.md (state contract) written"
fi
echo "  ✓ .handoff/loop/ initialized at $TARGET"
