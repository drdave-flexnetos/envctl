# TASK-0012 — `crates/agent-env` standalone library (kasetto absorption P0, Epic C)
# Architect plan (feature-architect) — 2026-06-13

VERDICT: NEEDS-DECISION → **RESOLVED 2026-06-13 (owner authorized): GO.**

> **DECISION (owner, 2026-06-13):** "Reconcile kasetto first … we want the kasetto work not
> anything we did. accept all kasetto changes. sync with github.com/pivoshenko/kasetto | then start
> the rust port." **DONE:** `meta/kasetto` `main` aligned to `upstream/main` = pivoshenko **v3.2.0**
> (`ec01cca release: v3.2.0`, Cargo `3.2.0`); the 305 FlexNetOS-divergent commits archived on branch
> `flexnetos-divergence-backup-2026-06-13` (archive-first, recoverable). **The implementer ports
> `crates/agent-env` from the LIVE `meta/kasetto` v3.2.0 source** (`/home/drdave/Desktop/meta/kasetto/src/`)
> — no longer from a tag. All "v3.2.0-only" surface in this plan is now present in the working tree.

**Executive summary:** TASK-0012 is buildable in one cycle as a standalone pure-Rust crate
`crates/agent-env` (config model + extends + multi-host resolver + SHA-256 + lock), and the no-c
gate already pre-arms for it (Gate 3.5 expects `envctl-agent-env`, forbids mimalloc). **But the
source-of-truth the crate must model — the 6-key+extends schema with `add`/`remove`/`lock`
mutation logic (`config_edit.rs`/`source_edit.rs`) — exists ONLY at the `meta/kasetto` v3.2.0 git
tag, NOT on the checked-out HEAD (`v3.0.0-23-gf2a50b7`, Cargo `3.0.0`).** Per ADR-0001's
NEVER-DOWNGRADE gate, syncing `meta/kasetto` source UP 3.0.0→3.2.0 is an ordered prerequisite that
touches a peer repo and needs owner authorization — hence NEEDS-DECISION. The envctl crate design
below is otherwise GO and unblocked once the v3.2.0 source is the reference.

## Target repos
| Repo | Role in TASK-0012 | Module count | Source-up prerequisite? |
|------|-------------------|--------------|------------------------|
| `meta/kasetto` | **Source-of-truth sync UP 3.0.0→3.2.0** (provide the v3.1+ surface the crate ports) | n/a (sync) | **YES — ordered FIRST, owner-authorized** |
| `envctl` | New crate `crates/agent-env` (the deliverable) | ~6 new modules (config, extend, source, hash, lock, lib) | builds on v3.2.0 reference |

Build-shape: because a `meta/kasetto` source-up precedes the envctl crate, the cycle is **A2
cross-repo, intra-cycle ORDERED** (kasetto source-up first, guardian-gated, *then* envctl absorbs).

## No-downgrade resolution (evidence-based)
The v3.1+ surface lives ONLY at the v3.2.0 tag; HEAD does NOT contain it. Source-up required first.
Evidence (`git -C /home/drdave/Desktop/meta/kasetto`):
- `git describe --tags` → `v3.0.0-23-gf2a50b7`; `Cargo.toml` = `3.0.0`; installed binary = `3.1.0`; a **`v3.2.0` tag exists**.
- `find src -name config_edit.rs -o -name source_edit.rs` on HEAD → **empty**.
- `git diff --stat HEAD v3.2.0 -- src/`: v3.1+ surface ADDED by v3.2.0 — `commands/add.rs`(+314),
  `commands/remove.rs`(+228), `commands/lock.rs`(+315), `commands/source_edit.rs`(+114),
  `fsops/config_edit.rs`(+811), `cli.rs`(+151), `source/parse.rs`(+138 `derive_browse_url`).
  `git show v3.2.0:Cargo.toml` → `version = "3.2.0"`.
