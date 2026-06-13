---
name: rust-port-merge
description: >-
  How to MERGE a parity-verified Rust unit (ported from source repo X) into a destination Rust repo Y
  with NO downgrade — landing decision (new module vs merge-into-existing vs map-onto-Y-substrate),
  symbol-level conflict resolution via grit locks, reuse-over-duplicate, and re-verifying parity in
  Y's context. ALWAYS use when a port has a destination repo (`dest_repo` Y), when merging ported Rust
  into another repo, or on "port X and merge into Y", "merge the rust code into <repo>", "reconcile the
  port with <repo>". The rust-port → rust-port-merge arc of ADR-0001. Behavior preserved across the move.
---

# Rust-Port Merge

Porting a unit to Rust is half the arc; **merging it into destination repo Y** is the other half
(ADR-0001's `rust-port → rust-port-merge`). A merge is not a copy-paste — it *places, reconciles,
de-duplicates, and wires* the ported unit into Y's existing crates and substrates, then re-proves the
behavior **in Y's context**. The guarantee is the same as the port: **no feature logic left behind, no
downgrade — now across the merge too.** Used by `rust-port-merge-integrator`; the gate is
`rust-port-parity-verifier` re-run in Y.

## When this runs

Only when the run has a `dest_repo` Y (in `loop_state.md`). The ITERATE cycle is **classification-driven**
(see the unit classes in `references/merge-ledger.md`): a `port-fresh`/`extend-Y` unit is ported then
merged; a `reuse-Y`/`map-onto-substrate` unit **skips the fresh port** and instead verifies Y's existing
symbol (or the substrate) against source X directly — so the loop never re-ports what Y already provides.
Per unit: (port-or-verify) → **merge into Y** (in the Y worktree) → build-health (Y) → **re-parity-verify
in Y** → **Y-regression check** → commit (to the Y branch). A unit is `merged` only after re-verification
passes in Y — a standalone PASS is necessary, not sufficient.

## Y git discipline — the merge writes to a real, separate repo

Y is a *different* git repo, so the harness's port-repo commit-per-cycle does not cover it. Apply the
owner's standing workflow **to Y**:
- **Worktree-per-task + feature branch.** At DISCOVER create a per-task git **worktree** of Y on a
  **feature branch** (`dest_worktree` on `dest_branch`, off `dest_base` in `loop_state.md`) — never
  merge onto Y's `main`. The worktree isolates the merge and gives **atomic rollback** (discard the
  worktree = undo the cycle).
- **Commit per merge cycle, in Y.** A merge run produces **two commits per cycle**: the port-repo
  `.handoff/loop/` state commit **and** a commit on `dest_branch` in the Y worktree with the merged
  Rust — committed only when the cycle's gates pass (see atomicity below).
- **PR + auto-merge into Y at merge-DONE.** When the merge ledger is 100% (+ Y green + Y-not-regressed),
  open a PR from `dest_branch` into `dest_base` and arm auto-merge (`gh pr merge --auto`, the repo's
  allowed method) — the same fail-closed push→PR→self-merge-on-green flow every repo uses.
- **grit symbol locks** on the Y symbols touched, released on **commit or rollback** (never leak a lock).

## The landing decision (record per unit in `merge-ledger.md`)

For each parity-verified unit, decide where it lands in Y — informed by the **researcher's reuse map**
(`reports/research.md`: what Y already provides) and the **cross-repo reference map**
(`reports/cross-repo-refs.md`: who references what):

| Landing | When | Rule |
|---------|------|------|
| **New module/crate in Y** | Y has nothing equivalent | place in Y's layout, wire imports/exports/Cargo, follow Y's conventions |
| **Merge into existing Y module** | Y has a partial/overlapping impl | unify — *complete* Y's version with X's behavior; a duplicate is an incomplete unification (no-downgrade directive), wire it, don't leave two |
| **Map onto a Y substrate** | the unit is a runtime construct Y delegates to a substrate | map onto `hf`/`weave`/`grit`/`icm`/provider-CLI per `rust-port/references/runtime-constructs.md` — only if it preserves every behavior |

The landing follows the unit's **class** (`merge-ledger.md`): `port-fresh`→new module, `extend-Y`→merge
into existing, `reuse-Y`→reuse Y's symbol (verify-only), `map-onto-substrate`→substrate. **Before any
"new module" landing, dup-scan Y** (`git kb code symbols`/`callers` search for an equivalent symbol) —
the reuse map is advisory and can be stale; a missed existing Y impl would create the very duplicate the
no-downgrade directive forbids. Found a near-duplicate → it's really `extend-Y`/`reuse-Y`, reclassify.

