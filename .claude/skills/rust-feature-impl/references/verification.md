# Verification recipe — prove a change is invariant-safe before it ships

Load this before claiming a feature is done. The `invariant-guardian` agent runs the same
recipe independently — the implementer running it first means fewer round-trips. Every command
runs from the **worktree root**. Treat an *errored* command (not just a failed assertion) as a
FAIL, fail-closed — never read an empty/errored result as "clean."

## Table of contents
1. The three CI gates
2. Cargo checks (fmt / clippy / test)
3. Engine purity (non-printing, logic-in-engine)
4. Front-end parity (CLI ↔ GUI ↔ Engine)
5. Fail-closed / dry-run defaults
6. Rust-native drift
7. Lock / manifest honesty
8. Full pre-push sequence

---

## 1. The three CI gates

```bash
bash ci/gates/no-c.sh     # supply chain: no C in the trust boundary; one ring-only rustls
bash ci/gates/shape.sh    # code-shape: no native-roots/accept-invalid TLS; edge isolation
bash ci/gates/enable.sh   # secretd systemd-unit enable invariant
```

- **`no-c.sh`** parses the resolved `cargo metadata` graph and fails CLOSED if any of
  `aws-lc-sys`, `aws-lc-rs`, `openssl-sys`, `libsql-ffi`, `libsql-sys`, `sqlite3-sys`, `rusqlite`
  resolve into the graph, if more than one `rustls` version exists, or if `ring` is absent. Run
  it after **any** dependency change. A build-time `cc`/`lemon.c` (libSQL parser codegen that
  emits Rust and links nothing) is accepted — that is not a violation.
- **`shape.sh`** greps non-test Rust source for forbidden TLS tokens
  (`danger_accept_invalid_certs`, `rustls-tls-native-roots`, `use_native_tls`, …) and enforces
  edge-module isolation once the Phase-8 edge lands.
- **`enable.sh`** asserts `crates/secretd/src/main.rs` is no longer the `todo!()` scaffold and
  that an enabled systemd unit has a matching `secretd --self-check` surface in both manifest and
  source.

## 2. Cargo checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace                 # or -p <crate> during the incremental pass
```

Never silence a clippy finding with a broad `#[allow]` to pass; fix the code or justify a narrow,
commented allow.

## 3. Engine purity

The engine library emits events; it does not print, and it holds the logic.

```bash
# No printing added to the engine library path:
git diff --unified=0 -- crates/engine | grep -nE '^\+.*(println!|eprint(ln)?!|print!|stdout\(\))' && echo "VIOLATION: engine printed" || echo "engine clean"
```

Also confirm by reading: new behavior lives in `crates/engine/src/`, not in `crates/cli/src/main.rs`
or `crates/gui`. clap/UI/printing belong to the front-ends only.

## 4. Front-end parity (the core cross-boundary check)

For each new or changed `Engine` method, prove **both** front-ends reach it (or the plan
justified an asymmetry). Don't existence-check — compare caller shapes.

```bash
# callers of the symbol across the workspace (AST-aware, not text):
git-kb code callers '<EngineMethod>' --json     # or kb_callers MCP
```

Expect to see a CLI call site (`crates/cli/...`) and a GUI call site (`crates/gui/...`). Read
both and confirm they pass/consume the same shape the engine expects/returns.

## 5. Fail-closed / dry-run defaults

For any destructive/mutating op introduced or touched:

- Confirm the default path is **preview** and mutation is gated behind `--apply`/`--build`.
- Confirm the guard (`UuidResolves` / `NotLiveDevice` / `NotMounted`) is invoked **before**
  mutation and refuses when it cannot prove safety.
- Confirm a unit test exercises the **refusal** path:

```bash
cargo test -p <crate> -- <guard_or_refuse_test_name>
```

A mutation op with only a happy-path test is incomplete.

## 6. Rust-native drift

```bash
# Any non-Rust source/package file newly added? (worktree against master)
git status --porcelain | grep -E '\.(js|ts|jsx|tsx|mjs|cjs|py|rb|omc)$|package\.json$|node_modules' \
  && echo "POSSIBLE DRIFT — verify before committing" || echo "no foreign source added"
```

A hit is not automatically a failure — confirm whether it is genuine language drift (reject /
port to Rust) or an accepted build-time artifact (e.g. the libSQL parser's `lemon.c` codegen,
which emits Rust and links nothing). When in doubt, treat as drift and report.

## 7. Lock / manifest honesty

If dependencies or declared components changed:

```bash
cargo run -p envctl -- lock --check          # envctl.lock still matches declared components
kasetto sync --locked                         # agent-env lock still satisfied (no fetch)
```

A green lock check means the committed reproducible state still reflects reality.

## 8. Full pre-push sequence

```bash
cargo fmt --all -- --check \
 && cargo clippy --workspace -- -D warnings \
 && cargo test --workspace \
 && bash ci/gates/no-c.sh \
 && bash ci/gates/shape.sh \
 && bash ci/gates/enable.sh \
 && echo "ALL GATES GREEN"
```

Only `ALL GATES GREEN` plus the parity + fail-closed + drift checks above earns a PASS verdict.