- The 6-key+extends schema is present in BOTH (`model/config.rs`, `model/extend.rs` on HEAD), **but**
  the `add`/`remove`/`lock --check`/`--upgrade-package` mutation logic + hardened `parse.rs` resolver
  are v3.2.0-only and ARE in TASK-0012's resolver+lock scope.

> **OWNER DECISION REQUIRED:** authorize (a) syncing `meta/kasetto` source 3.0.0→3.2.0 (working tree
> → `v3.2.0` tag / bump `Cargo.toml`), and (b) the A2 ordered cross-repo cycle. If declined,
> TASK-0012 can proceed against v3.0.0 but lands a *downgraded* resolver+lock (no
> `--check`/`--upgrade-package`, older `parse.rs`) — violates the no-downgrade checklist.

## Crate design
Layout (`crates/agent-env/src/*.rs`, snake_case, `#[forbid(unsafe_code)]`, non-printing):

| Module | Ports kasetto v3.2.0 | Public surface (TASK-0013 calls) |
|--------|----------------------|----------------------------------|
| `lib.rs` | crate root, error, re-exports | `pub use config::*; source::*; lock::*;` + `Result<T>`, `AgentEnvError` (thiserror) |
| `config.rs` | `model/config.rs` | `Config`, `Scope`, `SourceSpec`, `McpSourceSpec`, `CommandSourceSpec`, `*Field`/`*Entry` untagged enums, `GitPin`, `git_pin()`, `expected_revision()` |
| `extend.rs` | `model/extend.rs` + `fsops/config.rs` | `extract_extends()`, `merge_yaml()`, `load_config_recursive()` w/ `MAX_EXTENDS_DEPTH=8` cycle+depth guard |
| `source.rs` | `source/{hosts,parse,remote,auth}.rs` | `RepoUrl`, `parse_repo_url()`, `derive_browse_url()`/`BrowseDerived`, `rewrite_browse_to_raw_url()`, `archive_url(parsed, GitPin)`, `download_extract()` (tar-slip guard), `UrlRequestAuth` (env-only) |
| `hash.rs` | `fsops/hash.rs` | `hash_dir()`, `hash_file()`, `hash_str()` — SHA-256, `\`→`/` normalized (OS-invariant) |
| `lock.rs` | `lock.rs` + `commands/lock.rs` | `AgentLockFile`, `AgentLockEntry`/`AssetEntry`, `LockMode {Plain, Update(Vec<String>), Locked}`, `load`/`save`, `lock_check()`, keyed SHA-256 section distinct from engine FNV-1a |

**6 config keys + `extends`:** `destination`, `scope` (Global|Project), `agent` (21-preset enum +
raw One|Many — shape only here; path table deferred to TASK-0013), `skills: Vec<SourceSpec>`,
`mcps: Vec<McpSourceSpec>`, `commands: Vec<CommandSourceSpec>`; plus **`extends`** (string|list,
stripped pre-deserialize at `serde_yaml::Value`, recursively loaded+merged by identity
`(source, ref|branch, sub-dir)`; cycle guard + depth guard 8).

**Resolver:** host families GitHub(+GHE)/GitLab(+subgroups,self-hosted)/Bitbucket/Codeberg-Gitea-
Forgejo; host tarball URL builders; `ref>branch>default(main→master)` precedence; **tar-slip guard**
(reject `Component::ParentDir`, fail-closed `"unsafe archive path"`); **env-only credentials**
(`GITHUB_TOKEN`/`GH_TOKEN`, `GITLAB_TOKEN`/`CI_JOB_TOKEN`, `BITBUCKET_EMAIL`+`BITBUCKET_TOKEN`,
`GITEA`/`CODEBERG`/`FORGEJO_TOKEN` — never from config/lock).

**Hashing:** port `fsops/hash.rs` verbatim — `hash_dir` sorts, feeds `relpath\0<contents>\0` with
`\`→`/` normalization; `sha2` workspace dep.

**Lock + 3 modes:** `AgentLockFile { version, skills: BTreeMap, assets: BTreeMap }` (deterministic);
`AssetEntry { kind, name, hash, source, destination(scope-relative), source_revision }`. Modes:
**plain** (verify+fetch, refresh lock), **`--update`** (`Update(Vec<String>)` selective re-resolve),
**`--locked`** (verify w/ ZERO network, fail-closed). Plus **`lock --check`** (CI drift, no mutation)
and **`--upgrade-package <name>`** (v3.2.0-only). Serialize under own top-level key (e.g.
`[agent_assets]`) for clean later embedding into `envctl.lock` (TASK-0017). **Do NOT touch
`crates/engine/src/lock.rs` (FNV-1a).**

**11→6 verb mapping (modeled, surfaced TASK-0013):** `sync`→`agent sync`; `add`→`agent add`
(needs config_edit/source_edit, v3.2.0); `remove`/`rm`→`agent remove`; `lock`(+`--check`,
`--upgrade-package`)→`agent lock`; `list`→`agent list`; `clean`→`agent clean`; `init`/`status`/
`validate` fold into add/sync/list/lock-check.

## Cargo.toml
`crates/agent-env/Cargo.toml` package `envctl-agent-env`; deps: serde, serde_json, serde_yaml,
toml, sha2, thiserror (all workspace), `flate2 = { version="1", default-features=false,
features=["rust_backend"] }` (pure-Rust miniz_oxide — NEVER zlib/zlib-ng C), `tar = "0.4"`,
`reqwest` (workspace pin: default-features=false, rustls-tls→ring). **NO mimalloc**,
`unsafe_code="forbid"`. Workspace: add `"crates/agent-env"` to members; add flate2/tar to
`[workspace.dependencies]`.

## Tests (inline `#[cfg(test)] mod tests`)
config schema parse (wildcard/list/{name,path}, sub-dir alias, ref>branch pin); extend
extract/merge + **cycle guard + depth>8 guard**; resolver parse (GH/GHE/GitLab-subgroup/Bitbucket/
Codeberg), derive_browse_url, archive-URL builders, **tar-slip guard** (`..`→Err); hash stable +
**separator-invariant**; lock round-trip, legacy restamp, `lock_check` drift, **`--locked` zero
network**, scope-relative destination; no-downgrade meta-test (all 6 keys + extends round-trip).

