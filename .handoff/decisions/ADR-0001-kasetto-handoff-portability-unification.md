# ADR-0001 — Unify kasetto into envctl, complete handoff sync, and finish meta-portability

- **Status:** accepted (planning) — 2026-06-12
- **Plane:** env-control
- **Scope:** envctl (engine/cli/gui + new agent-env crate), envctl/.handoff, envctl/home overlay; coordinates with `handoff/` and `kasetto/` peer repos.
- **Supersedes/relates:** handoff ADR-0003 (kb↔handoff seam), ADR-0004 (fleet rollout / ledger residency), ADR-0006 (meta-portability), `docs/KASETTO-FEATURES.md` (stale, v3.0.0).

## Context

The owner directive (2026-06-12): treat envctl as an **agenticOS**. Three concrete missions, all
upgrade-only / no-downgrade / no-feature-lost:

1. Bring `envctl/.handoff` into **full sync** with the canonical handoff kernel (`meta/handoff`).
2. Verify and harden **meta policy conformance** (CLI / GitHub / MCP / plugins) and finish
   **meta-confinement + portability** (everything envctl consumes resolves inside `meta`).
3. **Integrate / unify kasetto** (the agent-environment provisioner) into envctl as a first-class,
   pure-Rust engine capability — no kasetto feature may be lost.

## Research (deep codebase + cross-reference; PROCESS RULE compliance)

### Stream 1 — handoff sync (canonical kernel vs envctl)
- Canonical kernel `meta/handoff`: witnessed `ledger.db` (blake3, rvf-crypto), `schemas/{packet.v2,task.v1,session.v1}`, the `hf` binary (`hf/src/main.rs`, verbs init/seed/status/claim/release/checkpoint/handoff/resume/ship/review/policy/session/sync/fleet/drift), `.handoff/{policy.toml,hooks/hooks.toml,policies/rules.toml,active.md,packets/latest.md,skills/}`, p7-conformance CI, handoff-discipline skill + steward agent.
- **Ledger residency (ADR-0004):** one ledger per orchestration home = `meta/.handoff/ledger.db` (fleet) / `meta/handoff/.handoff/ledger.db` (kernel). **Per-repo `.handoff/` carries git-committed TEXT ONLY — no `ledger.db`.** envctl writes events to the shared ledger via `hf checkpoint`/`hf handoff`.
- **envctl/.handoff today ≈ 30% (Tier B stub):** has `context/capsule.json` + `README.md` + `loop/` (env-ownership backlog). **Missing:** `hf` on PATH (not built — no `meta/handoff/target/release/hf`), `policy.toml`, `hooks/hooks.toml`, `policies/rules.toml`, `active.md`, `packets/latest.md` (must be **rendered by hf, never hand-written**), task cards (`handoff.task.v1`, id `^TASK-[0-9]{4,}$`), ADRs, skills.

### Stream 2 — meta policy + install verification
- `meta` CLI 0.2.22 (OK); `gh` authed as `drdave-flexnetos`, org `FlexNetOS`, envctl remote matches `.meta.yaml` (OK).
- MCP baseline parity OK: `.mcp.json` ≡ `.codex/config.toml` = {github, context7, exa, memory, playwright, sequential-thinking}. **Flag:** `exa` declared but not connected this session.
- Plugins OK: 4 meta subprocess plugins + 5 Claude marketplace plugins; `meta@gitkb` MCP live as `plugin_meta_meta`.
- **Gap:** `hf` (handoff kernel binary) not built/on PATH — this is the keystone blocker for Stream 1.

### Stream 3 — portability / meta-confinement (ADR-0006)
- Symlink source-of-truth **VERIFIED**: `envctl/home/.claude/settings.json` is the real file; `~/.claude/settings.json` and `meta/settings.json` symlink into it. Wiring via `manifest/components.d/portability-links.toml` (archive-first, idempotent, version-guarded).
- **~80% portable.** Keystone gap: **`META_ROOT` is aspirational, not implemented** (referenced only in `.handoff/` docs). `engine::dashboard::locate_meta_file()` already walks to the `.meta.yaml` marker — reuse it.
- **3 HIGH violations** in the live source-of-truth `home/.claude/settings.json`: hardcoded `statusLine.command` path + 2 hardcoded `extraKnownMarketplaces.*.path` (all `/home/drdave/Desktop/meta/...`).
- Global `home/.config/kasetto/kasetto.yaml` sources `mcps` from `https://github.com/FlexNetOS/agent-skills` — should resolve **in-meta**. MED shell/nushell hardcodes. Phase-2 tool relocation partial (only `meta` symlinked; `rtk`/`kasetto`/`meta-mcp` still real files).

