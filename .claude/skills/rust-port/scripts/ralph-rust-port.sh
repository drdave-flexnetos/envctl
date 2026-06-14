#!/usr/bin/env bash
# ralph-rust-port.sh — external Ralph loop: self-restarts the rust-port harness with a FRESH context
# each iteration (each `claude -p` = a clean session) until a terminal sentinel. Truth lives on disk
# (.handoff/loop/ parity-ledger + commits), so every restart resumes cold with zero loss.
#
# SAFE BY DEFAULT and SAFE-ONLY as shipped: each spawned session prompts for permission as usual;
# this runner contains no permission-system bypass. Unattended apply is a deliberate operator change
# authorized at the settings layer (see the meta-plugin runner's note for the pattern).
set -euo pipefail
WORKTREE="${RALPH_WORKTREE:-$(pwd)}"; BUDGET="${RALPH_BUDGET:-3}"
MAX_ITERS="${RALPH_MAX_ITERS:-100}"; SLEEP_BETWEEN="${RALPH_SLEEP:-5}"; MODEL="${RALPH_MODEL:-opus}"
WS="$WORKTREE/.handoff/loop"; mkdir -p "$WS"
log(){ printf '[ralph rust-port %s] %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }
command -v claude >/dev/null || { log "FATAL: claude not on PATH"; exit 1; }
log "SAFE mode: permission prompts active. This runner never bypasses the permission system."

read -r -d '' PROMPT <<EOP || true
Resume the rust-port harness (external Ralph runner, fresh context): run /rust-port resume
(if ejected here) or /harness:rust-port resume. Worktree: $WORKTREE.
1. If .handoff/loop/HANDOFF.md exists, follow session-relay RESUME (authoritative): read it, run
   verify-on-resume baseline, continue at the parity ledger's next unported unit. Else DISCOVER:
   inventory the source into .handoff/loop/parity-ledger.md.
2. Run up to $BUDGET cycles: one unit each — port FULLY (no stubs, every branch), build+clippy,
   then differential parity-verify source-vs-Rust. Commit per cycle. NEVER fake a -[x]; a downgrade
   never passes the gate.
3. Then write EXACTLY ONE sentinel under .handoff/loop/ and stop (no ScheduleWakeup):
   DONE (left-behind sweep clean + tests green) | NEEDS-HUMAN (reason) | else HANDOFF.md.
EOP

cd "$WORKTREE"; i=0
while :; do
  i=$((i+1)); [ "$i" -gt "$MAX_ITERS" ] && { log "MAX_ITERS hit — halting."; exit 3; }
  for s in STOP DONE NEEDS-HUMAN; do [ -f "$WS/$s" ] && { log "$s — halting."; [ "$s" = DONE ] && exit 0 || exit 2; }; done
  log "iter $i/$MAX_ITERS — spawning fresh agent (budget=$BUDGET, model=$MODEL)"
  claude -p "$PROMPT" --model "$MODEL" --add-dir "$WORKTREE" \
    >>"$WS/ralph-run-$i.log" 2>&1 || log "iter $i nonzero (continuing from durable state)"
  for s in DONE NEEDS-HUMAN STOP; do [ -f "$WS/$s" ] && { log "$s."; [ "$s" = DONE ] && exit 0 || exit 2; }; done
  sleep "$SLEEP_BETWEEN"
done
