#!/usr/bin/env bash
# Stop / PreCompact hook (envctl) — auto-checkpoint this session into the FLEET ledger (ADR-0004 §3)
# AND mirror it one-way into GitKB (`hf sync`, ADR-0003 HFTASK-0011 — TASK-0024 GO-LIVE).
#
# LIVE: the kernel's `hf checkpoint --auto --quiet` + `hf sync` verbs landed (Epic A TASK-0001/0002,
# meta/handoff #17). Both calls are fail-soft — if `hf` is absent OR rejects a flag we swallow it and
# exit 0, so the session is NEVER blocked. `hf sync` is one-way (ledger truth → .kb context/overridable),
# so a checkpoint also lands in code intelligence; this makes "auto-sync to .handoff and .kb" TRUE.
#
# Ledger-residency (the kernel invariant): the shipped `hf` resolves a CWD-relative `.handoff/`
# (`const HF=".handoff"`, no --ledger flag), so we run it from $META_ROOT — the witnessed FLEET
# ledger is $META_ROOT/.handoff/ledger.db ONLY. NEVER a per-repo ledger (that would violate ADR-0004).
# $META_ROOT is resolved by walking up to the .meta.yaml marker, so this works from envctl or any
# of its worktrees (meta/.worktrees/<slug>/envctl) without a hardcoded path.
set -u

d="${CLAUDE_PROJECT_DIR:-$PWD}"
META_ROOT=""
while [ "$d" != "/" ] && [ -n "$d" ]; do
  [ -f "$d/.meta.yaml" ] && META_ROOT="$d" && break
  d="$(dirname "$d")"
done
[ -n "$META_ROOT" ] || exit 0

# find hf: prefer PATH (post-relocation), else the kernel build under meta/handoff.
HF="$(command -v hf 2>/dev/null || true)"
if [ -z "$HF" ]; then
  for c in "$META_ROOT/handoff/target/release/hf" "$META_ROOT/handoff/target/debug/hf"; do
    [ -x "$c" ] && HF="$c" && break
  done
fi
[ -n "$HF" ] || exit 0

# fail-closed residency: refuse to let a per-repo ledger be created. Only the fleet ledger at
# $META_ROOT/.handoff/ledger.db is permitted; run hf from there so its CWD-relative .handoff resolves to it.
cd "$META_ROOT" 2>/dev/null || exit 0
"$HF" checkpoint --auto --quiet >/dev/null 2>&1 || true
# TASK-0024 GO-LIVE: one-way mirror the witnessed FLEET ledger → GitKB (.kb context/overridable/
# {active,progress}). Same $META_ROOT residency as the checkpoint (never a per-repo ledger). Fail-soft.
"$HF" sync --auto >/dev/null 2>&1 || true
exit 0
