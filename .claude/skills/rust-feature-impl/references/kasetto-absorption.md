# Kasetto absorption — the no-downgrade playbook (Epic C)

Load this whenever a task's scope is the **agent-env / kasetto absorption** (backlog Epic C,
TASK-0011…0018). It is the absorption companion to `references/verification.md`: that file proves a
change is invariant-safe; this file proves the absorption **lost no kasetto feature**. The corpus is
`.handoff/decisions/ADR-0001-kasetto-handoff-portability-unification.md` (read it fully — the
no-downgrade checklist is authoritative there). Like `verification.md`, treat an *errored* command as
a FAIL, fail-closed.

The mission (ADR-0001 §Decision.3): absorb the external `kasetto` agent-environment provisioner into a
new **pure-Rust workspace crate `crates/agent-env`**, driven through the `Engine` API (engine-first,
non-printing, Events), surfaced by new CLI verbs `envctl agent {sync,add,remove,lock,list,clean}` with
GUI parity. Drop `mimalloc`. Adopt SHA-256 for the agent-asset lock, unified into `envctl.lock` as a
**separate keyed section** that leaves the existing FNV-1a component section untouched. Retire the
external `kasetto` binary dependency **only after the no-downgrade checklist below passes**.

## Table of contents
1. The single rule: no downgrade
2. Version-skew guard (source of truth = the v3.2.0 git tag)
3. The 11→6 verb mapping table
4. ALREADY-PORTED / DO-NOT-RE-PORT
5. Config schema (6 keys + `extends`)
6. Agent preset (21-preset enum)
7. MCP-merge: additive / never-clobber (hard invariant + regression fixture)
8. Command-format transforms + verbatim skill copy
9. Source resolver + security guards
10. SHA-256 lock-unification spec
11. Drop-mimalloc end-to-end
12. Target shape & backlog mapping
13. Verification deltas the guardian must see green

---

## 1. The single rule: no downgrade

Every absorbed verb, flag, schema key, transform, host, and guard that exists in the live kasetto
**v3.2.0** surface MUST exist in `crates/agent-env` after absorption, reachable through the Engine and
both front-ends. "Looks done" is not done — the checklist is item-by-item. A naive port keyed off the
stale docs (v3.0.0) silently drops the entire v3.1+ surface; §2 is the guard against exactly that.

**No-downgrade checklist (must ALL hold):**
- **All 11 verbs** present, incl. the v3.1 additions `add`, `remove`/`rm`, `lock`(`--check`,
  `--upgrade-package`). (Full list + mapping in §3.)
- **`--dry-run` / `--json` / `--locked` on every verb** that kasetto exposes them on (preview,
  machine-readable, no-fetch). `--dry-run` is the fail-closed default discipline; `--json` is the
  front-end-agnostic surface; `--locked` proves the lock satisfies without a network fetch.
- **Config schema:** the 6 keys (`destination`, `scope`, `agent`, `skills`, `mcps`, `commands`) **plus
  `extends`** — see §5.
- **Agent preset:** the **21-preset enum** plus a raw/escape form — see §6.
- **3 lock modes:** plain / `--update` / `--locked` — see §10.
- **Provisioning:** verbatim skill copy; the **5 command-format transforms**; the **MCP-merge**
  additive/never-clobber (§7–8).
- **Source resolver:** 6 host families (+GHE / subgroups / self-hosted), browser-URL→raw rewrite,
  `SOURCE@REF` pin, default-branch fallback, tar-slip path-traversal guard, env-only credentials,
  `extends` identity-keyed merge with cycle + depth guards (§9).

If any line cannot be checked green, the absorption is **not** complete and the external `kasetto`
binary dependency must NOT be retired.

## 2. Version-skew guard (source of truth = the v3.2.0 git tag)

The **source of truth is the `kasetto` v3.2.0 git tag — NOT `docs/KASETTO-FEATURES.md`**, which is
stale (written for v3.0.0). The installed binary is 3.1.0; tags reach v3.2.0. Porting from the docs
would silently drop everything added in v3.1+.

**v3.1+ surface a naive v3.0.0 port would drop** (the explicit "do not lose" list):
- `add` verb
- `remove` / `rm` verb
- `lock --check`
- `lock --upgrade-package`
- `config_edit.rs` (config-mutation source path behind `add`/`remove`)
- `source_edit.rs` (source-mutation source path behind `add`/`remove`)

