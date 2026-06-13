# FINDING-0002 — TASK-0002 blocked: installed `hf` is the S1 spike, missing the fleet verbs

- **Status:** **RESOLVED 2026-06-13 (Option A) — UNBLOCKS TASK-0002/0003.** The kernel built the
  missing fleet verbs in `meta/handoff` PR **#17** (`feat: fleet verbs hf fleet status/render, hf
  sync` + handoff-loop harness; handoff HEAD `1adbb13`); installed `hf` rebuilt 2026-06-13 04:29.
  Verified live from `$META_ROOT` on resume (owner "check now"): `hf fleet status` (fleet ledger
  present, 64 members enumerated), `hf fleet render envctl` (wrote `packets/latest.md`),
  `hf sync --dry-run` (one-way `.kb` mirror). The S1-spike gap is closed; TASK-0002 is executable as
  written. (Was: NEEDS-DECISION (owner / kernel team) — blocked TASK-0002, Epic A.)
> **RESOLVED 2026-06-13.** The blocker is cleared: a concurrent `meta/handoff` session BUILT the
> missing verbs — `hf fleet status`/`fleet render` + `hf sync` (PR #17), and `hf drift` + `hf policy`
> + `FLEET_GUIDE.md` (commit `000e4c0`). The installed `~/.local/bin/hf` was rebuilt and verified:
> `hf fleet render envctl` compiles a per-repo packet from the FLEET ledger with NO per-repo
> `ledger.db` (residency-safe), `hf sync --dry-run` mirrors to `.kb`, `hf drift`/`policy` run.
> **TASK-0002/0003 are now UNBLOCKED** — the design analysis below still stands; only the
> "blocked-on-unbuilt-verbs" status is cleared. Owner decided (2026-06-13) to execute TASK-0002 in
> the NEXT session (let the concurrent kernel session settle first).

- **Status:** RESOLVED 2026-06-13 (was NEEDS-DECISION) — backlog **TASK-0002** unblocked.
- **Date:** 2026-06-13 · **Surfaced by:** forge-loop agenticOS-consolidation cycle 2.
- **REVISED 2026-06-13** after reading the authoritative design corpus (`~/Downloads/tmp/handoff` =
  Ark Handoff Ledger PRD v2 + schemas/templates) and `meta/handoff/docs/adr-0004-fleet-handoff-rollout.md`
  + envctl ADR-0001 §22/§23. **The v1 framing ("shipped `hf` needs a `--ledger`/`HANDOFF_DIR` flag")
  was WRONG and is retracted** — see "Correction" below.
- **Cross-refs:** ADR-0004 (`meta/handoff/docs/adr-0004-fleet-handoff-rollout.md`, accepted
  2026-06-12), ADR-0001 §22/§23 (envctl), PRD v2 §2/§4.1–4.4 + §4 layout (`~/Downloads/tmp/handoff/
  handoff/docs/Ark_Handoff_Ledger_PRD_v2.md`), design-bundle templates
  (`~/Downloads/tmp/handoff/handoff/templates/.handoff/`), kernel cards HFTASK-0007/0011,
  `meta/handoff/hf/src/{main.rs,kb.rs}`, backlog TASK-0002/0003.

## Context

TASK-0002 asks: seed envctl's Tier-A `.handoff` via `hf` — render `active.md` + `packets/latest.md`,
mint `handoff.task.v1` cards, land `hf sync` (.kb write-back) — with **no per-repo `ledger.db`** and
**no hand-written packets**. TASK-0001 (build+install `hf`) is DONE.

## The design is SETTLED (not ambiguous) — ADR-0004 + PRD v2

The residency model I initially read as an "open ambiguity" is in fact **decided**:

- **PRD v2 §4.1–4.4:** the repo is the memory; **Git + ledger + task cards are authoritative; the
  handoff packet is a compiled, non-authoritative VIEW.**
- **ADR-0004 §2 (Tiered contents, policy P7):** a repo's `.handoff/` Tier-A layer = `context/
  capsule.json` (REQUIRED), `tasks/` (minted cards), `packets/` (resume packets), `README.md`;
  **OPTIONAL** `hooks/hooks.toml` + `policies/rules.toml` when the repo runs autonomous loops.
- **ADR-0004 §3 (Ledger residency, settles open-question #13):** **one witnessed ledger per
  orchestration home**; **per-repo `.handoff/` carries NO `ledger.db` / no binary state — git
  text only.** Events are checkpointed into the fleet ledger.
- **ADR-0004 §4:** cross-repo aggregation = **`hf fleet status`** (enumerate `../.meta.yaml`
  members, read each repo's capsule+cards, join with fleet-ledger events; Git is the sync transport).

So TASK-0002's "no per-repo ledger, packets are compiled not hand-written" is **correct by design**.

### Ledger location — reconciled, NO discrepancy
envctl **ADR-0001 §23** reconciles ADR-0004's wording: there are two orchestration homes —
`meta/.handoff/ledger.db` = **FLEET** (what envctl writes to) and `meta/handoff/.handoff/ledger.db`
= **KERNEL** (the kernel's own self-dev). Verified live: the fleet ledger is empty (`hf status` from
`meta` → 0 tasks); the kernel ledger holds the 23 `HFTASK-####` cards (`hf status` from
`meta/handoff` → 23 tasks). **Cycle-1's residency target (`meta/.handoff/ledger.db`) was correct.**