**Reuse > duplicate, but never reuse-by-narrowing.** Mapping onto a Y symbol/substrate is legal only if
it preserves **every** behavior in the unit's contract. A near-fit that loses a behavior is *extended*
(complete the feature), or — if genuinely impossible — a `- [!]`/`- [≠]` owner-decision, never a silent
narrowing.

## Symbol-level conflict resolution (grit, parallel-safe)

- Read the **blast radius** for the symbols you touch from `reports/cross-repo-refs.md` (every Y caller
  affected). Take a **grit symbol lock** on those Y symbols so a concurrent merge can't corrupt shared
  state; release on commit. Coarser-than-symbol locking is allowed (stricter is fine); skipping it is not.
- Resolve a collision by **unifying** the two sides (complete/merge), never by dropping one. If you
  truly can't reconcile without losing behavior, keep both, flag it, and route the decision up.
- A merge that touches a **contract Y's consumers depend on** (shared protocol/API/type) is checked for
  compatibility (the protocol-drift method via the cross-repo-referencer) — a wire/type change is a
  breaking change to flag, not silently ship.

## The no-downgrade-across-the-merge gate (bidirectional + atomic)

The move can downgrade in **two directions**, and both are gated:

1. **Don't downgrade X (the ported behavior).** The move can drop a re-export, narrow a type to fit Y,
   lose an error variant, or collapse a streaming path during integration. So **re-run the differential
   parity gate in Y's context** — the merged code, called through Y, must still match source X over the
   unit's whole contract (happy + every error/edge + runtime behaviors), exercising **every symbol** of
   the unit (symbol rollup holds across the merge). Only that re-PASS counts toward `- [x]`.
2. **Don't downgrade Y (its own existing behavior) — the dual gate.** A green Y build with passing Y
   tests can still change a Y-consumer-visible semantic on an untested path. So capture **Y's own
   behavioral baseline at DISCOVER** (Y's test suite + golden fixtures for each unit's Y blast-radius
   from `cross-repo-refs.md`) and, after the merge, **re-run it and diff** → `findings/y-regression.md`.
   A merge that changes a Y-consumer behavior without owner approval is a **downgrade-of-Y** (`- [!]`/
   `- [≠]`) — symmetric with the X rule. Y must also stay green (`cargo build`/`clippy`/`test`).

**Atomic.** A unit flips to `- [x]` (and its Y changes commit to `dest_branch`) **only when all three
pass**: re-verify-against-X **and** Y-green **and** Y-not-regressed. On any failure,
`git -C <dest_worktree> reset --hard` (restore Y to last-green HEAD), **release the grit locks**, and
mark `- [~]` with the exact breakage — never leave a broken half-merge in Y's tree.

## Breaking-contract resolution (resolve, don't just flag)

The cross-repo-referencer **flags** when a merge would change a contract Y's *consumers* depend on (a
shared protocol/API/type — which may compile in Y while breaking other repos). Flagging is not enough —
the DONE gate's "no Y consumer contract broken" can otherwise *only* block, never be satisfied when a
contract genuinely must change. Resolve it the no-downgrade way:
- **(a) Additive / back-compat** — keep the old surface, add the new alongside (no consumer breaks).
- **(b) Adapter / shim** — a compatibility layer so existing consumers keep working unchanged.
- **(c) Versioned bump** — version the contract and propagate to consumers via the **protocol-drift
  method** (the `protocol-drift-scan` skill), updating each consumer in scope.
Only when consumers genuinely cannot be updated in scope is it a `- [≠]` with owner approval — never a
silent breaking ship.

## Y-drift on resume (Y is a moving target)

A port-and-merge spans many sessions; Y's base advances under the harness. On resume (and as a per-cycle
pre-check), **fetch Y, rebase `dest_branch` onto `dest_base`, re-index Y, and re-run the
cross-repo-referencer over the merged set**. Any `- [x]` merged unit whose Y blast-radius changed drops
to `- [~]` for re-verification — a merge proven against an old Y is not proven against the new Y. (See
`merge-ledger.md` "Y is mutable".)

## Merge-ledger & DONE

- Ledger schema + classification + status legend + ordering: `rust-port/references/merge-ledger.md`.
- When `dest_repo` is set, **DONE also requires**: the merge ledger at 100% (every unit `- [x]` merged +
  re-verified in Y, or owner-approved `- [≠]`); a merge left-behind sweep (no ported unit unmerged);
  Y's `build`/`clippy`/`test` green; **Y not regressed** (the Y-regression diff clean); and **no Y
  consumer contract broken** (or resolved per the breaking-contract section). The port-only DONE
  conditions still all apply.
- **At merge-DONE, open the PR into Y** from `dest_branch` → `dest_base` with auto-merge armed.
