# HANDOFF — kasetto agent environment + Phase-8 progress + consolidation

**Updated:** 2026-06-04. **Scope:** the agent-environment stabilization (via the `kasetto` tool), the
envctl secrets Phase-8 code progress, and the repo consolidation/cleanup. SEPARATE from the project's
own `HANDOFF.md` (the secrets-stack verification guide) — read both.

> **Repo state is now CLEAN.** `FlexNetOS/envctl` has a single branch `master` (canonical). The
> divergent ECC `master`, plus `env-ctl-2` and `merge/env-ctl`, were consolidated/deleted; PR #7
> (envctl) and PR #1 (vault_hub) are MERGED. `env-ctl` + `envctl-merge-envctl` are archived. Only the
> F2/F5/F6 design spike remains.

## TL;DR

| Item | State | Commit |
|------|-------|--------|
| Agent env (`.claude`/`.codex`) now kasetto-managed + locked; ECC config retired | ✅ done | `28c207e` |
| F15/F12 ported from env-ctl (remote bearer registry/mint + plane-bound row MAC) | ✅ done | `838d347` |
| F14 — `PresenceGate` egress-gate abstraction | ✅ done | `e82a21e` |
| Repo consolidation: `env-ctl-2` → `master`, stale branches deleted, PRs #7/#1 merged | ✅ done | `0376e5a` |
| Secrets design corpus carried into `docs/secrets/` (incl. `SERVER-MODE.md`) | ✅ done | `4b219a3` |
| `env-ctl` + `envctl-merge-envctl` archived to `~/Desktop/_archives/` | ✅ done | — |
| F2 / F5 / F6 — internet-facing relay edge | ⛔ NOT started — needs a design spike first | — |

All completed work is on branch **`master`** (the sole branch), builds clean, tests green
(116 secrets tests + 70 engine tests), clippy `-D warnings` clean, passes `ci/gates/{no-c,shape,enable}.sh`.

## Consolidation — DONE (2026-06-04)

The repo cleanup the operator asked for is COMPLETE (verified upgrades-only — no work lost):

- **envctl is one clean repo.** `env-ctl-2` was consolidated onto `master` via a history-preserving
  `-s ours` merge (`0376e5a`, a fast-forward — the old ECC `master` history is retained, not orphaned).
  A 5-agent adversarial safety workflow + an inline check both confirmed `master` held ZERO real work
  `env-ctl-2` lacked (only ECC-regenerated config + older pre-F12/F15 secrets ancestors). Stale
  branches `env-ctl-2` and `merge/env-ctl` deleted. PR #7 → MERGED.
- **vault_hub** PR #1 (the kasetto harness) → MERGED into `main`; branch deleted.
- **Audit + carry-over.** env-ctl is fully ⊆ envctl for code (no missing crates; F15/F12 verified). Its
  unique **secrets design docs** were carried into `envctl/docs/secrets/` (`4b219a3`) so envctl owns the
  design basis; the `workflows/*.js` build scripts were left in the archive only.
- **Archives** (full `.git`, `target/` excluded): `~/Desktop/_archives/env-ctl-2026-06-04.tar.gz`
  (338M), `~/Desktop/_archives/envctl-merge-envctl-2026-06-04.tar.gz` (394K).

**Operator TODO:** remove the `env-ctl` GitHub repo, and the local `~/Desktop/env-ctl` +
`~/Desktop/envctl-merge-envctl` directories (the local worktree still holds branch `merge/env-ctl`;
remove it with `git -C ~/Desktop/envctl worktree remove ~/Desktop/envctl-merge-envctl`). All content is
preserved in the archives + envctl history.

**Actual remaining work:** the **F2/F5/F6 design spike** (see "Phase-8 remaining" below) — the only
thing left.

## Repo roles (historical — now consolidated)

**`/home/drdave/Desktop/envctl` (branch `master`) is the single canonical repo.** Do all work there.