### Stream 4 — kasetto feature surface (for no-downgrade integration)
- **kasetto is already pure-Rust** and passes envctl's `ci/gates/no-c.sh` as-is (rustls+ring, miniz_oxide, sha2; zero banned C). **Only cleanup on absorb: drop `mimalloc`/`libmimalloc-sys`** (a linked C allocator) and kasetto's release-profile tuning.
- **Version skew:** `docs/KASETTO-FEATURES.md` written for v3.0.0; installed binary 3.1.0; tags reach v3.2.0. v3.1+ added **`add`, `remove`/`rm`, `lock`(`--check`,`--upgrade-package`)** + `config_edit.rs`/`source_edit.rs` — a naive v3.0.0 port would silently drop them.
- **envctl has ALREADY ported** kasetto §2 lock (`crates/engine/src/lock.rs`, FNV-1a), §16 runtime (`crates/engine/src/runtime.rs`), `doctor`, and `lock --check`. **Not yet absorbed:** the agent-env provisioning itself (skill/MCP/command sync, multi-host source resolver, the transform/merge installers, `extends`, the 21-agent preset) — still delegated to the external `kasetto` binary via `manifest/agent-env.toml`.

## Decision

1. **Handoff:** build & install `hf`, wire envctl to the shared `meta/.handoff/ledger.db`, and let
   **`hf` render** envctl's `policy.toml`/`hooks`/`policies`/`active.md`/`packets`/`skills`. Add a
   p7-conformance CI gate. Do **not** create a per-repo `ledger.db`; do **not** hand-write packets.
2. **Portability:** implement `envctl env` → export `META_ROOT` (reuse `locate_meta_file`), then heal
   the 3 HIGH `settings.json` refs and the global kasetto.yaml source via `$META_ROOT`/templating;
   finish Phase-2 relocation never-downgrade (sync in-meta source up to installed version first).
3. **Kasetto unification (no downgrade):** absorb kasetto into a new pure-Rust workspace crate
   `crates/agent-env`, driven through the `Engine` API (engine-first, non-printing, Events), surfaced
   by **new CLI verbs `envctl agent {sync,add,remove,lock,list,clean}`** with GUI parity. Drop
   `mimalloc`. Adopt **SHA-256** for the agent-asset lock (kasetto's, cryptographic — the
   no-downgrade choice) and unify into `envctl.lock` (separate keyed section) while keeping the
   FNV-1a component section. Reframe `manifest/agent-env.toml` from "drive external binary" to
   "built-in subsystem." Retire the external `kasetto` binary dependency only after the
   **no-downgrade checklist** passes.

### No-downgrade preservation checklist (every kasetto feature that MUST survive)
- All 11 verbs incl. v3.1 `add`/`remove`/`lock`(`--check`,`--upgrade-package`); `--dry-run`/`--json`/`--locked` everywhere.
- Config schema: `destination`, `scope`, `agent` (21-preset enum + raw), `skills`/`mcps`/`commands` (source/branch/ref/sub-dir + untagged wildcard-or-list), **`extends`** (identity-keyed merge, cycle/depth guard).
- Lock: OS-invariant content hashing (SHA-256), scope-relative destinations, 3 modes (plain/`--update`/`--locked`), revision-mismatch refetch, "never prune on partial failure", lock↔runtime separation.
- Provisioning: verbatim skill copy; 5 command-format transforms; 4 MCP-merge formats **additive, never-clobber** (must preserve global `broker`/`repowire`/`weave` servers); per-agent native paths.
- Source resolver: GitHub(+GHE)/GitLab(+subgroups/self-hosted)/Bitbucket/Codeberg/Gitea/Forgejo; browser-URL→raw; `SOURCE@REF`; default-branch fallback; **tar-slip guard**; env-only creds.

## Consequences
- envctl becomes the single owner of the agent environment (provisioning + lock + continuity), no
  external `kasetto` runtime dependency, no-C trust boundary intact, full meta-portability.
- The 15 execution units below are seeded as `handoff.task.v1` cards (planning artifacts). They will
  be reconciled with the kb board / witnessed ledger once `hf` is built (TASK-0001) and may be
  re-minted via `hf task mint --from-kb`. Packets remain hf-rendered.
