---
name: agent-env-config
description: "The CORRECT conventions and agent-environment configuration for the envctl Rust workspace — supersedes the broken ECC-auto-generated skill/instincts that assert JavaScript conventions. Use whenever writing or reviewing envctl code, naming files/types, writing tests, composing commits, or configuring the .claude/.codex agent setup (skills, MCP servers, multi-agent roles). Triggers: 'what conventions', 'how do I name this', 'write a test', 'commit message', 'configure the agents', 'MCP setup', 'is camelCase right'."
---

# Agent Environment Config & Conventions (envctl)

envctl is a **pure-Rust** Cargo workspace (8 crates: engine, cli, gui, secrets-engine, secrets-proto, secretd, secretctl, secrets-store-libsql). The ECC-auto-generated config (`.claude/skills/envctl/SKILL.md`, `.claude/homunculus/instincts/.../envctl-instincts.yaml`) was derived from a misread and asserts **JavaScript** conventions. **This skill is the source of truth; the ECC conventions are wrong — ignore them.**

## Corrections — ECC says X, the truth is Y

| ECC (WRONG) | Correct for envctl (Rust) |
|-------------|---------------------------|
| camelCase file names (`envManager.rs`) | **snake_case** files (`env_manager.rs`) |
| camelCase function names | **snake_case** functions (`load_env`, `relay_mint`) |
| "relative imports" JS-style (`import {x} from '../lib/x'`) | Rust `use` paths: `use crate::vault::store;`, `use super::*;` |
| named exports | `pub` items in modules; `mod` tree declares structure |
| test files named `*.test.ts` | `#[cfg(test)] mod tests { ... }` in the same `.rs`, or `crate/tests/*.rs` integration tests |
| (no lint guidance) | `cargo fmt` + `cargo clippy -- -D warnings`; **no C in the trust boundary** (`ci/gates/no-c.sh`) |

**Correct conventions that ECC happened to get right (keep):** PascalCase for structs/enums (`BearerRow`, `RemotePeer`), SCREAMING_SNAKE_CASE for consts, commit subjects prefixed by area (`envctl:` / `engine:` / `secretd:`).

## Code Conventions

- **Naming:** snake_case modules/files/functions/vars; PascalCase types/traits/enum variants; SCREAMING_SNAKE_CASE consts/statics.
- **Module structure:** the `mod` tree is the API surface; expose with `pub`. Prefer `use crate::…` / `use super::…`. No JS-style relative-path imports (they don't exist in Rust).
- **Tests:** unit tests in `#[cfg(test)] mod tests` beside the code; integration tests in `crates/<crate>/tests/*.rs`; e2e where a daemon is involved (e.g. `secretd/tests/e2e.rs`). Run with `cargo test -p <crate>` or `cargo test --workspace`.
- **Lints & safety:** MSRV 1.80, stable toolchain. `cargo fmt` and `cargo clippy -- -D warnings` must be clean. The engine is sync, pure-Rust, **non-printing** (emits events, doesn't println). The supply-chain gate `ci/gates/no-c.sh` forbids linking any C library into the trust boundary — never add a dep that pulls one in.
- **Commits:** concise subject prefixed by area (`engine:`, `secretd:`, `secrets-store-libsql:`, `docs:`); body explains why. (git-cliff-style conventional prefixes are welcome.)

## Agent Environment Layout

The environment targets two agent runtimes; keep them consistent (kasetto manages this — see `env-stabilize`):

- **Claude Code** → `.claude/` : skills under `.claude/skills/<name>/SKILL.md`, plus `.claude/settings*.json`. Do NOT hand-maintain `.claude/homunculus/instincts/...` ECC files — they're superseded by these curated skills.
- **Codex** → `.codex/` : `config.toml` (MCP servers + multi-agent), `AGENTS.md`, role configs under `.codex/agents/`. Codex-facing skill mirror under `.agents/skills/`.

### MCP baseline (keep identical across both runtimes)
`github`, `context7`, `exa`, `memory`, `playwright`, `sequential-thinking`. This pack is the standard tool surface for envctl work; provision it through kasetto so Claude (`.mcp.json`/settings) and Codex (`config.toml` `[mcp_servers.*]`) stay in lockstep rather than drifting.

### Multi-agent roles (Codex)
Three read-only roles are the baseline: **explorer** (gather evidence before changes), **reviewer** (correctness/security/tests), **docs-researcher** (verify APIs against primary sources). Keep them read-only; mutation happens in the main thread.

## Why this matters
Agents act on whatever the environment tells them. If the config says "camelCase + *.test.ts", an agent will produce non-idiomatic Rust that fails `cargo fmt`/clippy and confuses review. A correct, curated environment is the precondition for finishing envctl — it's the difference between agents that help and agents that add noise.
