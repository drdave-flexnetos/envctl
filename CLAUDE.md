# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`envctl` is a **pure-Rust Cargo workspace** (8 crates) that declaratively manages a
dual-RTX-5090 Ubuntu workstation. Two halves share one engine:

- **env-manager** — `engine` + `cli` (`envctl`) + `gui` (`envctl-gui`). Brings the box to
  a declared state via TOML *components* whose lifecycle hooks wrap the proven bash in
  `assets/scripts/`. Verbs: `auto-detect`, `install`, `auto-fix`, `reset`, `add-repo`,
  `graph`, `lock`, `doctor` (see `README.md`).
- **secrets stack** — `secrets-engine` (pure-Rust crypto vault), `secrets-proto` (tonic/prost
  gRPC), `secretd` (async tokio daemon), `secretctl` (client), `secrets-store-libsql`
  (libSQL **remote** backend). Design corpus in `docs/secrets/`.

## Session start: work in a fresh git worktree (mandatory)

This repo lives inside the `meta` workspace. **Begin every session by creating an isolated
worktree** rather than editing the checked-out tree directly. After verifying sync
(`git fetch && git status` — confirm clean and even with `origin/master`):

```bash
meta git worktree create <task-slug>     # preferred: meta-managed, multi-repo aware
# or, single-repo: git worktree add ../envctl-<task-slug> -b <task-slug>
```

Do all work in the worktree; never start coding on a stale or dirty `master`.

## Build / test / lint

```bash
cargo build -p envctl-engine -p envctl       # engine + CLI, zero system deps
cargo run  -p envctl -- auto-detect          # read-only, safe anytime (add --json for EnvReport)
cargo run  -p envctl-gui                      # needs system dev libs — see README "Native GUI"
cargo test --workspace                        # all crates
cargo test -p envctl-secrets-engine vault     # single crate / filter by test name
cargo test -p envctl-secretd --test e2e       # one integration test file (daemon e2e)
cargo fmt --all && cargo clippy --workspace -- -D warnings   # must be clean before commit
```

Tests are inline `#[cfg(test)] mod tests` beside the code, or `crates/<crate>/tests/*.rs`
integration tests (`#[tokio::test]` for the async daemon path). MSRV 1.80, stable toolchain
(`rust-toolchain.toml`).

## CI gates — run before pushing anything that touches deps or the trust boundary

```bash
bash ci/gates/no-c.sh     # supply-chain: forbids C in the trust boundary (see below)
bash ci/gates/shape.sh    # code-shape invariants (native-roots, edge module)
bash ci/gates/enable.sh   # secretd systemd-unit enable invariant
```

## NON-NEGOTIABLE invariants (a change that breaks these is a regression)

- **No C library in the trust boundary.** No SQLite/OpenSSL/aws-lc may be *linked*. The store
  uses libSQL `remote` only (`default-features = false`); crypto is pure-Rust (ring, blake3,
  chacha20poly1305, argon2). `ci/gates/no-c.sh` proves this fail-closed from the resolved
  `cargo metadata` graph — **never add a dependency that pulls one of the banned crates in.**
- **Exactly one rustls, ring-only** (not aws-lc-rs). All TLS/CA crates pin `features = ["ring"]`.
- **The engine is the single shared library** (`crates/engine/src/lib.rs`): sync, pure-Rust,
  **non-printing** (emits `Event`s, never `println!`), no UI, no clap. CLI and GUI both drive
  the *identical* `Engine` API so the front-ends can't diverge. Put logic in the engine, not in
  `main.rs` or the GUI.
- **Destructive ops are fail-closed and dry-run by default.** Guards (`UuidResolves`,
  `NotLiveDevice`, `NotMounted`) *refuse* when they can't prove safety (unit-test enforced).
  `auto-fix`/`reset`/`add-repo` default to preview; mutation needs `--apply`/`--build`.

## CRITICAL: keep everything rust-native — detect and reverse language drift

This is a **pure-Rust** workspace by design. Watch for and immediately correct any drift toward
another language or toolchain:

- **No new non-Rust source/package files** should appear in the workspace. If an external tool
  emits one — e.g. a stray `.omc` file, or **ECC auto-pushing a JS/Node package** — treat it as
  drift, not as intended state.
- **When drift is found:** (1) verify it (don't act on a false positive — confirm the file/dep
  is actually language drift and not an accepted build-time artifact like the libSQL parser's
  `lemon.c` codegen, which emits Rust and links nothing); (2) **transform it to a rust-native
  equivalent** (a workspace crate, a TOML component, a pure-Rust dependency); (3) **sync it
  properly** into the codebase — add the crate to `Cargo.toml` `members`, wire it through the
  `Engine` API, and update `kasetto.lock`/`envctl.lock` so the reproducible state reflects it.
- The `add-repo --refactor=ai --goal port-to-rust` verb is the sanctioned path for porting an
  external repo into the workspace as a Rust crate. Use it (or its design as a template) rather
  than carrying foreign-language code as-is.

## Agent environment is kasetto-managed — do NOT hand-edit ECC files

The `.claude/` and `.codex/` agent config (skills + MCP baseline) is **provisioned and locked
by kasetto** (`kasetto.yaml` → `kasetto.lock`), sourced from `./agent-skills`. It supersedes the
**ECC-auto-generated** files, which were derived from a misread and assert **JavaScript**
conventions (camelCase, `*.test.ts`, JS imports) — those are **wrong for this repo**.

- **Source of truth for conventions:** the `agent-env-config` skill. Rust idiom: snake_case
  files/modules/functions, PascalCase types, SCREAMING_SNAKE_CASE consts, `#[cfg(test)]` tests,
  area-prefixed commit subjects (`engine:`, `secretd:`, `docs:`). Ignore any ECC instinct/skill
  that says otherwise.
- **To change the agent env:** edit `agent-skills/` + `kasetto.yaml`, then `kasetto sync`.
  Do **not** hand-maintain `.claude/skills/*` or `.claude/homunculus/instincts/*` — they're
  generated. CI enforces with `kasetto sync --locked` (fails on drift).
- Keep the MCP baseline identical across Claude (`.mcp.json`) and Codex (`.codex/config.toml`):
  `github`, `context7`, `exa`, `memory`, `playwright`, `sequential-thinking`.

## Pointers

- `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, `docs/DESIGN-NOTES.md` — env-manager design.
- `docs/secrets/SERVER-MODE.md`, `THREAT-MODEL.md`, `DESIGN-NOTES.md` — secrets-stack design;
  feature IDs (F12/F14/F15, OI-*, CF-*) referenced in commits and gate comments live here.
- `manifest/*.toml` — declarative components; drop-ins land in `manifest/components.d/`.
- The manifest dir defaults to `./manifest` (override with `ENVCTL_MANIFEST_DIR`).
- Logging: `RUST_LOG` (e.g. `RUST_LOG=envctl_engine=debug`).
