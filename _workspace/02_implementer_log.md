# Implementation log: Drift-severity summary

> Note: the rust-implementer agent completed all three source edits, then hit a transient
> server rate-limit before writing this log / running the final verify. The orchestrator
> completed the build/test/gate verification and recorded it here (SendMessage to resume the
> agent is unavailable in this environment).

## Changes
- `crates/engine/src/drift.rs`: added `use serde::{Deserialize, Serialize};`; new `DriftSummary`
  struct ({high,medium,low,total}) with `from_items(&[DriftItem]) -> DriftSummary` and a
  `Display` one-liner; new `#[cfg(test)] mod tests` with 3 tests.
- `crates/engine/src/lib.rs`: added `pub use drift::DriftSummary;` re-export.
- `crates/cli/src/main.rs`: imported `DriftSummary` from the engine; in the `Cmd::Graph` `--live`
  human-readable branch, `println!("{}", DriftSummary::from_items(&rep.drift))` when a live report
  is present. `--dot`/`--json` branches untouched.

## Engine API (parity contract)
`DriftSummary::from_items(&[DriftItem]) -> DriftSummary` — pure, non-printing, front-end-agnostic.
CLI consumes it now; GUI consumes the identical call in the follow-up (call site documented in the
plan: the GUI drift view fed by `Event::Report { report }`).

## Tests added
- `summary_counts_by_severity` — 2 High/1 Medium/3 Low → {2,1,3,6}.
- `summary_empty_is_zero` — `from_items(&[])` == default, total 0.
- `summary_display_wording` — pins `"Drift: 1 high, 0 medium, 2 low (3 total)"`.

## Build/test status
- `cargo build -p envctl-engine -p envctl` — PASS.
- `cargo test -p envctl-engine drift` — PASS (5 passed: 3 new + 2 existing drift tests).
- `cargo clippy -p envctl-engine -p envctl -- -D warnings` on the changed files — no findings in
  drift.rs/lib.rs/main.rs (the 6 workspace clippy errors are all in untouched files, see Handoff).
- CI gates: `no-c.sh`, `shape.sh`, `enable.sh` — all PASS.

## Deviations
- Did NOT print in `auto-detect`/`print_report` (the plan's optional step 4) — kept to the
  required `graph --live` scope.
- GUI wiring intentionally deferred (crate needs system dev libs; not buildable here).

## Handoff notes
- The change is read-only (no guard/`--apply` needed) — guardian's fail-closed check is N/A.
- **Baseline environment condition (not from this change):** the pinned toolchain is a floating
  `stable` = rustc/clippy 1.96; the workspace has pre-existing `cargo fmt --check` drift across
  ~25 files and 6 pre-existing clippy `-D warnings` errors (1.96-era lints) in files this change
  never touched (model.rs, lock.rs, addrepo.rs, command.rs). Verify these are pre-existing and out
  of scope; the feature itself adds none.
