#!/usr/bin/env bash
# ci/gates/p7.sh — fail-closed p7-conformance gate for this repo's `.handoff/` Tier-A layer.
#
# META-ORG-POLICY **P7** / handoff ADR-0003 + ADR-0004 §2/§3: a member repo's `.handoff/` is the
# git-committed continuity layer — TEXT ONLY, no binary `ledger.db` (the witnessed FLEET ledger lives
# at `$META_ROOT/.handoff/ledger.db`). This gate makes the conformance the loop already verifies by
# hand (capsule/policy/hooks schema tags, residency, the rendered packet) fail-CLOSED. Grep-based and
# dependency-free, mirroring ci/gates/{shape,enable}.sh. Run from the repo root: `bash ci/gates/p7.sh`.
#
# It validates the COMMITTED Tier-A artifacts; it deliberately does NOT invoke any ledger-mutating
# `hf` verb (`hf init`/`status`/`resume`/`sync` open a CWD-relative ledger → would itself violate P7).
# The packet is checked as the static artifact `hf fleet render <member>` produced.
set -euo pipefail
fail() { echo "P7 GATE FAIL: $*" >&2; exit 1; }

HND=".handoff"
[ -d "$HND" ] || { echo "P7 GATE PASS (no .handoff/ — not a continuity member)"; exit 0; }

# --- Gate 1: REQUIRED Tier-A core exists (ADR-0004 §2). ---
[ -f "$HND/context/capsule.json" ] || fail "missing REQUIRED $HND/context/capsule.json"
[ -f "$HND/README.md" ]            || fail "missing REQUIRED $HND/README.md"
[ -d "$HND/tasks" ]                || fail "missing REQUIRED $HND/tasks/ dir"
[ -d "$HND/packets" ]              || fail "missing REQUIRED $HND/packets/ dir"

# --- Gate 2: schema tags pin each artifact to its versioned contract. ---
grep -q '"schema": "handoff.context_capsule.v1"' "$HND/context/capsule.json" \
  || fail "capsule.json missing/!= schema \"handoff.context_capsule.v1\""
# OPTIONAL autonomous-loop descriptors: validate the tag IFF the file exists.
if [ -f "$HND/policies/rules.toml" ]; then
  grep -Eq '^[[:space:]]*schema[[:space:]]*=[[:space:]]*"handoff.policy.rules.v1"' "$HND/policies/rules.toml" \
    || fail "policies/rules.toml missing/!= schema \"handoff.policy.rules.v1\""
fi
if [ -f "$HND/hooks/hooks.toml" ]; then
  grep -Eq '^[[:space:]]*schema[[:space:]]*=[[:space:]]*"handoff.hooks.v1"' "$HND/hooks/hooks.toml" \
    || fail "hooks/hooks.toml missing/!= schema \"handoff.hooks.v1\""
fi
# Every minted task card must carry the task schema. (nullglob: no cards yet → skipped, not an error.)
shopt -s nullglob
for card in "$HND"/tasks/*.task.json; do
  grep -q '"schema": "handoff.task.v1"' "$card" \
    || fail "$card missing/!= schema \"handoff.task.v1\""
done
shopt -u nullglob

# --- Gate 3: ledger residency (ADR-0004 §3 / P7) — NO per-repo ledger, ever. ---
# 3a: nothing ledger-like tracked in git under .handoff.
if git ls-files "$HND" | grep -qE '\.db$|(^|/)ledger\.db$'; then
  fail "a binary ledger (*.db) is git-tracked under $HND — the FLEET ledger lives at \$META_ROOT/.handoff"
fi
# 3b: nothing ledger-like present on disk either (catches an untracked stray about to be added).
if find "$HND" -name '*.db' -type f 2>/dev/null | grep -q .; then
  fail "a *.db file exists under $HND — remove it; the witnessed ledger is fleet-only"
fi
# 3c: the .gitignore guard must be present so a stray ledger can never be committed.
grep -qE '^\.handoff/\*\*/ledger\.db' .gitignore \
  || fail ".gitignore is missing the residency guard '.handoff/**/ledger.db'"

# --- Gate 4: the resume packet, if rendered, is the v2 contract (compiled, not hand-written). ---
if [ -f "$HND/packets/latest.md" ]; then
  grep -q 'handoff.packet.v2' "$HND/packets/latest.md" \
    || fail "packets/latest.md is not a handoff.packet.v2 (re-render via 'hf fleet render <member>')"
fi

echo "P7 GATE PASS"
