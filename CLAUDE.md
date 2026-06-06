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

## Harness: Feature Forge (the construction crew)

**Goal:** turn a feature / upgrade / design request into invariant-verified working Rust, fast —
a design → implement → verify crew. The crew *builds* the feature; it is not the building.

**Trigger:** for any request to add / build / implement / design / upgrade / extend / refactor an
envctl feature, Engine method, CLI/GUI surface, secrets-stack capability, or manifest component
(and follow-ups like "re-run", "fix the guardian's findings", "revise the design"), use the
**`feature-forge`** skill. It drives `feature-architect` → `rust-implementer` →
`invariant-guardian`. For **continuous/autonomous** runs over a backlog ("keep building", "loop on
the roadmap", "run unattended") use **`forge-loop`**; for **cross-session handoff/resume** ("transfer
the session", "resume from handoff") use **`session-relay`** (checkpoints via `continuity-steward`,
coordinates over **weave**, schedules a best-effort successor cron at a per-session cycle budget).
To **provision the whole box / install all toolchains, PATH, and env vars in a loop until
`doctor` is green** ("install everything", "set up the box", "loop until installed"), use
**`env-install-loop`** (the same loop+relay continuity, driving envctl's `doctor`/`install`/
`auto-fix` verbs + `env-toolchain-install`). For **fully unattended, self-restarting** provisioning
with a fresh context every cycle ("run it overnight / set-and-forget", "auto-provision", "cycle
install and reset until done") use **`auto-provision`** — the external Ralph runner that spawns a
fresh `claude -p` per cycle (the `/new` effect) wrapping `env-install-loop`. Simple questions and
trivial edits may be answered/done directly. (A SINGLE component install → `env-toolchain-install`;
drift/lock/doctor → `env-stabilize`; conventions → `agent-env-config`.)

**Placement:** the harness is **hand-authored and git-tracked**, intentionally *outside* the
kasetto pipeline. Agent definitions live in `.claude/agents/*.md` and the harness skills
(`feature-forge`, `rust-feature-impl`, `forge-loop`, `session-relay`, `env-install-loop`,
`auto-provision`) live directly in `.claude/skills/` — edit those files in place and commit them. They are **not** sourced from `agent-skills/`, not in `kasetto.yaml` /
`kasetto.lock`, and not produced by `kasetto sync`. (Note: this is a deliberate exception to the
general "`.claude/skills/*` are kasetto-generated" rule above — the kasetto-managed skills remain
`agent-env-config`, `env-stabilize`, `env-toolchain-install`.)

**Change history:**
| Date | Change | Target | Reason |
|------|--------|--------|--------|
| 2026-06-04 | Initial harness build | agents/{feature-architect,rust-implementer,invariant-guardian}; skills/{feature-forge,rust-feature-impl} | Build a feature-delivery construction crew (design/implement/verify) that upholds the non-negotiable invariants |
| 2026-06-04 | Architect uses return-value (not Write) | agents/feature-architect; skills/feature-forge | Smoke test: `Plan` type is read-only and cannot Write its plan file — orchestrator persists the returned text |
| 2026-06-04 | Add rtk-proxy + baseline-stash guidance | skills/rust-feature-impl/references/verification; skills/feature-forge | Smoke test: rtk summarizes cargo/git output (corrupts fmt/clippy diagnostics); floating `stable`=1.96 causes pre-existing workspace fmt/clippy drift to be mis-attributed to the change |
| 2026-06-04 | Add continuity layer: Ralph loop + session handoff | agents/continuity-steward; skills/{forge-loop,session-relay}; skills/feature-forge | Run Feature Forge continuously over a backlog and survive context rot / token burn — cycle-budget handoff writes a durable checkpoint, coordinates over weave, and schedules a durable-cron successor session |
| 2026-06-05 | Correct relay signal model after full smoke | skills/session-relay | Smoke test: `CronCreate{durable}` is session-only here (not persisted), and a self-identity weave message is invisible to the successor's own inbox. Authoritative resume signal = committed `HANDOFF.md` + cron prompt; weave is a cross-identity (`to:all`) observable heartbeat |
| 2026-06-05 | Add env-install-loop (whole-box provisioning loop) | skills/env-install-loop; agents/continuity-steward; skills/session-relay | First-class loop to drive the workstation to fully-installed/healthy/drift-free via envctl doctor/install/auto-fix + env-toolchain-install, reusing the loop+relay continuity. Generalized continuity-steward + session-relay to serve both the feature and env loops |
| 2026-06-05 | Add auto-provision (self-restarting fresh-context Ralph runner) | skills/auto-provision (+scripts/ralph-provision.sh); skills/env-install-loop | Fully-unattended provisioning that restarts with a fresh context each cycle (the `/new` effect) by spawning a fresh `claude -p` per iteration, wrapping env-install-loop; added install↔reset remediation rung + DONE/NEEDS-HUMAN/STOP sentinels. Safe-by-default (RALPH_APPLY opt-in for unattended apply) |
| 2026-06-05 | Add component-research/audit phase (auto-append upgrades to backlog) | skills/env-install-loop; skills/auto-provision (+scripts/ralph-provision.sh) | Generalize the manual pytorch deep-dive (shallow gate, no-CUDA-assert, verify side-effect, toolkit↔driver skew) into a loop phase: subagents deep-probe each component past detect/verify (real exercise, gate quality, version currency+advisories, cross-component skew, hook hygiene, wiring reach) and append evidence-based, owner-classified items (`harden:`/`fix:`/`upgrade:` loop-fixable; `feature:` routed to feature-forge). Two-tier DONE (Tier-1 provisioned vs Tier-2 upgrades-resolved/routed). `research=` arg + `RALPH_RESEARCH` toggle (default on) |
| 2026-06-05 | Add A2 cross-repo parallel build (default-OFF, scale auto-trigger) | skills/{feature-forge,forge-loop,session-relay}; agents/{rust-implementer,continuity-steward} | Cross-repo parallelism via the three-owner split — **meta** owns the coordinated worktree set (one independent branch per repo → no cross-repo conflict by construction) + aggregation (`meta --json git worktree exec --parallel`), **grit** owns intra-repo `file::symbol` locks only (Option X: `init/claim/release/heartbeat/gc/status/queue`, never `done`/`session`/`worktree`), the **orchestrator** owns the guardian gate (only it commits/merges/PRs, only after that repo's guardian PASSes). Auto-trigger by scale (1 repo ≤3 mod → sequential DEFAULT; 1 repo >3 mod → `Workflow.pipeline`; >1 repo → A2) with `FORGE_PARALLEL=0` escape hatch; sequential single-crew unchanged when no >1-repo trigger fires. PR-1 = minimal-coherent foundation (envctl-style gate scope + schema/2-repo continuity demo); per-repo gate contracts, grit-lifecycle inversion, full N-branch resume, dep-ordered fan-out staged to PR-2..5 |
| 2026-06-05 | Per-repo invariant-contract descriptor (gates as data) — PR-2 | .forge/invariants.toml; skills/feature-forge (Phase 2-A2 step 5); agents/invariant-guardian | Replace the hardcoded "envctl → 3 gates; else degrade to fmt/clippy/test" A2 prose with a portable, declarative per-repo `.forge/invariants.toml` (ordered `[[gate]]` list; `kind=shell|cargo`, `required`; fail-closed result mapping) that each repo owns. envctl ships its own descriptor encoding exactly today's gates (no behavior change); a repo without one falls back to generic-Rust (fmt --check / clippy -D warnings / test) + a NOTE. Sequential single-crew path unchanged (descriptor consumed only in the A2 path) |
