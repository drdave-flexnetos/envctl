# TASK-0014 guardian report — PASS-WITH-NOTES (clean to commit)
All 5 invariants upheld (engine non-printing single-lib unchanged; fail-closed apply=false
default ASSERTED on-disk; exit-code contract; no new dep/no-c; --json global). Real gates:
cargo test -p envctl 18 passed; clippy --workspace -D warnings clean; fmt clean; no-c/shape/
enable PASS; smoke `agent --help`/`agent list --json` OK. Zero engine src change (CLI-only).
NOTE (non-blocking, out of scope): 4 pre-existing unnecessary_to_owned lints in
crates/engine/tests/agent_sync_parity.rs (#84, --all-targets only — NOT the CI gate). Follow-up.