**Procedure (do this BEFORE writing any port code):**
1. Resolve the absorption source against the **v3.2.0 tag** (`git -C <kasetto> describe --tags`;
   checkout/diff the tag, not `HEAD`, not the docs).
2. **Diff the installed binary 3.1.0 against v3.2.0** to enumerate what v3.2.0 adds over the installed
   surface, so the port targets v3.2.0, not the binary you happen to have on PATH.
3. Cross-check the resulting verb/flag list against §1 and §3. Treat `docs/KASETTO-FEATURES.md` as a
   *non-authoritative* hint only; if it disagrees with the tag, the tag wins.

## 3. The 11→6 verb mapping table

kasetto exposes 11 verbs; envctl collapses them onto 6 `envctl agent` verbs. The collapse must drop
**zero** behavior — the mapping below is the contract. Every kasetto verb lands on an envctl verb (or a
flag of one); nothing is orphaned.

| # | kasetto verb | envctl `agent` target | Notes |
|---|--------------|----------------------|-------|
| 1 | `sync` | `agent sync` | the core provision: fetch + transform + install skills/mcps/commands |
| 2 | `add` | `agent add` | v3.1; appends a skill/mcp/command source (drives `config_edit.rs`/`source_edit.rs`) |
| 3 | `remove` / `rm` | `agent remove` | v3.1; removes a source (alias `rm` preserved) |
| 4 | `lock` | `agent lock` | carries `--check` and `--upgrade-package` (v3.1) as flags |
| 5 | `lock --check` | `agent lock --check` | CI/verify mode (no mutation) |
| 6 | `lock --upgrade-package` | `agent lock --upgrade-package` | v3.1; bump a single pinned package |
| 7 | `list` | `agent list` | enumerate provisioned skills/mcps/commands/agents |
| 8 | `clean` | `agent clean` | remove provisioned assets (fail-closed; never prune on partial failure) |
| 9 | `init` | `agent add` (init path) / `agent sync` bootstrap | bootstrap a config; fold into add/sync rather than a standalone verb |
| 10 | `status` | `agent list` (status mode) / `agent lock --check` | drift/status surface folds into list + lock --check |
| 11 | `validate` | `agent lock --check` (validate mode) | schema/lock validation folds into lock --check |

> The six **canonical** envctl verbs are `sync, add, remove, lock, list, clean` (ADR-0001 §Decision.3).
> Verbs 9–11 are kasetto surfaces that **fold into** those six as modes/flags — they must remain
> reachable, just not as separate top-level verbs. If the architect proposes dropping any kasetto verb
> outright instead of folding it, that is a downgrade — reject it.

## 4. ALREADY-PORTED / DO-NOT-RE-PORT

These kasetto surfaces are **already in envctl** — the absorption MUST NOT re-implement, fork, or
regress them. Name them verbatim and route around them:

- **kasetto §2 lock → `crates/engine/src/lock.rs`** — the **FNV-1a** component hash, `LockDriftKind`
  (`Added` / `Removed` / `Changed`), and the `lock --check` CI gate. This is the **component** lock and
  it stays FNV-1a. Do **not** swap its hash, do **not** route agent-asset hashing through it.
- **kasetto §16 runtime → `crates/engine/src/runtime.rs`** — machine-local runtime state, deliberately
  **out of the committed lock**. Preserve the lock↔runtime separation (see §10).
- **Existing `doctor`** — already ported; agent-env does not re-implement diagnostics.
- **Existing `lock --check`** — already a CI gate; agent-env's lock work extends it, it does not
  replace it.

**Hard rule:** agent-env adds a **SEPARATE keyed SHA-256 section** to `envctl.lock` for agent assets,
leaving the **FNV-1a component section untouched**. Two hash families coexist in one lock file by
design (FNV-1a for components, SHA-256 for agent assets). Regressing the FNV-1a section, or rehashing
components under SHA-256, is a downgrade.

What is **NOT yet absorbed** (this is the actual work): the agent-env provisioning itself — skill/MCP/
command sync, the multi-host source resolver, the transform/merge installers, `extends`, and the
21-agent preset — all still delegated to the external `kasetto` binary via `manifest/agent-env.toml`.

## 5. Config schema (6 keys + `extends`)

