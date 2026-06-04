# HANDOFF — kasetto agent environment + Phase-8 progress

**Paused:** 2026-06-04. **Scope of this doc:** the agent-environment stabilization (done via the
`kasetto` tool) and the envctl secrets Phase-8 code progress made in the same session. This is
SEPARATE from the project's own `HANDOFF.md` (the secrets-stack verification guide) — read both.

## TL;DR

| Item | State | Commit |
|------|-------|--------|
| Agent env (`.claude`/`.codex`) now kasetto-managed + locked; ECC config retired | ✅ done | `28c207e` |
| F15/F12 ported from env-ctl (remote bearer registry/mint + plane-bound row MAC) | ✅ done | `838d347` |
| F14 — `PresenceGate` egress-gate abstraction | ✅ done | `e82a21e` |
| F2 / F5 / F6 — internet-facing relay edge | ⛔ NOT started — needs a design spike first | — |
| Consolidate the duplicate worktree (Phase E) | ⛔ pending | — |

All three completed commits are on branch **`env-ctl-2`**, build clean, tests green
(116 secrets tests + 70 engine tests), clippy `-D warnings` clean, and pass `ci/gates/{no-c,shape,enable}.sh`.

## Which copy is canonical (READ THIS — avoids the #1 confusion)

There are three directories on the Desktop, each with a distinct ROLE (per the operator). **`/home/drdave/Desktop/envctl`
(branch `env-ctl-2`) is the single canonical workspace.** Do all work there.

| Dir | Role (operator's words) | What it is now | Use |
|-----|-------------------------|----------------|-----|
| `~/Desktop/envctl` (`env-ctl-2`) | **the original project** | **Canonical** unified 8-crate workspace; current head. | **Work here.** |
| `~/Desktop/env-ctl` (separate repo, `main`) | **the project to enhance features for envctl** | The feature-development source (F15/F12 etc. were authored here, then ported into envctl). Currently `2f7f8e9`; envctl is now AHEAD of it (F14 lives only in envctl). | Active feature source — but envctl is canonical. See open question below. |
| `~/Desktop/envctl-merge-envctl` (`merge/env-ctl`) | **the merge repo a prior Claude session created to merge the two together** | A **git worktree of envctl's `.git`** (branch `merge/env-ctl`), now **3 commits STALE** (stuck at `77fd8fe`). The merge it was made for has effectively landed in envctl. | Spent — ignore / retire (Phase E). Do NOT edit. |

**Open question for the operator (don't assume):** now that the merge has landed and envctl is ahead, should future feature work go (a) directly into envctl (canonical), or (b) into env-ctl first then port/merge into envctl? F15/F12 were authored in env-ctl and ported; F14 was authored directly in envctl. Confirm the intended workflow before the next feature lands.

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

1. **Open spec (OI-SM-1 in `env-ctl/docs/SERVER-MODE.md`):** `jti` replay-store sizing/eviction,
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
- Phase-8 spec: `env-ctl/docs/SERVER-MODE.md` (also covers FS-S16..S25, OI-SM-1..6).
- Engine gate (F14): `crates/secrets-engine/src/broker/gate.rs`.
- Cross-effort plan: `~/.claude/plans/elegant-growing-turtle.md` (has a Progress section).
- kasetto adoptions catalog: `docs/KASETTO-FEATURES.md` (other lower-risk envctl enhancements:
  universal `--json`, `--locked`/`--update`, multi-host source resolver).
