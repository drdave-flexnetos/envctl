# Baseline — rust-port target build health (2026-06-13)

Established on commit `6ecb270` (the seed crate), in worktree
`/home/drdave/Desktop/meta/.worktrees/task-0012-agent-env/envctl`. Verified via `rtk proxy cargo …`
(raw passthrough — the shell hook rewrites cargo→rtk and corrupts diagnostics otherwise).

| Gate | Command | Result |
|------|---------|--------|
| build | `rtk proxy cargo build -p envctl-agent-env` | **PASS** (exit 0) |
| test | `rtk proxy cargo test -p envctl-agent-env` | **PASS** — 61 unit + 1 integration (`all_six_keys_plus_extends_round_trip`) |
| clippy | `rtk proxy cargo clippy -p envctl-agent-env -- -D warnings` | **PASS** (exit 0) |
| fmt | `rtk proxy cargo fmt -p envctl-agent-env -- --check` | **PASS** (exit 0) |
| no-c | `bash ci/gates/no-c.sh` | **PASS** — `rustls=['0.23.40'] on ring=['0.17.14']; zero aws-lc/openssl/C-SQLite`; mimalloc-free |

The Rust target skeleton builds green. This is the floor every port cycle must keep green (the
build-health gate). The parity gate (differential vs kasetto v3.2.0) is the SEPARATE, stronger gate
that upgrades `- [~]` → `- [x]`.