## Invariant checklist
- No C: serde*/toml/sha2/thiserror/flate2(rust_backend)/tar/reqwest(rustls+ring); no mimalloc/SQLite/
  OpenSSL/aws-lc. `cargo tree -p envctl-agent-env` clean.
- no-c.sh: Gate 3.5 pre-arms for `envctl-agent-env`, forbids mimalloc → green once named + mimalloc-free.
- One rustls ring-only: reqwest workspace pin; no 2nd TLS, no aws-lc.
- Standalone non-printing: library only; `Result<T, AgentEnvError>`; no println/clap/Engine.
- Fail-closed: tar-slip refuses `..`; `--locked` refuses unsatisfied w/ zero fetch; lock defaults non-mutating.
- Conventions: snake_case, PascalCase, SCREAMING consts (MAX_EXTENDS_DEPTH, LOCK_VERSION), inline tests, MSRV 1.80, forbid unsafe.

## Risks / open questions
1. **OWNER DECISION (the NEEDS-DECISION):** authorize meta/kasetto source-up 3.0.0→3.2.0 + A2 cycle.
2. **21-preset agent enum scope:** model the enum shape (One|Many + raw) here; defer per-agent native
   path-mapping methods to TASK-0013. Confirm split acceptable.
3. **flate2 backend:** pin `default-features=false, features=["rust_backend"]` (miniz_oxide) to avoid C zlib.
4. **Lock embedding seam:** ship standalone `AgentLockFile` now under `[agent_assets]`; unify into
   `envctl.lock` at TASK-0017; do NOT touch engine FNV-1a lock here.

## Cross-reference (relayed, NOT a TASK-0012 blocker)
TASK-0020 (github-app mint surface) is a separate workstream carded on develop; touches the envctl
injection seam but is independent of TASK-0012.
