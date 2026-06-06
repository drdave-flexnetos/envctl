# Verification report: node-via-bun manifest truth-telling fix (bun-first, zero engine change)

## Verdict
**VERDICT: PASS-WITH-NOTES** — every invariant and gate is green; the change is exactly
manifest + docs + engine *tests* (zero `src/` change). One non-blocking note: a pre-existing
flaky dashboard test (`deploy_refuses_foreign_file_without_force`) failed once under full
parallel load, then passed 5/5 on re-run; it is unmodified by this change and unrelated to it.

---

## Gate results
| Gate | Result | Evidence |
|------|--------|----------|
| `bash ci/gates/no-c.sh`  | **PASS** (exit=0) | `resolved graph clean: rustls=['0.23.40'] on ring=['0.17.14']; zero aws-lc/openssl/C-SQLite` → `NO-C GATE PASS` |
| `bash ci/gates/shape.sh` | **PASS** (exit=0) | `SHAPE GATE PASS` |
| `bash ci/gates/enable.sh`| **PASS** (exit=0) | `ENABLE GATE PASS` |

## cargo (via `rtk proxy`)
| Check | Result | Evidence |
|-------|--------|----------|
| `cargo fmt --all -- --check` | **PASS** (exit=0) | clean |
| `cargo clippy --workspace -- -D warnings` | **PASS** (exit=0) | `Finished` |
| `cargo test --workspace` | **PASS-on-rerun** | engine lib: first full-workspace run hit a flaky `dashboard::tests::deploy_refuses_foreign_file_without_force` (dashboard.rs:658, unrelated); engine lib re-ran 31 passed / 0 failed **2/2**; failing test re-ran isolated **3/3 ok** |

At-risk / new tests (explicitly re-run, all **ok**):
- `reverse_dependents_transitive` (engine.rs:211) — **ok** (still holds via codex/gemini → bun)
- `impact_closure_and_cascade` (graph.rs cascade, ~295) — **ok**
- `group_ai_clis_does_not_require_node_via_bun` (new) — **ok**
- `node_real_component_exists_with_empty_requires` (new) — **ok**

## Invariant checks (1–8)
1. **No C in trust boundary** — **PASS**. `no-c.sh` green; resolved graph clean; **no Cargo.toml/Cargo.lock change** (`git diff --name-only | grep Cargo` → none). Manifest+tests-only change adds zero deps; no banned crate (SQLite/OpenSSL/aws-lc) introduced.
2. **Code-shape invariants** — **PASS**. `shape.sh` green.
3. **secretd enable invariant** — **PASS**. `enable.sh` green.
4. **Engine purity** — **PASS**. `git diff crates/engine/src/` is **empty** (zero src change). Grep for `+`-added `println!/eprint!/print!/stdout` in engine src → **none** (exit 1). All new logic is manifest data; no logic added to `main.rs`/GUI.
5. **Front-end parity** — **PASS (N/A by design)**. No new/changed Engine method, Event, or type — confirmed by the implementer log and zero `src/` diff. CLI and GUI consume the identical unmodified Engine API; nothing to diverge.
6. **Fail-closed + dry-run defaults** — **PASS / not weakened**. `crates/engine/src/guard.rs` and `crates/engine/src/executor.rs` are **untouched** (`git diff --name-only` shows neither). The change removed a *false* manifest edge (`group-ai-clis → node-via-bun`); it did not relax any guard. The reverse-dep refusal path remains exercised by `reverse_dependents_transitive` (passes). `node-real` deliberately has no `remove` hook (documented footgun guard for n8n) — not an incompleteness.
7. **Rust-native, no drift** — **PASS**. No new non-Rust source/package files. Only `manifest/*.toml`, `docs/DESIGN-NOTES.md`, `manifest/envctl.lock`, and `crates/engine/tests/engine.rs` changed. `node-real`'s install/fix shell hooks are component-script bodies (the sanctioned manifest mechanism), not workspace source files.
8. **Lock honesty** — **PASS**. `cargo run -p envctl -- lock --check` → `✓ envctl.lock matches the manifest (50 components)` (exit=0). Lock diff reflects all three manifest changes: `group-ai-clis` content_hash `120fb5a6…` → `3bd21a9b…` and `node-via-bun` dropped from its requires; `node-via-bun` content_hash `34fb0dc5…` → `de3c6556…` (hooks rewritten); `node-real` added with `requires = []`.

## Parity check
N/A — no Engine method added or changed (invariant 5). The bug lived entirely in manifest data;
the engine's detect→verify→drift pipeline is already data-driven and was not modified.

## The actual goal — JS story truthfully green
`cargo run -p envctl -- auto-detect --json` (exit=0):
- `node-via-bun` → `detected: true, healthy: true`
- `node-real`    → `detected: true, healthy: true`
- `group-ai-clis`→ `detected: true, healthy: null` (meta, no verify hook — correct)
- `"drift": []` — **empty**: NO spurious `node-via-bun` drift item, NO Unhealthy JS item.

`cargo run -p envctl -- doctor` (exit=0): toolchains block all green incl. `✓ bun 1.3.14`;
grep over doctor output for `drift|unhealthy|broken|partial` referencing JS → none.

**No cascade**: `group-ai-clis` still `detected: true` healthy; its five members
(claude/codex/gemini/kimi/devin) untouched (only the description + requires array edited).
bun retained as default (still required by codex/gemini). Real node v22 retained via `node-real`.

## Findings
| Severity | Location | What | Suggested fix |
|----------|----------|------|---------------|
| **note (non-blocking, pre-existing)** | `crates/engine/src/dashboard.rs:658` test `deploy_refuses_foreign_file_without_force` | Flaky under full-workspace parallel test load: failed once, then passed 5/5 (3× isolated, 2× full engine-lib). File is **unchanged by this work** (last touched commit a4b35c4, the dashboard pass). Self-contained temp-dir test → a parallel/FS-timing race, not a regression. | Not in scope for this change. If desired later, make the test serial or use a per-test unique subdir guard. Route as a separate, low-priority backlog item — NOT a blocker for this manifest fix. |

No blocking findings. No invariant weakened to force a pass.

## Re-test needed
None required for this change. To re-confirm after any future touch:
```
bash ci/gates/no-c.sh; bash ci/gates/shape.sh; bash ci/gates/enable.sh
rtk proxy cargo fmt --all -- --check
rtk proxy cargo clippy --workspace -- -D warnings
rtk proxy cargo test -p envctl-engine          # 31 pass (rerun if dashboard flake appears under -j)
rtk proxy cargo run -p envctl -- lock --check  # 50 components
rtk proxy cargo run -p envctl -- auto-detect --json   # drift:[] ; node-via-bun & node-real healthy:true
```

VERDICT: PASS-WITH-NOTES
