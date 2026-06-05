# Plan: Drift-severity summary (engine helper + `graph --live` CLI line)

VERDICT: GO

## Summary
Add a pure, deterministic engine helper that folds the `Vec<DriftItem>` already produced by
`drift::compute` into a `DriftSummary` of counts by `Severity` (high / medium / low + total).
Surface it as a single concise line in the CLI `graph --live` output path. The engine computes
and returns data only; the CLI owns all printing, preserving engine-first architecture and
front-end parity.

## Placement
- **Engine (`crates/engine`)** — all logic. Add `DriftSummary` + constructor in
  `crates/engine/src/drift.rs` (next to `compute`). Re-export from `crates/engine/src/lib.rs`.
- **CLI (`crates/cli`)** — one render line in the `Cmd::Graph` `--live` arm of
  `crates/cli/src/main.rs`.
- **GUI (`crates/gui`)** — parity follow-up only (needs system dev libs; won't build here). Call
  site named below.

## Engine API delta
Purely additive; no existing signature changes. `detect::run` (`crates/engine/src/detect.rs:95-96`)
already populates `report.drift = drift::compute(&report, reg)`, so every `EnvReport` carries the
items the summary needs.

New in `crates/engine/src/drift.rs`:

```rust
use crate::model::{DriftItem, Severity};

/// Counts of drift items by severity. Pure, non-printing; the CLI/GUI render it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftSummary {
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub total: usize,
}

impl DriftSummary {
    /// Fold drift items into per-severity counts. Deterministic.
    pub fn from_items(items: &[DriftItem]) -> DriftSummary {
        let mut s = DriftSummary::default();
        for d in items {
            match d.severity {
                Severity::High => s.high += 1,
                Severity::Medium => s.medium += 1,
                Severity::Low => s.low += 1,
            }
            s.total += 1;
        }
        s
    }
}

impl std::fmt::Display for DriftSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Drift: {} high, {} medium, {} low ({} total)",
            self.high, self.medium, self.low, self.total)
    }
}
```

- `Serialize, Deserialize` (serde, already a dep) for JSON-friendliness, matching repo idiom.
- `Display::fmt` writes into a caller `Formatter` — never stdout — so the non-printing invariant
  holds. Both front-ends get identical wording.

CLI consumption (`crates/cli/src/main.rs`, `Cmd::Graph { .. live }` human-readable summary branch,
~line 192): when `live` produced a report, `println!("{}", DriftSummary::from_items(&rep.drift))`.
`--dot` and `--json` branches stay byte-for-byte unchanged.

lib.rs re-export (`crates/engine/src/lib.rs`; `pub mod drift;` already at line 13): add
`pub use drift::DriftSummary;`.

GUI parity (follow-up): the GUI drift view (analogue of CLI `print_report` drift block ~line
706-721) computes `DriftSummary::from_items(&report.drift)` from the `Event::Report { report }`
payload (`event.rs:37-39`) and renders `summary.to_string()` as a header. Identical engine call;
not buildable here.

## Invariant check
- #1 No C in the trust boundary: PASS — no new dep; only std + already-present serde.
- #2 Exactly one rustls, ring-only: PASS — no TLS/CA surface touched.
- #3 Engine is the shared, non-printing lib: PASS — helper returns data; Display writes to
  Formatter; all println! stays in CLI; CLI+GUI call identical helper.
- #4 Destructive ops fail-closed + dry-run: PASS (N/A) — read-only; no mutation/guard/--apply.
- #5 Rust-native only: PASS — only Rust edited.
- #6 Reproducible state: PASS — no dep/component/manifest change.

## Safety guards
None required — pure read-only computation + print.

## Lock/manifest sync
No changes. No deps added, no components changed, trust boundary untouched.

## Work breakdown (leaf-first)
1. Engine type + helper in `crates/engine/src/drift.rs` (add `use serde::{Deserialize, Serialize};`).
2. `#[cfg(test)] mod tests` at bottom of `drift.rs` (file currently has none). DriftItem fields are
   public (`model.rs:234-240`).
3. Re-export `pub use drift::DriftSummary;` in `lib.rs`.
4. CLI wiring (required): print the summary in the `Cmd::Graph` live human-readable branch.
   (Optional: also in `print_report` ~line 706 so `auto-detect` shows it — not required.)
5. `cargo fmt --all`; `cargo clippy --workspace -- -D warnings`; `cargo test -p envctl-engine`;
   `cargo build -p envctl-engine -p envctl`.
6. GUI parity follow-up (note or implement when buildable).

## Verification plan
- Unit tests in `drift.rs`:
  - `summary_counts_by_severity`: 2 High / 1 Medium / 3 Low → {2,1,3,6}.
  - `summary_empty_is_zero`: `from_items(&[])` == default, total 0.
  - `summary_display_wording`: `{high:1,medium:0,low:2,total:3}.to_string()` ==
    `"Drift: 1 high, 0 medium, 2 low (3 total)"`.
- Build/format/lint: build engine+cli; fmt; clippy -D warnings.
- CI gates: none gate this change; run `no-c.sh` once as cheap confirmation. `cargo test --workspace`.
- Manual smoke: `graph --live` shows the line; `--dot`/`--json` unchanged.

## Open questions
None blocking. Optional micro-decision: also print in `auto-detect`/`print_report` (defaulted to
NOT, required scope is `graph --live`).
