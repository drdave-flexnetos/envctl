#!/usr/bin/env bash
# ralph-provision.sh — the external "Ralph" loop that self-restarts the env-install-loop with a
# FRESH context every iteration (each `claude -p` process is a clean session = the `/new` effect),
# until the box is fully provisioned. This is the real, executable form of "run /new then
# /env-install-loop and hand off to yourself": the agent cannot type /new, but a new process can.
#
# Each iteration spawns one fresh headless agent that runs one cycle-budget of install/repair
# (install <-> reset remediation), commits per cycle, then writes EXACTLY ONE sentinel + exits:
#   .handoff/loop/HANDOFF.md  -> more work remains; this script spawns the next fresh process
#   .handoff/loop/DONE        -> doctor green + lock/kasetto clean + gates pass; this script exits 0
#   .handoff/loop/NEEDS-HUMAN -> hit a sudo/reboot/hardware wall; this script halts for you
#   .handoff/loop/STOP        -> kill switch (you `touch` it); this script halts
#
# LEGACY FALLBACK (read-only): a pre-migration successor may still target the old _workspace/ path.
# If a .handoff/loop/ sentinel is absent, the spawned agent may READ legacy _workspace/{HANDOFF.md,
# DONE,NEEDS-HUMAN,STOP} only to recover an in-flight mission — it must NEVER write the legacy path.
#
# SAFETY: default is ATTENDED/SAFE — headless agents cannot answer permission prompts, so without
# an explicit opt-in they will NOT perform destructive `--apply`/`--build`/`reset` actions. To run
# truly unattended (apply changes to THIS workstation without prompts) you must opt in with
# RALPH_APPLY=1, which adds --dangerously-skip-permissions. Do that only when you accept that this
# loop will modify the live system on its own. There is always a bounded max-iterations backstop.
set -euo pipefail

# --- config (env-overridable) ---
WORKTREE="${RALPH_WORKTREE:-$(pwd)}"            # repo/worktree to run in
BUDGET="${RALPH_BUDGET:-3}"                      # install cycles per fresh process
MAX_ITERS="${RALPH_MAX_ITERS:-50}"              # hard backstop on process restarts
SLEEP_BETWEEN="${RALPH_SLEEP:-5}"               # seconds between iterations
MODEL="${RALPH_MODEL:-opus}"
RESEARCH="${RALPH_RESEARCH:-1}"                 # run the component-research/audit pass (1=on)
WS="$WORKTREE/.handoff/loop"