Preserve all of:
- `destination` — where assets land.
- `scope` — provisioning scope (drives scope-relative lock destinations, §10).
- `agent` — the 21-preset enum **plus** a raw form (§6).
- `skills` — source / branch / ref / sub-dir + an untagged wildcard-or-list form.
- `mcps` — same source shape; feeds the MCP-merge (§7).
- `commands` — same source shape; feeds the 5 command-format transforms (§8).
- **`extends`** — identity-keyed merge of one config into another, with **cycle and depth guards**
  (§9). Dropping `extends` is a downgrade.

## 6. Agent preset (21-preset enum)

The `agent` key accepts a **21-value preset enum** (the named agent targets kasetto knows how to write
native paths for) **plus** a raw/escape value for unlisted targets. Port the full enum verbatim — a
subset is a downgrade. Each preset carries its per-agent native destination paths (where skills/mcps/
commands get written for that agent).

## 7. MCP-merge: additive / never-clobber (HARD INVARIANT + regression fixture)

**Hard invariant:** MCP provisioning is **additive and never-clobbering**. `agent sync` installs the
6 baseline MCP servers (`github`, `context7`, `exa`, `memory`, `playwright`, `sequential-thinking`)
**without removing or overwriting** any server already present in the target config. Critically, a
config that already contains the **global `broker` / `repowire` / `weave`** servers MUST still contain
them after `agent sync`, side-by-side with the 6 baseline servers.

**Named regression fixture (must be a test):** seed a target config containing `broker`, `repowire`,
and `weave`; run `agent sync`; assert the post-sync config contains **all 9** servers — the original 3
(`broker`/`repowire`/`weave`) **plus** the 6 baseline. A sync that drops, renames, or overwrites any
pre-existing server FAILS this fixture and is a downgrade. (This is the single most likely silent
regression in the absorption — the guardian asserts it explicitly, see §13.)

There are **4 MCP-merge formats** (per-agent config layouts) — all 4 must merge additively, never
clobber, per ADR-0001 §No-downgrade.

## 8. Command-format transforms + verbatim skill copy

Provisioning preserves:
- **Verbatim skill copy** — skills are copied byte-for-byte to the target (no transform).
- **5 command-format transforms** — commands are rewritten into each agent's native command format;
  all 5 transforms must be ported. Enumerate them from the v3.2.0 source (the `commands` installer) and
  port each; a missing transform means commands silently fail to install for that agent format.
- **Per-agent native paths** — each preset (§6) writes to its own destination layout.

## 9. Source resolver + security guards

Port the full multi-host resolver and **every** security guard (these are not optional hardening — they
are part of the no-downgrade surface):

- **Host families:** GitHub (+ GitHub Enterprise / GHE), GitLab (+ subgroups, + self-hosted),
  Bitbucket, Codeberg, Gitea, Forgejo.
- **Browser-URL → raw rewrite** — a pasted browser URL is rewritten to the raw/API fetch URL.
- **`SOURCE@REF` pin** — a source may pin an exact ref (`owner/repo@sha|tag|branch`).
- **Default-branch fallback** — when no ref is pinned, resolve the remote's default branch.
- **tar-slip path-traversal guard** — extracted archive entries are confined; `../` / absolute paths
  that escape the destination are refused (fail-closed). This is a security guard — never weaken it.
- **Env-only credentials** — credentials come from the environment only, never from config files or
  the lock. Never serialize a credential into `envctl.lock`.
- **`extends` identity-keyed merge** with **cycle guard** (refuse a config that extends itself
  transitively) and **depth guard** (bounded recursion). Both guards must be present.

**No banned C** in any of this: the resolver/extractor stay pure-Rust (rustls+ring, `miniz_oxide` for
zip/gzip, `sha2` for hashing) — kasetto already passes `ci/gates/no-c.sh` as-is. Do not introduce a C
tar/zip/zlib or a C TLS while porting.

## 10. SHA-256 lock-unification spec

Adopt kasetto's **SHA-256** content hashing for the agent-asset lock (the cryptographic choice — the
no-downgrade option vs. a weaker hash), unified into `envctl.lock` as a **separate keyed section**:

- **OS-invariant content hashing** — the hash is over content, normalized so it is identical across
  operating systems (no path-separator / line-ending / mode skew).
