# Verification report: Drift-severity summary (engine helper + `graph --live` CLI line)

## Verdict — PASS-WITH-NOTES

All NON-NEGOTIABLE invariants hold, all three CI gates pass, the engine builds and its full
test suite (incl. the 3 new drift tests) is green, and the diff is exactly the contracted
3-file change. One **minor, non-blocking** finding: the feature's own new test code in
`drift.rs` is not `cargo fmt --check`-clean under the pinned floating `stable` (rustfmt 1.96).
This is mechanical (two single-line struct literals rustfmt wants expanded) and is the only
fmt issue traceable to this change; everything else flagged by fmt/clippy is pre-existing
1.96-era baseline noise in untouched files. Not a regression of any invariant — hence
PASS-WITH-NOTES rather than FAIL.

## Gate results
- `ci/gates/no-c.sh` — **PASS** ("NO-C GATE PASS"; `rustls=['0.23.40'] on ring=['0.17.14']`,
  zero aws-lc/openssl/C-SQLite). exit 0.
- `ci/gates/shape.sh` — **PASS** ("SHAPE GATE PASS"). exit 0.
- `ci/gates/enable.sh` — **PASS** ("ENABLE GATE PASS"). exit 0.

## cargo
- build (`cargo build -p envctl-engine -p envctl`) — **PASS**, exit 0.
- test (`cargo test -p envctl-engine`) — **PASS**: 19 passed / 0 failed in the lib bin, plus
  17 passed in the secondary set; the 3 new tests are green:
  `drift::tests::summary_counts_by_severity`, `summary_display_wording`, `summary_empty_is_zero`.
- fmt (`cargo fmt --all -- --check`) — **FAIL (mostly baseline; 1 feature-owned hit)**. See
  Findings F1. The vast majority of the diff is pre-existing whole-file drift in untouched files
  (executor.rs, wiring.rs, guard.rs, lib.rs module-order reorder, drift.rs `compute()` lines
  46/60/67, etc.). The **only** fmt diff traceable to this change is in the new test module:
  `drift.rs:169` and `drift.rs:182`.
- clippy (`cargo clippy -p envctl-engine -p envctl -- -D warnings`) — **FAIL, all pre-existing**:
  6 errors, all in files this change never touched — `model.rs:96`, `lock.rs:7`, `lock.rs:8`,
  `addrepo.rs:114`, `addrepo.rs:240`, `command.rs:17`. Zero findings in drift.rs / lib.rs /
  main.rs. Confirms the implementer's baseline claim. Not a feature regression.

## Invariant checks (1–8 from invariant-guardian.md)
1. **No C in trust boundary** — **PASS**. `no-c.sh` green; no dep added (diff adds only
   `use serde::{Deserialize, Serialize};`, serde already present). One rustls (0.23.40) on ring.
