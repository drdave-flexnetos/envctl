#!/usr/bin/env bash
# ci/gates/enable.sh — the Phase-6 / OI-1 enable gate (post-Phase-6 inverted form).
#
# Materialized from docs/ops/02-envctl-component.md §4.3. PRE-Phase-6 this gate asserted the manifest
# kept `enable = false` while `crates/secretd/src/main.rs` was the `todo!("secretd server bring-up")`
# scaffold (so an auto-enabled unit could not panic-loop into a false "vault is up"). Phase 6 has
# landed, so the gate now asserts the INVERSE invariant, fail-closed:
#   (1) main.rs is no longer the scaffold — the daemon actually serves, AND
#   (2) IF the unit ships enabled, its `verify` hook's `secretd --self-check` surface must exist in
#       both the manifest (the hook calls it) and the source (the subcommand is defined). An enabled
#       unit whose verify references a missing subcommand would wire a verify that can never pass —
#       and a bare `secretd` with no `--self-check` would hang the hook by serving forever.
# Run from the repo root: `bash ci/gates/enable.sh`.
set -euo pipefail
fail() { echo "ENABLE GATE FAIL: $*" >&2; exit 1; }

MAIN=crates/secretd/src/main.rs
MANIFEST=manifest/env-ctl.toml

[ -f "$MAIN" ]     || fail "missing $MAIN"
[ -f "$MANIFEST" ] || fail "missing $MANIFEST"

# (1) Phase 6 must really be done: main.rs is no longer the `todo!()` scaffold.
if grep -q 'todo!("secretd server bring-up' "$MAIN"; then
  fail "secretd main.rs is still the Phase-6 todo!() scaffold — the manifest must keep enable=false"
fi

# (2) If the systemd unit ships enabled, the verify hook's `secretd --self-check` surface MUST exist.
if grep -Eq '^[[:space:]]*enable[[:space:]]*=[[:space:]]*true' "$MANIFEST"; then
  grep -q -- 'secretd --self-check' "$MANIFEST" \
    || fail "manifest enables the unit but its verify hook does not invoke 'secretd --self-check'"
  grep -q -- 'self-check' "$MAIN" \
    || fail "manifest enables the unit but secretd defines no --self-check subcommand"
fi

echo "ENABLE GATE PASS"
