---
name: env-stabilize
description: "How to keep the environment reproducible and drift-free — the detect/drift, doctor-diagnostic, and content-hashed lock discipline envctl uses, plus how kasetto provisions and locks the agent config. Use when checking environment health, diagnosing drift, regenerating or verifying a lock, or making the agent environment reproducible. Triggers: 'is the environment stable', 'check for drift', 'run doctor', 'the lock is stale', 'make this reproducible', 'env is inconsistent', 'sync the agents'."
---

# Environment Stabilization (drift · doctor · lock)

A stable environment is one that is **fully declared and reproducible**: every tool and every agent-config file is described by a manifest/source and pinned by a content hash, so drift is detectable and repair is mechanical. envctl already practices this (its `doctor` verb and committed `envctl.lock` were adopted from kasetto); apply the same discipline to the agent environment, with kasetto as the tool.

## The Three Disciplines

### 1. Drift detection
Drift = the live state diverges from the declared state. envctl's engine compares each component's `detect`/`verify` result against the manifest. For the agent environment, drift = the installed `.claude`/`.codex` skills/MCPs differ from the declared source. Detect it before trusting the environment; never "fix" by editing the live files (that just hides drift) — fix the source and re-sync.

### 2. Doctor diagnostics
A `doctor` pass is a read-only health report: what's present, what's broken (detects-present but fails verify), what's missing, what's drifted. Run it before and after any environment change. envctl exposes `envctl doctor`; kasetto exposes `kasetto doctor`. Treat a non-green doctor as a blocker, not a warning.

### 3. Content-hashed lock
The lock is the **manifest-of-record**: a content hash over the canonical declared set, committed to the repo, enforced as a CI gate. envctl commits `envctl.lock` (44 components, hashed) and gates on `lock --check`. kasetto commits `kasetto.lock` (skills + MCP/command assets, destinations stored relative to the scope root, hashes OS-normalized). **The lock is authoritative**: a plain sync installs exactly what the lock pins and does zero network fetch when on-disk state already matches. Only an explicit `--update` re-resolves moving sources.

## kasetto as the Stabilizer

kasetto is the toolkit that *provisions and locks* the agent environment from a single curated source:

- **Source of truth:** a curated skill set (`agent-skills/`) + an MCP pack, declared in `kasetto.yaml` (project scope), targeting `claude-code` + `codex`.
- **`kasetto sync`** writes the skills into each agent's dir, merges the MCP servers into each agent's native settings format (Claude JSON, Codex TOML), and records everything in `kasetto.lock`.
- **Reproducibility test:** a second `kasetto sync` is a **no-op** (lock authoritative). If it isn't, something drifted — investigate before proceeding.
- **CI gate:** `kasetto sync --locked` never fetches and fails if the lock can't satisfy the config — wire it into CI so the committed environment is enforced.

## kasetto ⟷ envctl: two layers — leverage it right

kasetto is **not** a rival to envctl; it is the **agent-layer twin** of envctl — same declarative, idempotent, content-locked, drift-detecting discipline, applied one layer up. Leverage them as composing layers, not duplicates:

| | **envctl** | **kasetto** |
|---|---|---|
| Manages | the **workstation / OS** (toolchains, GPU, PATH, env, systemd) | the **agent's toolkit** (skills · MCPs · commands) |
| Targets | apt · nix · `/usr/local` · `~/.bashrc` · systemd | `.claude/` + `.codex/` (project or global `~/.claude`) |
| Lock / verbs | `envctl.lock`; `doctor`/`install`/`auto-fix`/`reset`/`lock --check` | `kasetto.lock`; `sync`/`--locked`/`list`/`doctor`/`clean`/`--update` |

**The rules that make it pay off:**
1. **One source of truth — never hand-edit what kasetto owns.** Change `agent-skills/` + `kasetto.yaml`, then `kasetto sync`; never touch generated `.claude/skills/*`, `.mcp.json`, `.codex/config.toml`. Enforce with `kasetto sync --locked`.
2. **Multi-agent parity for free.** One config keeps `claude-code` and `codex` byte-identical — add an agent under `agent:`, don't maintain two configs by hand.
3. **Scope split.** `scope: project` for repo-specific toolkits (envctl conventions); a **global** `~/.config/kasetto/kasetto.yaml` for the personal baseline (general MCPs) that follows you everywhere.
4. **Shareable sources.** kasetto can source skills from a **git repo / URL with moving-ref → locked-hash pinning** (`--update` re-resolves + relocks). Publish a canonical `agent-skills` upstream and point projects at it instead of copy-pasting; envctl itself may keep a local `./agent-skills` mirror for fetch-free operation.
5. **Know what stays OUT of kasetto.** The Feature Forge harness (`feature-forge`, `forge-loop`, `env-install-loop`, `auto-provision`, `session-relay`, `rust-feature-impl`) is hand-authored and git-tracked — bespoke, fast-iterating orchestration. kasetto is for the **stable, shared, locked baseline** only.
6. **One `doctor` over both layers.** A `kasetto` component in the envctl manifest (`manifest/agent-env.toml`) makes "agent env provisioned + locked" just another component of a green box: detect = binary present, verify = `kasetto sync --locked` clean, install/fix = `kasetto sync`.

## Stabilization Workflow

1. `kasetto doctor` (and `envctl doctor`) → baseline health.
2. Reconcile: ensure `.claude`/`.codex` come from the curated source, not stale ECC auto-generation. Retire superseded ECC files (the wrong-convention `instincts` + auto SKILL.md) so there is one source of truth.
3. `kasetto sync` → provision + lock. Commit `kasetto.yaml` + `kasetto.lock`.
4. Re-run `kasetto sync` → confirm no-op (proves reproducibility).
5. `kasetto list` → confirm the curated skills + MCP pack are present across both agents.
6. Record the change; from now on, environment changes go through the source + `kasetto sync`, never by hand-editing live agent files.

## Why
Without lock + doctor + drift discipline, the environment silently rots: someone edits a live `.claude` file, a regen overwrites it, conventions drift, and agents start acting on stale or wrong config. Locking makes the environment a committed, verifiable artifact — the foundation that lets the team trust the agents enough to hand them envctl's remaining work.