2. **Code-shape** — **PASS**. `shape.sh` green; no native-roots / accept-invalid TLS tokens.
3. **secretd enable** — **PASS**. `enable.sh` green (unrelated surface, untouched).
4. **Engine purity (non-printing, logic-in-engine)** — **PASS**. Diff over `crates/engine` adds
   NO `println!`/`eprint*`/`print!`/`stdout()` (`git diff … crates/engine | grep` → "engine
   clean"). The only added `println!` is in the CLI front-end (`crates/cli/src/main.rs:195`).
   `DriftSummary::Display::fmt` writes into the caller's `Formatter` — never stdout. Logic
   (the fold + wording) lives in the engine; the CLI only renders.
5. **Front-end parity** — **PASS (documented asymmetry)**. See Parity check below.
6. **Fail-closed / dry-run defaults** — **PASS (N/A)**. Read-only feature: no mutation, no
   guard, no `--apply`/`--build`. `live_report` is read-only `engine.detect()`. Correctly N/A.
7. **Rust-native, no drift** — **PASS**. `git status --porcelain` shows only the 3 `.rs` files
   modified (+ untracked `_workspace/` harness scratch). No `.js/.ts/.py/.omc`/`package.json`/
   `node_modules`. No foreign file, no banned dep.
8. **Lock honesty** — **PASS (N/A)**. No dep / component / manifest change → no `envctl.lock` /
   `kasetto.lock` update needed. Matches plan §"Lock/manifest sync".

## Parity check — Engine method → front-end callers
- **Engine API:** `DriftSummary::from_items(&[DriftItem]) -> DriftSummary` —
  `crates/engine/src/drift.rs:118` (impl), re-exported at `crates/engine/src/lib.rs:26`
  (`pub use drift::DriftSummary;`). Pure, front-end-agnostic, consumes the public
  `report.drift: Vec<DriftItem>` every `EnvReport` already carries.
- **CLI caller:** `crates/cli/src/main.rs:195` —
  `println!("{}", DriftSummary::from_items(&rep.drift));`, inside the human-readable `else`
  branch of `Cmd::Graph`, guarded by `if let Some(rep) = live_report.as_ref()`. `live_report`
  is `Some` only under `--live` (`main.rs:172-177`). The `--dot` (`:180`), `--json` (`:182`),
  `--impact` (`:183`), `--why` (`:188`) branches are byte-for-byte unchanged — verified in diff.
- **GUI caller:** not present (acceptable, **documented**). The GUI crate needs system dev libs
  and cannot build in this environment. The plan (`01_architect_plan.md:74-77`) names the exact
  GUI call site, and I confirmed it is real and shape-compatible: `crates/gui/src/main.rs:165`
  handles `Event::Report { report }` and already consumes `report.drift` (`:173`
  `report.drift.len()`, `:181` `self.drift = report.drift`). So `DriftSummary::from_items(
  &report.drift)` is a drop-in identical engine call there — same shape the CLI passes. The
  asymmetry is justified per the plan; parity is preserved at the API level.

## Findings
- **F1 — minor / fmt (feature-owned).** `crates/engine/src/drift.rs:169` and `:182`: the new
  test module writes struct literals on one line —
  `DriftSummary { high: 2, medium: 1, low: 3, total: 6 }` and
  `let s = DriftSummary { high: 1, medium: 0, low: 2, total: 3 };` — which rustfmt 1.96 wants
  expanded to multi-line. This is the only `cargo fmt --check` diff attributable to this change.
  *Suggested fix:* run `cargo fmt -p envctl-engine` (or expand those two literals by hand).
  Non-blocking: it's test-only, mechanical, and the surrounding workspace already fails fmt
  pre-existingly, so it does not change the gate's pass/fail state for the repo. Recommend
  fixing before commit for hygiene.
- **F2 — note / baseline (NOT this change).** Whole-workspace `cargo fmt --check` drift (~25
  files incl. executor.rs, wiring.rs, guard.rs, lib.rs module-order, drift.rs `compute()` lines
  46/60/67) is pre-existing 1.96-era reformatting. Confirmed by inspecting the diff hunks: they
  reformat code this change never touched.
- **F3 — note / baseline (NOT this change).** 6 `clippy -D warnings` errors, all in untouched
  files (model.rs:96, lock.rs:7/8, addrepo.rs:114/240, command.rs:17). Confirmed zero clippy
  findings in the 3 changed files. Environment/baseline issue; out of scope for this feature.

## Re-test needed
After F1 fix only (F2/F3 are out-of-scope baseline):
```bash
cd /home/drdave/Desktop/meta/.worktrees/harness-upgrade/envctl
cargo fmt -p envctl-engine
cargo fmt -p envctl-engine -- --check        # expect: drift.rs no longer listed
cargo test -p envctl-engine                  # expect: 3 new drift tests still green
```
No need to re-run the gates / build / parity checks — F1 is a test-only whitespace change and
cannot regress them.

---

## Orchestrator adjudication of F1 (post-report)

The orchestrator adjudicated finding **F1** (the two test struct literals rustfmt 1.96 wants
expanded) as **no-action / reclassified into the toolchain baseline**, on this evidence:

- There is **no `cargo fmt --check` CI gate** in the repo (no `.github/workflows`, no fmt gate in
  `ci/`); fmt is a local discipline, not a landing blocker.
- Committed `master` keeps short struct literals **single-line** in untouched code
  (`crates/engine/src/executor.rs:74,102` — `Event::GuardRefused { component: …, reason: … }`),
  the exact style the new test code follows. So the feature is **style-consistent with the
  committed repo**; the diff F1 reports is the same rustfmt-1.96-vs-committed-toolchain delta that
  hits ~25 untouched files.
- Expanding only these 2 lines to satisfy 1.96 would make them the **only** 1.96-style code in an
  otherwise older-style repo — reducing consistency, not improving it.

**Resolution:** leave the feature code as-is (consistent with committed style). Proper remediation
is a **separate, workspace-wide task**: pin the toolchain to a fixed version and/or run a single
`cargo fmt --all` + `clippy` reconciliation for rustfmt/clippy 1.96 across the whole repo. Final
verdict stands: **PASS** for the feature; the fmt/clippy items are a pre-existing environment
baseline, tracked separately.