| Dir | Role (operator's words) | Status now |
|-----|-------------------------|------------|
| `~/Desktop/envctl` (`master`) | **the original project** | **Canonical** unified 8-crate workspace; the sole repo going forward. |
| `~/Desktop/env-ctl` | **the project to enhance features for envctl** | Consolidated in (code ported; secrets docs carried to `docs/secrets/`). **ARCHIVED** → `~/Desktop/_archives/env-ctl-2026-06-04.tar.gz`. Operator to remove from GitHub + locally. |
| `~/Desktop/envctl-merge-envctl` | **a merge worktree a prior Claude session created** | Provably contained in envctl (`77fd8fe`). **ARCHIVED** → `~/Desktop/_archives/envctl-merge-envctl-2026-06-04.tar.gz`. Remove the local worktree. |

Future features land **directly in envctl `master`**.

## The kasetto-managed agent environment

The `.claude/` + `.codex/` agent config is now provisioned and locked by **kasetto** (the env-manager
CLI, installed at `~/.local/bin/{kasetto,kst}`), from a committed source — NOT hand-edited.

- **Source of truth:** `envctl/kasetto.yaml` + `envctl/agent-skills/` (3 curated skills:
  `env-toolchain-install`, `agent-env-config`, `env-stabilize`; + a 6-server MCP pack:
  github/context7/exa/memory/playwright/sequential-thinking). Lock: `envctl/kasetto.lock` (committed).
- **The old ECC auto-generated config was RETIRED** — it asserted JavaScript conventions (camelCase
  files, `*.test.ts`) wrong for this Rust repo. The `agent-env-config` skill supersedes it with correct
  Rust conventions. Do not regenerate the ECC bundle over this.
- **Operate it:**
  - `kasetto sync` — provision/refresh (authoritative; a clean tree is a no-op).
  - `kasetto doctor` — health check. `kasetto list` — inventory.
  - `kasetto sync --locked` — CI enforcement (never fetches; fails on drift). Wire into CI.
  - Change the env by editing `agent-skills/` or `kasetto.yaml` then `kasetto sync` — never by
    hand-editing `.claude/.codex` live files.

## Phase-8 remaining (F2/F5/F6) — GATED ON PURPOSE, not forgotten

F2 (in-process TLS+DPoP/EKM HTTPS listener), F5 (streaming-revocation tear-down), F6 (DPoP `jti`
replay store) were **deliberately not implemented**. They are one coupled, **internet-facing** subsystem
(the whole A13–A16 threat model) and have hard blockers:

1. **Open spec (OI-SM-1 in `docs/secrets/SERVER-MODE.md`):** `jti` replay-store sizing/eviction,
   server-issued nonce lifecycle, and clock-drift window are *unspecified*. F6 can't be built right
   without these decisions.
2. **New deps vs. the `no-c` gate:** F2 needs a DPoP/RFC-9449 verifier, rustls `ServerConfig` + EKM,
   and CVE-pinned `tonic ≥0.12.3` + patched `hyper` (CVE-2024-47609). Each must clear `ci/gates/no-c.sh`.
3. **Structural invariants:** three disjoint CAs (edge / remote-clients / MITM) with forbidden-state
   FS-S25 if mixed; module isolation (CI grep); listener self-check; mandatory negative tests
   (FS-S16/S17). A subtly-wrong DPoP/EKM binding is a silent auth bypass.
4. Even **env-ctl** hasn't built these — they're "pending" upstream too.

**Recommended next step:** a written **design spike** (pin the dep set + prove `no-c`; resolve OI-SM-1
params; specify module-isolation + self-check + negative-test plan; sequence **F6 → F2 → F5**) for
review, THEN implement against the approved design. VPS / Profile-B remains blocked behind OI-SM-2/3
(operator-authorizer protocol + trusted time) — Profile A is the only production path.

**What already exists for Phase 8 (the foundation):** F3/F4 (`RemotePeer`/`CrossKindPresentation`
plane binding in `decide()`), F15/F12 (`relay_mint_remote` + `remote_clients` registry + plane-bound
row MAC), and F14 (`broker::gate::{GateState, PresenceGate, gate_absent_since_ms}` — the egress-gate
choke point the Profile-B operator-box gate will plug into). The listener is the missing serving layer.

## Spec / reference pointers
- Phase-8 spec: `docs/secrets/SERVER-MODE.md` (also covers FS-S16..S25, OI-SM-1..6).
- Engine gate (F14): `crates/secrets-engine/src/broker/gate.rs`.
- Cross-effort plan: `~/.claude/plans/elegant-growing-turtle.md` (has a Progress section).
- kasetto adoptions catalog: `docs/KASETTO-FEATURES.md` (other lower-risk envctl enhancements:
  universal `--json`, `--locked`/`--update`, multi-host source resolver).
