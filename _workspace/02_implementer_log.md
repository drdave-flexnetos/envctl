# Implementation log: node-via-bun manifest truth-telling fix (bun-first, zero engine change)

Conservative, manifest-only fix per `_workspace/01_architect_plan.md`. No engine/Rust logic
changed — only manifest TOML, the regenerated lock, a docs note, and engine *test* additions
(+ a stale-comment update). Bun stays the default JS runtime; real node v22 retained as the
narrow n8n carve-out.

## Changes
- `manifest/base.toml`: rewrote `node-via-bun` detect hook (truthful: succeeds when EITHER the
  `~/.bun/bin/node→bun` shim exists OR a real `node` resolves on PATH) and verify hook
  (`node -e 'process.exit(0)'` instead of `node --version`, which bun's shim cannot satisfy by
  design). id/name/requires/install/fix/remove unchanged.
- `manifest/base.toml`: added new `node-real` component (Real Node 20–24 for V8-only tools like
  n8n/isolated-vm) immediately after `node-via-bun`, before `rustup`. Standalone (no `requires`);
  no `remove` hook by design (avoids breaking n8n); `wiring.path_entries = ["~/.local/bin"]`.
- `manifest/ai-clis.toml`: dropped `"node-via-bun"` from `group-ai-clis.requires` (false edge —
  its detect probes only the 5 CLIs) and updated the description.
- `manifest/envctl.lock`: regenerated via `envctl lock` — 49 → 50 components.
- `docs/DESIGN-NOTES.md`: added a "JS runtime: bun-first, with a narrow real-node carve-out"
  section (no pre-existing JS section existed in docs/ to extend; DESIGN-NOTES was the plan's
  fallback target).
- `crates/engine/tests/engine.rs`: added two regression tests; updated the stale comment at the
  `reverse_dependents_transitive` assertion (line ~212) from "transitive (via node-via-bun)" to
  "transitive (via codex-cli/gemini-cli -> bun)". The assertion itself (line ~211) is unchanged
  and still passes (group-ai-clis remains a transitive reverse-dep of bun via codex/gemini).

## Engine API
**None.** No new/changed Engine method, Event, or type. The bug lived entirely in manifest data;
the engine's detect→verify→drift pipeline is already correctly data-driven. Parity contract is
unchanged — CLI and GUI consume the same (unmodified) Engine API.

## Tests added
- `group_ai_clis_does_not_require_node_via_bun` — proves `group-ai-clis.requires` excludes
  `"node-via-bun"` and includes all five CLIs (claude-code-cli, codex-cli, gemini-cli, kimi-cli,
  devin-cli). Regression for the dropped false edge.
- `node_real_component_exists_with_empty_requires` — proves `reg.get("node-real")` exists and its
  `requires` is empty (standalone n8n carve-out).
- Existing at-risk test `reverse_dependents_transitive` stays GREEN (comment updated only).

## Build/test status
All commands run from the worktree root via `rtk proxy` (so cargo/CLI output + exit codes are
not reshaped). Exit codes captured.

- `rtk proxy cargo build -p envctl-engine -p envctl` → Finished, **exit=0**
- `rtk proxy cargo test -p envctl-engine` → **20 passed; 0 failed**, exit=0
  (incl. new tests + `reverse_dependents_transitive` + graph cascade tests)
- `rtk proxy cargo fmt --all` then `--check` → **fmt-check-exit=0** (fmt reformatted the new test
  body; re-checked clean)
- `rtk proxy cargo clippy --workspace -- -D warnings` → Finished, **clippy-exit=0**
- `rtk proxy cargo test --workspace` → all suites pass, **test-exit=0**
  (engine 20, secrets-engine 70, relay 17, vault 15, store-libsql 11, phase0 6, …; the 7
  `integration_remote` tests are `ignored` — require a running sqld, pre-existing)
- `rtk proxy cargo run -p envctl -- lock` → `wrote manifest/envctl.lock (50 components)`, exit=0
- `rtk proxy cargo run -p envctl -- lock --check` →
  `✓ envctl.lock matches the manifest (50 components)`, **exit=0**
- `bash ci/gates/no-c.sh` → `NO-C GATE PASS`
  (`resolved graph clean: rustls=['0.23.40'] on ring=['0.17.14']; zero aws-lc/openssl/C-SQLite`), exit=0
- `bash ci/gates/shape.sh` → `SHAPE GATE PASS`, exit=0
- `bash ci/gates/enable.sh` → `ENABLE GATE PASS`, exit=0

### auto-detect --json (JS-runtime components) — exit=0
```
{"id": "node-real",     "detected": true, "healthy": true}
{"id": "node-via-bun",  "detected": true, "healthy": true}
{"id": "group-ai-clis", "detected": true, "healthy": null}   # meta: no verify hook, detected:true
drift list length: 0   # NO spurious node-via-bun drift, NO Unhealthy JS item
```

### doctor (JS line) — exit=0
```
✓ bun         1.3.14
```
`grep -niE "drift|unhealthy|node-via-bun|node-real|broken|partial"` over full doctor output →
no matches (grep-exit=1). JS-runtime story is truthfully green with zero spurious drift.

### Lock delta
49 → **50** components (`node-real` added). Verified: `grep -c "^\[components\." envctl.lock` = 50;
`node-real` present in the lock once.

## Deviations
None. Implemented exactly as the plan specified (Steps 1–5 + the recommended tests). The only
incidental note: `cargo fmt` reformatted the multi-line `reg.get("group-ai-clis")…requires`
expression in the new test — cosmetic, expected, and re-checked clean.

## Handoff notes (for invariant-guardian)
- **Zero engine code change** — confirm `crates/engine/src/**` is untouched (only
  `crates/engine/tests/engine.rs` changed, tests + one comment). No `println!` added to engine.
- **Guards untouched** — `guard.rs` / `executor.rs` reverse-dep refusal logic unchanged. We only
  removed a *false* manifest edge (`group-ai-clis → node-via-bun`); we did NOT weaken any guard.
  `reset node-via-bun` no longer needs to cascade-protect group-ai-clis because the edge is gone,
  but the fail-closed machinery itself is intact (still protects bun via codex/gemini and protects
  node-via-bun→bun, etc.).
- **no-C trust boundary**: manifest-only change adds no deps; `ci/gates/no-c.sh` re-run GREEN.
- **Lock honesty**: re-locked to 50 components; `lock --check` clean.
- **`node-real` has no `remove` hook by design** — intentional (removal would break n8n). Verify
  the guardian does not flag the missing remove hook as incompleteness; it's a deliberate footgun
  guard, documented in base.toml and DESIGN-NOTES.md.
- `_workspace/01_architect_plan.md` shows as modified in `git status` — that is the architect's
  persisted plan from the orchestrator, NOT touched by this implementer step.

STATUS: GREEN
