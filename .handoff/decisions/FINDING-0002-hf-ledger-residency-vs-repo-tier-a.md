# FINDING-0002 — `hf` cannot seed a per-repo Tier-A `.handoff` against the shared ledger (TASK-0002 blocker)

- **Status:** NEEDS-DECISION (owner / kernel team) — blocks backlog **TASK-0002** (Epic A).
- **Date:** 2026-06-13 · **Surfaced by:** forge-loop agenticOS-consolidation cycle 2.
- **Cross-refs:** ADR-0001 (kasetto-handoff-portability-unification), ADR-0004 (single shared
  ledger), backlog `.handoff/loop/backlog.md` TASK-0002/0003, skill `handoff-sync` (P0 residency
  guard), `meta/handoff/hf/src/{main.rs,kb.rs}`.

## Context

TASK-0002 asks: *"Seed envctl `.handoff` via `hf` — render `policy.toml`, `hooks/hooks.toml`,
`policies/rules.toml`, `active.md`, `packets/latest.md`, `skills/`. Do NOT create a per-repo
`ledger.db`; do NOT hand-write packets."* i.e. envctl gets an hf-**rendered** Tier-A text/packet
layer, while the witnessed events live only in the **shared** `$META_ROOT/.handoff/ledger.db`
(ADR-0004). TASK-0001 (build+install `hf`) is DONE; this is the next Epic-A pick.

## The blocker (source-proven against the shipped binary)

The shipped `hf` (`meta/handoff`, built cycle 1) is **strictly CWD-relative** — there is **no
`--ledger` flag and no `HANDOFF_DIR`/`HANDOFF_LEDGER` env override** (`const HF=".handoff"`):

| fn (`hf/src/main.rs`) | resolves to |
|---|---|
| `ledger_path()` | `<CWD>/.handoff/ledger.db` |
| `tasks_dir()` | `<CWD>/.handoff/tasks/` |
| `packet_path()` | `<CWD>/.handoff/packets/latest.md` |

Three constraints then collide and are **mutually exclusive** with the shipped binary:

1. **Ledger-residency (ADR-0004, the #1 non-negotiable gate)** → every ledger-touching verb must
   run from `$META_ROOT` so the ledger is `meta/.handoff/ledger.db`. Running any mutating verb from
   the envctl worktree creates `envctl/.handoff/ledger.db` — FORBIDDEN.
2. **`hf task mint --from-kb` (`kb.rs::cmd_mint_from_kb`)** resolves the KB as
   `current_dir().parent()/.kb`. It therefore only finds `meta/.kb` when `hf` is run **from a child
   repo** (e.g. `envctl`) — which is exactly the run that creates the forbidden per-repo ledger. Run
   from `$META_ROOT`, `parent()` = `Desktop`, `Desktop/.kb` is absent → *"no meta `.kb/` found —
   cannot mint."*
3. **`hf seed` (`main.rs::cmd_seed`)** does **not** seed an "envctl Tier-A" layer — it writes the
   **kernel's own 22 `HFTASK-####` cards** (its `spike/**`,`handoff/**` self-buildout backlog) into
   `<CWD>/.handoff/tasks/`. `hf handoff` likewise renders packets into `<CWD>/.handoff`.

**Net:** there is no shipped mechanism to render envctl's Tier-A text/packet layer **while** the
ledger resides at the meta root. You either (a) run from envctl and create a forbidden per-repo
ledger, or (b) run from meta root and operate on the *kernel's* ledger/packets (not envctl's), with
`mint --from-kb` unable to find `.kb`. The envctl backlog (`TASK-0001..`) also is not present as
`.kb` task docs to mint from.

## Why this is a real blocker, not thrash

The residency invariant is the highest-priority non-negotiable gate (a wrong ledger is worse than no
ledger). No envctl-side change can satisfy TASK-0002 without violating it. The capability gap lives
in the **kernel** (`meta/handoff`), which is a separate `.meta.yaml` project, out of envctl's scope
and out of the loop's auto-fixable surface.

## Decision required (owner / kernel team)

Pick the path for Epic A to proceed:

- **Option A — add a ledger/dir split to `hf` (recommended).** Kernel feature in `meta/handoff`: a
  `--ledger <path>` flag and/or `HANDOFF_LEDGER`/`HANDOFF_DIR` env so a repo's text/packet layer
  (`<repo>/.handoff/...`) can be rendered while events go to `$META_ROOT/.handoff/ledger.db`; and
  make `mint --from-kb` resolve `.kb` from the meta root regardless of CWD. The `handoff-sync` skill
  already anticipates this ("if a future hf adds `--ledger`/`HANDOFF_DIR`, prefer that"). This is
  natural kernel work (cf. kernel cards HFTASK-0007 `.handoff/policy.toml`, HFTASK-0011 `hf sync`).
- **Option B — redefine "envctl Tier-A" as shared-ledger-only.** Accept that the *rendered* state
  layer (`active.md`, `packets/latest.md`) lives **only** at `meta/.handoff`, and envctl's per-repo
  `.handoff` stays the **hand/loop-maintained** text layer (`policy.toml`, `hooks/`, `skills/`,
  `loop/`) + the markdown `loop/HANDOFF.md` the loop already uses. Rescope TASK-0002 accordingly
  (no per-repo render). This needs no kernel change but contradicts TASK-0002's literal "render
  `packets/latest.md` per repo."
- **Option C — seed the kernel's own ledger/cards at meta root now.** Run `hf seed` + `hf handoff`
  from `$META_ROOT` to bring up the *kernel's* HFTASK backlog in the shared ledger (this would also
  give the TASK-0001 hook a real active task to witness). But these are the **kernel's** bring-up
  cards, mutating the shared fleet ledger — an owner/kernel call, not an envctl-loop action.

## Consequences / linkage

- **TASK-0003 (p7-conformance gate)** depends on a seeded Tier-A layer + `hf resume --json` emitting
  `handoff.packet.v2` — so it is **also blocked** behind this decision (the residency-invariant
  portion of the gate could land independently).
- **TASK-0001 GO-LIVE end-to-end proof** (a Stop firing `hf checkpoint --auto` writing a witnessed
  event) needs an **active task in the shared ledger**, which Option A/C would provide; until then
  the hook is live but its event-write is a correct no-op.

## Loop disposition

Mark TASK-0002 `- [!]` blocked (this finding), surface in the DONE/HANDOFF summary, and proceed to
the next actionable, unblocked backlog item per the dependency-aware order.