log() { printf '[ralph %s] %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }
die() { log "FATAL: $*"; exit 1; }

command -v claude >/dev/null || die "claude CLI not on PATH"
[ -d "$WORKTREE/.git" ] || [ -f "$WORKTREE/.git" ] || die "WORKTREE is not a git repo: $WORKTREE"
mkdir -p "$WS"
mkdir -p "$WS/cycle"

# Apply-mode opt-in. SAFE by default: no permission bypass, so destructive steps are refused.
APPLY_ARGS=()
if [ "${RALPH_APPLY:-0}" = "1" ]; then
  APPLY_ARGS=(--dangerously-skip-permissions)
  log "RALPH_APPLY=1 — UNATTENDED APPLY MODE: this loop WILL modify the live workstation."
else
  log "SAFE mode (default): destructive --apply/reset will be refused. Set RALPH_APPLY=1 to act."
fi
[ "$RESEARCH" = "1" ] && log "RESEARCH on: each agent audits components + appends upgrades to backlog (RALPH_RESEARCH=0 to skip)." \
                      || log "RESEARCH off: install/repair only."

# The per-iteration prompt. Resume from the committed checkpoint; do one budget of work; then write
# exactly one sentinel and exit so this script decides whether to respawn.
if [ "$RESEARCH" = "1" ]; then
  RESEARCH_LINE="1b. Run the env-install-loop RESEARCH pass: deep-probe each declared component past
   detect/verify (real end-to-end exercise, gate quality, version currency + advisories, cross-
   component skew, hook hygiene, wiring reach) and auto-append classified items to backlog.md
   (harden:/fix:/upgrade: = loop-fixable; feature: = route to feature-forge, surface only). Findings
   must be evidence-based/sourced; offload probing to subagents. Read-only."
else
  RESEARCH_LINE="1b. (research pass disabled: RALPH_RESEARCH=0 — install/repair only.)"
fi
read -r -d '' PROMPT <<EOF || true
/env-install-loop resume (external Ralph runner, fresh context).
Branch/worktree: $WORKTREE.
1. If .handoff/loop/HANDOFF.md exists, follow session-relay RESUME from it (it is the authoritative
   signal); else DISCOVER gaps via envctl doctor + auto-detect and build .handoff/loop/backlog.md.
   (Legacy fallback, READ-ONLY: if .handoff/loop/HANDOFF.md is absent, you MAY read a legacy
   _workspace/HANDOFF.md to recover an in-flight pre-migration mission, but NEVER write _workspace/ —
   re-emit all new state under .handoff/loop/.)
$RESEARCH_LINE
2. Run up to $BUDGET install/repair cycles. Remediation ladder per item: install (dry-run -> apply)
   -> if detected-but-unhealthy and auto-fix won't resolve, reset --apply then install --apply ->
   verify on PATH + env vars in a FRESH shell -> commit per cycle. Destructive ops are fail-closed.
3. Then write EXACTLY ONE sentinel under .handoff/loop/ and stop (do not ScheduleWakeup):
   - DONE (with evidence: doctor green, auto-detect all healthy/zero drift, lock --check + kasetto
     sync --locked clean, build + no-c/shape/enable gates pass) -> create .handoff/loop/DONE
   - hit a sudo / interactive-auth / reboot / hardware wall you cannot clear -> create
     .handoff/loop/NEEDS-HUMAN with the reason
   - otherwise write .handoff/loop/HANDOFF.md (spawn continuity-steward) and exit.
Commit every cycle so the next fresh process resumes cold. Never weaken a fail-closed guard.
EOF

cd "$WORKTREE"
i=0
while :; do
  i=$((i + 1))
  if [ "$i" -gt "$MAX_ITERS" ]; then
    log "reached MAX_ITERS=$MAX_ITERS without DONE — halting (backstop). Inspect .handoff/loop/."
    exit 3
  fi
  # Pre-flight sentinel checks (kill switch + terminal states win before spawning).
  [ -f "$WS/STOP" ]        && { log "STOP sentinel present — halting for human."; exit 2; }
  [ -f "$WS/DONE" ]        && { log "DONE — provisioning complete."; exit 0; }
  [ -f "$WS/NEEDS-HUMAN" ] && { log "NEEDS-HUMAN: $(cat "$WS/NEEDS-HUMAN" 2>/dev/null) — halting."; exit 2; }

  log "iteration $i/$MAX_ITERS — spawning fresh agent (budget=$BUDGET, model=$MODEL)"
  # Fresh process => clean context (the /new effect). --print = headless. Don't abort the loop on a
  # single failed run; let the next iteration re-read durable state and continue.
  claude -p "$PROMPT" --model "$MODEL" --add-dir "$WORKTREE" "${APPLY_ARGS[@]}" \
    >>"$WS/ralph-run-$i.log" 2>&1 || log "iteration $i exited nonzero (continuing from durable state)"

  # Post-run terminal sentinels.
  [ -f "$WS/DONE" ]        && { log "DONE — provisioning complete."; exit 0; }
  [ -f "$WS/NEEDS-HUMAN" ] && { log "NEEDS-HUMAN: $(cat "$WS/NEEDS-HUMAN" 2>/dev/null) — halting."; exit 2; }
  [ -f "$WS/STOP" ]        && { log "STOP sentinel present — halting for human."; exit 2; }

  sleep "$SLEEP_BETWEEN"
done
