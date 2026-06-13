# Loop state — rust-port
session_started: <UTC e.g. 2026-06-13T15:00:00Z>   # you supply it; the runtime can't read the clock
loop: rust-port
branch: <branch>
worktree: <abs path>
source_root: <abs path of the project being ported, e.g. ~/Desktop/meta/Archon>
source_toolchain: <bun | node | python | ...>     # needed so the parity-verifier can RUN the source
rust_target: <abs path / crate of the Rust port>
dest_repo: none     # <abs path of destination repo Y for port-and-merge>, or `none` for port-only
dest_branch: <Y feature branch for this merge run, e.g. merge/from-archon>   # only when dest_repo != none
dest_worktree: <abs path of the per-task git worktree of Y on dest_branch>   # owner rule: worktree-per-task; gives atomic rollback
dest_base: <Y base branch the PR targets, e.g. main>   # for Y-drift rebase + the final PR
cycle_budget: 3
cycles_this_session: 0     # reset to 0 on RESUME
cycles_total: 0
ledger: parity 0/<total_units> units verified
symbols: 0/<total_symbols> symbols mapped+verified   # X = symbols at [x]/[≠]; Y = harvested+visibility-filtered source symbols (git kb code symbols --json --limit -1; empty harvest of non-empty source = NEEDS-HUMAN)
merge: 0/<total_units> merged+reverified-in-Y   # only when dest_repo != none; X = units [x] in merge-ledger.md (merged + re-verified in Y); else N/A
classes: port-fresh=? extend-Y=? reuse-Y=? map-onto-substrate=?   # up-front unit classification from the researcher reuse map (drives ITERATE; reuse-Y/map-onto skip the fresh port)
last_item: (none — discovery only)
status: DISCOVER complete — parity ledger seeded
last_update: <UTC>