- **Scope-relative destinations** — lock entries record destinations relative to `scope`, not absolute
  machine paths (so the lock is portable).
- **3 modes:**
  - *plain* — verify + fetch as needed, write/refresh the lock.
  - *`--update`* — re-resolve refs and rewrite the lock to current.
  - *`--locked`* — verify the lock is satisfied **without any network fetch**; fail if it isn't.
- **Revision-mismatch refetch** — if a locked entry's recorded revision no longer matches, refetch
  (in non-`--locked` modes).
- **Never prune on partial failure** — if any source fails mid-sync, do **not** prune/remove the
  already-installed assets; leave the prior good state intact (fail-closed).
- **Coexists with FNV-1a** — the SHA-256 agent-asset section lives **alongside** the existing FNV-1a
  component section in `envctl.lock`; neither touches the other (§4).
- **Lock ↔ runtime separation** — committed lock is content/identity; machine-local runtime state stays
  in `runtime.rs`, out of the committed lock (§4).

## 11. Drop-mimalloc end-to-end

kasetto links `mimalloc` (via `libmimalloc-sys`) — a **C allocator** — plus release-profile tuning. On
absorb:
- **Drop** the `mimalloc` and `libmimalloc-sys` dependencies and kasetto's release-profile tuning from
  the absorbed crate. `crates/agent-env` uses the system/global allocator like the rest of the
  workspace.
- **Extend `ci/gates/no-c.sh`** to also grep/forbid `mimalloc|libmimalloc-sys` in the resolved graph
  (today it forbids the SQLite/OpenSSL/aws-lc family; add the allocator). This makes the drop
  fail-closed and permanent.
- **Verify:** `cargo tree -p envctl-agent-env` shows **no** `mimalloc` / `libmimalloc-sys` (and
  `bash ci/gates/no-c.sh` stays green with the extended grep).

## 12. Target shape & backlog mapping

The absorption follows the standard engine-first delivery (see the parent `rust-feature-impl` skill):

1. **New pure-Rust crate `crates/agent-env`** — added to the workspace `members`; pure-Rust deps only
   (rustls+ring, miniz_oxide, sha2); **no** mimalloc.
2. **Engine module + Events** — provisioning logic lives in the engine path, **non-printing**: it emits
   `Event`s for progress/results, returns typed data, never `println!`. (Reuse / extend the `Engine`
   API; the agent-env logic is engine code, not CLI/GUI code.)
3. **CLI `envctl agent {sync,add,remove,lock,list,clean}`** — clap parsing + rendering only; each verb
   calls the identical Engine method.
4. **GUI parity** — the same Engine methods reachable from `envctl-gui` (or the plan justifies an
   asymmetry).
5. **`manifest/agent-env.toml`** reframed from "drive external `kasetto` binary" to "built-in
   subsystem."

**Backlog mapping:** this maps to Epic C, **TASK-0011…0018**. TASK-0012 (the `crates/agent-env` crate)
gates TASK-0013…0018 (Engine module, CLI verbs, GUI parity, source resolver, lock unification, MCP
merge) — honor that dependency order. When an item spans envctl + `meta/kasetto` (e.g. syncing the meta
kasetto source UP first), it routes to the A2 cross-repo shape, intra-cycle **ORDERED** (source-up
first, guardian-gated, before envctl absorbs) — see `feature-forge` Scope-D notes.

## 13. Verification deltas the guardian must see green

On top of the standard `references/verification.md` gates, an agent-env change must additionally prove:
- **No-downgrade checklist** (§1) holds — every verb / flag / schema key / host / guard present.
- **`mimalloc` / `libmimalloc-sys` absent** — `cargo tree -p envctl-agent-env` clean; extended
  `ci/gates/no-c.sh` green (§11).
- **FNV-1a component lock section intact** while agent assets use the **SHA-256** section (§4, §10) —
  both coexist in `envctl.lock`.
- **MCP-merge preserved** the global `broker` / `repowire` / `weave` servers alongside the 6 baseline —
  the §7 regression fixture passes.
- **`--dry-run` / `--json` / `--locked`** present on every verb; **3 lock modes** behave per §10
  (`--locked` does no network fetch; never-prune-on-partial-failure holds).
- **No banned C** anywhere in the resolver/extractor/crypto path; engine purity + front-end parity per
  the standard recipe.
