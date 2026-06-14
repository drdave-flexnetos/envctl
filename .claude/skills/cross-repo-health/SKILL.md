---
name: cross-repo-health
description: >-
  How to establish and gate on a green build/test/lint baseline across the meta workspace.
  ALWAYS use when checking whether repos build/test/lint cleanly, establishing the verify-on-resume
  baseline, or gating a loop cycle on health. Triggers on "is it green", "build health", "run the
  tests", "clippy", "cargo check across repos", "baseline", "did my change regress anything".
  Rust-native: cargo + the meta CLI; failures-only reporting.
---

# Cross-Repo Health

The loop never claims "done" on top of a red tree. This skill is how `build-health-auditor`
produces the green baseline that gates every cycle.

## Checks, per repo (Rust-native)

| Check | Command | Applies to |
|-------|---------|------------|
| Compile | `cargo check` | every Rust crate |
| Build | `cargo build` | crates where check isn't enough (build scripts, bins) |
| Lint | `cargo clippy -- -D warnings` | crates that opt into clippy gating |
| Test | `cargo test` | crates with tests |
| Catalog | `bash scripts/validate.sh` | hubs (harness_hub, sibling hubs) |
| Registry shape | check `meta-plugins/plugins/*` point at live repos | meta-plugins |

Run broad sweeps through the `meta` CLI so you don't hand-`cd`:

```
meta exec -- cargo check                 # all repos
meta --include meta_plugin_protocol,meta_plugin_api,meta_cli exec -- cargo test
```

Use `rtk cargo …` where available to compress output (the goal is failures, not full logs).

## Reporting — failures only

Write `.handoff/loop/findings/health.md` as a matrix: `repo → {check,build,clippy,test,validate} →
pass | fail | skip`. For each `fail`, include the **minimal reproducing command** and a short
output excerpt — never the whole log. A check you didn't run is `skip` (with the reason), never a
silent pass.

## The verify-on-resume baseline

Write `.handoff/loop/baseline.md` with the exact command block a successor session runs FIRST to
confirm green before continuing. Keep it small and fast (the in-scope repos' `cargo check` +
`bash scripts/validate.sh`), not the full test matrix — it's a gate, not the audit.

## Gating rules

- **Gate, don't fix** by default. The verdict (GREEN / RED) is the deliverable. A trivial fix you
  introduced this cycle you may correct; anything non-trivial becomes a routed backlog item.
- **Honest red.** Report red with evidence; never report green you can't reproduce in a fresh
  shell. Environmental failures (missing target, no network) are `skip` with the reason, not `fail`
  — and they don't block the whole loop on one repo.
- **Bounded coverage.** "All repos" is large; log which repos were checked and which were deferred
  so partial coverage never reads as complete.

## Human walls

If a check needs sudo / interactive auth / a reboot, surface `NEEDS-HUMAN` with the reason to the
orchestrator — do not spin or force.