## The actual blocker — the installed `hf` is the S1 spike, missing the fleet verbs

envctl **ADR-0001 §22** documents the kernel's `hf` verb set as
`init/seed/status/claim/release/checkpoint/handoff/resume/ship/review/**policy**/**session**/**sync**/**fleet**/**drift**`.
The **installed binary** (built cycle 1 from `meta/handoff`, the S1 spike) exposes only
`init|seed|status|session|claim|release|checkpoint|sync-cards|done|task mint|ship|review|handoff|resume`.
**It has no `fleet`, no `policy`, no `drift`, and no standalone `sync` (only `sync-cards`).**

TASK-0002 needs exactly the **unbuilt** verbs:
1. **`hf fleet status` + fleet-aware packet rendering** — to compile envctl's `packets/latest.md` +
   `active.md` from the **fleet** ledger's envctl-scoped events (the shipped `hf handoff` compiles
   from the *CWD-relative* ledger, so per ADR-0004's "no per-repo ledger" it would compile from
   nothing). ADR-0004 §76 lists `fleet status` as a verb **"to implement."**
2. **`hf sync`** — the one-way `.handoff`→`.kb` write-back (ADR-0001 §6 / **HFTASK-0011**). The
   shipped binary has `sync-cards` (ledger→cards) but not `sync` (cards/ledger→`.kb`).
3. **`hf task mint`** writes a card to the **CWD** `.handoff/tasks/` and resolves `.kb` as
   `current_dir().parent()/.kb` — so minting envctl cards into the **fleet** ledger/text without a
   per-repo ledger also needs the fleet-aware path (or HFTASK-0007's `session` + `.handoff/policy.toml`).

These are **carded kernel work** in `meta/handoff` (HFTASK-0007 `session start|end` + `policy.toml`;
HFTASK-0011 `hf sync` `.kb` mirror; plus `hf fleet status` per ADR-0004 §4/§76) — **out of envctl's
scope**. The kernel ledger shows HFTASK-0007/0011 still `Backlog` (not Done).

## Correction (what v1 of this finding got wrong)
- ❌ v1: "shipped `hf` is strictly CWD-relative and needs a `--ledger`/`HANDOFF_DIR` flag." → The
  model **deliberately** has no per-repo ledger and no flag; a repo's packets are compiled centrally
  by the (unbuilt) fleet verbs from the fleet ledger. A `--ledger` flag is **not** the fix.
- ❌ v1 implied the design was an open ambiguity. → It is **decided** (ADR-0004, PRD v2).
- ✅ Correct blocker: the **installed `hf` is the S1 spike**; the **fleet/sync verbs are unbuilt**.

## What is ALREADY unblocked / done
envctl's **REQUIRED** Tier-A text core already exists and is git-committed: `context/capsule.json`,
`README.md`, `tasks/` + `packets/` dirs. The only residency-safe text gap is the **OPTIONAL**
`hooks/hooks.toml` + `policies/rules.toml` + `skills/` (design-bundle templates at
`~/Downloads/tmp/handoff/handoff/templates/.handoff/`) — static declarative text needing **no ledger
and no kernel change**, so they can be seeded now if desired.

## Decision required (owner / kernel team)

- **Option A — build the fleet verbs in the kernel, then re-run TASK-0002 (recommended).** Implement
  HFTASK-0007 (`hf session` + `.handoff/policy.toml`), HFTASK-0011 (`hf sync` `.kb` mirror), and
  `hf fleet status` + fleet-aware packet rendering in `meta/handoff`; install the upgraded `hf`
  (re-run TASK-0001's symlink — propagates automatically). Then TASK-0002/0003 proceed as written.
  Proper path, already carded. Route: `handoff-kernel-engineer` against `meta/handoff` (NOT an
  envctl cycle).
- **Option B — land the residency-safe text subset now, defer the rendered/minted/synced parts.**
  Seed envctl's OPTIONAL `hooks/policies/skills` from the design-bundle templates this session (no
  kernel dep), keep `packets/latest.md`+`active.md`+card-minting+`hf sync` blocked under A. Partial
  progress; TASK-0002 stays open until A lands.
- **Option C — rescope envctl Tier-A to the required text core (already satisfied).** Accept that
  envctl's per-repo packets are produced **centrally by `hf fleet status`** once built (ADR-0004 §4),
  so envctl never renders its own packet — close the envctl side, leaving only the kernel verb work.

## Consequences / linkage
- **TASK-0003 (p7 gate)** validates cards/packets + `hf resume --json` → `handoff.packet.v2`; needs
  the seeded/rendered layer, so it tracks A. The residency-invariant portion (assert no per-repo
  `ledger.db` tracked) is landable independently.
- **TASK-0001 GO-LIVE end-to-end** (a Stop firing `hf checkpoint --auto` writing a witnessed event)
  needs an **active task in the fleet ledger**; Option A's `hf session`/fleet-aware minting provides
  it. Until then the hook is live but its event-write is a correct no-op.

## Loop disposition
TASK-0002 `- [!]` blocked; TASK-0003 `- [!]` blocked (tracks A). The fix is kernel-side
(`meta/handoff` fleet verbs), so the envctl loop proceeds to the next unblocked item (Epic C
TASK-0012) pending the owner's choice of A/B/C.
