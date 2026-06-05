---
name: rust-feature-impl
description: "The envctl feature-delivery recipe — how to implement a feature/upgrade in this pure-Rust workspace so it lands clean: engine-first architecture, front-end parity, fail-closed guards, no-C trust boundary, and lock/manifest sync. ALWAYS use when writing, extending, or wiring envctl Rust code (engine/cli/gui/secretd/secrets-*), adding an Engine method or Event, touching a destructive op, or before pushing a change that affects deps or the trust boundary. Pairs with agent-env-config (naming/test/commit conventions) and feature-forge (the orchestrator)."
---

# envctl Feature Delivery (engine-first, invariant-safe)

This skill is the **how** for building a feature in envctl. It assumes the **what/where** is
already decided (by the `feature-architect` plan) and the **conventions** are owned by the
`agent-env-config` skill — read that for naming, test layout, and commit style; this skill does
not repeat them. The authoritative invariants live in the repo `CLAUDE.md`; the table below is
the working checklist, not a substitute for reading it.

## The one rule that shapes everything: engine-first

`crates/engine` is the **single shared library**. It is **sync, pure-Rust, and non-printing** —
it emits `Event`s, never `println!`, has no UI and no clap. The CLI (`envctl`) and GUI
(`envctl-gui`) are thin front-ends that drive the *identical* `Engine` API, which is what stops
them diverging.

**Therefore, the delivery order is always:**

1. **Engine** — add the method / `Event` variant / type that carries the new behavior. Logic
   lives here. Emit events for anything a front-end might display; return data, don't print it.
2. **CLI** — wire `crates/cli` to the new Engine method; render its events/return for the
   terminal (clap-side parsing + printing belongs here, not in the engine).
3. **GUI** — wire `crates/gui` to the **same** Engine method so the front-ends stay at parity.
   If you added a capability to one front-end, expose the equivalent in the other (or the plan
   must justify the asymmetry).
4. **Tests** — beside the code and as integration/e2e per the plan.

If you find yourself writing real logic in `main.rs` or in the GUI, stop — it belongs in the
engine.

## NON-NEGOTIABLE invariants (a change that breaks one is a regression)

| # | Invariant | What it means in practice |
|---|-----------|---------------------------|
| 1 | **No C in the trust boundary** | Never add a dep that pulls SQLite/OpenSSL/aws-lc. libSQL store is `remote` only (`default-features = false`); crypto is pure-Rust (ring, blake3, chacha20poly1305, argon2). Proven by `ci/gates/no-c.sh`. |
| 2 | **Exactly one rustls, ring-only** | Every TLS/CA crate pins `features = ["ring"]`; never aws-lc-rs. |
| 3 | **Engine is the shared, non-printing lib** | Sync, pure-Rust, emits `Event`s, no `println!`, no UI, no clap. CLI + GUI drive the identical API. |
| 4 | **Destructive ops fail-closed + dry-run by default** | Guards (`UuidResolves`, `NotLiveDevice`, `NotMounted`) refuse without proof of safety. Mutation needs explicit `--apply`/`--build`. Unit-test the refusal. |
| 5 | **Rust-native only** | No new non-Rust source/package files. A stray `.omc` or an ECC-pushed JS/Node package is **drift** — don't commit it; the sanctioned port path is `add-repo --refactor=ai --goal port-to-rust`. |
| 6 | **Reproducible state** | If deps/components change, sync `envctl.lock` / `kasetto.lock` / `manifest/*.toml` so the locked state still reflects reality. |

For the exact verification commands and what each proves, read
`references/verification.md` — load it before you claim a change is done, and the
`invariant-guardian` runs the same recipe independently.

## Build / test / lint (run from the worktree root)

```bash
cargo build -p envctl-engine -p envctl                  # tight inner loop, zero system deps
cargo run  -p envctl -- auto-detect                      # read-only, safe anytime (--json for EnvReport)
cargo test -p <crate>                                    # the crate you touched
cargo test --workspace                                   # everything
cargo fmt --all && cargo clippy --workspace -- -D warnings   # must be clean before commit
```

GUI (`cargo run -p envctl-gui`) needs system dev libs — see README "Native GUI". MSRV 1.80,
stable toolchain.

## Adding an Engine method — the parity pattern

1. Define the method on the `Engine` (or the relevant sub-API) in `crates/engine/src/`. Keep it
   sync and pure; return a typed result and/or emit `Event`s. No printing.
2. If it surfaces progress/results, add or reuse an `Event` variant; both front-ends render it.
3. Before changing an existing signature, check callers with code intelligence
   (`git-kb code callers <symbol> --json` or `kb_callers`) — both the CLI and GUI are callers,
   plus tests. Update every call site.
4. CLI: parse args (clap) → call the Engine method → render. GUI: control → same Engine method →
   render. The Engine call is identical from both sides.
5. Unit-test the engine logic; for destructive paths, test that the guard refuses without
   `--apply`.

## Destructive / mutating ops — the fail-closed recipe

- Default to **preview** (dry-run). The mutation only happens behind `--apply` (or `--build` for
  the relevant verbs).
- Gate the mutation on the proving guard: `UuidResolves` (the target UUID still resolves to the
  expected device), `NotLiveDevice` (refuse to touch the running system disk), `NotMounted`
  (refuse to operate on a mounted target). The guard **refuses when it cannot prove safety** —
  that's the point; never make a guard pass by assuming.
- Add a unit test that the op **refuses** in the unsafe case, not just that it works in the safe
  case.

## Commit & finish

- Area-prefixed subject (`engine:`, `cli:`, `gui:`, `secretd:`, `secrets-store-libsql:`,
  `docs:`); body explains *why*. Conventional-commit prefixes welcome.
- Before pushing anything touching deps or the trust boundary, run all three gates
  (`no-c.sh`, `shape.sh`, `enable.sh`) plus `fmt`/`clippy`/`test` — see `references/verification.md`.
- This repo lives in the `meta` workspace: do work in an isolated worktree, never on a stale or
  dirty `master`.
