---
name: rust-port-parity
description: >-
  How to PROVE behavioral parity between a Rust port and its source via differential/golden testing
  — run both over the same inputs and diff outputs, side effects, and error behavior. ALWAYS use to
  verify a ported unit before it counts as done, to capture golden fixtures, or to decide PASS/FAIL
  on parity. Triggers on "verify parity", "does the Rust match the source", "differential test",
  "golden test the port", "prove no downgrade". Behavior comparison, not existence checking.
---

# Rust-Port Parity

"It compiles" and "the function exists" are not parity. This skill proves a unit is faithful by
**running source and Rust over the same inputs and diffing observable behavior** — or rejects it.
Used by `rust-port-parity-verifier`; it is the no-downgrade gate.

## Differential method

1. **Pick inputs that cover the unit's whole contract** (from the ledger row): happy path + **every
   error branch** + edge/empty/null + boundary values + ordering/concurrency cases where relevant.
   Untested branches are unproven → the unit cannot be `- [x]`.
2. **Capture source behavior** as golden fixtures — run the source unit (or reuse its existing tests)
   and record: return values, serialized output (exact JSON/wire shape), error kind/message where
   contractual, and side effects (files written, DB rows, network calls, stdout/exit codes).
3. **Run the Rust** over the identical inputs and **diff** against the golden fixtures.
4. **Verdict:** `PASS` only if every contract behavior matches; `FAIL` with the exact input +
   expected(source) vs actual(Rust); `INCONCLUSIVE` if one side won't run (then the unit stays open).

## Techniques

- **Golden/snapshot** (e.g. `insta`) for serialized outputs; commit fixtures under the Rust crate's tests.
- **Property/differential** — generate random inputs, assert source and Rust agree (great for pure units).
- **Side-effect capture** — sandbox fs/DB; compare the *set* of effects, not just return values.
- **Runtime/concurrency differential** — for DAG executors, run-loops, providers, and gates: feed a
  **streaming** input and diff chunk sequence + timing (not a buffered blob); feed a **parallel**
  workload and assert layers run concurrently with the source's result-ordering; feed a
  **cancel/timeout** and assert the run aborts at the right point and drains; assert the
  **backpressure** bound. Collapsing parallel→sequential, streaming→one-shot, or cancellable→stuck
  is a downgrade that a happy-path PASS would hide.
- **Wire compatibility** — for APIs/serialization, assert byte/field-level shape equality so existing
  clients keep working (a renamed JSON field is a downgrade).

## Gate discipline (fail-closed)

- **Default skeptical.** If you can't reproduce parity, it's not done — return `- [~]`/`- [!]`, never
  wave it through. A green build is the verifier's precondition, not its verdict.
- **Cover the contract.** A unit with N error paths needs N exercised; a "happy-path PASS" is a FAIL
  in disguise.
- **Divergence is explicit.** A deliberate behavior change passes only as a `- [≠]` ledger row with
  recorded owner approval — never an unflagged "close enough."
- **Persist the trail.** Write each verdict + evidence to `.handoff/loop/findings/parity.md`; that
  audit trail is the proof of no-downgrade.
