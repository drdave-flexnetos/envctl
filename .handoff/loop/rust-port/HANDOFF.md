# HANDOFF — rust-port-merge (kasetto → envctl, Epic C absorption)
closed_utc: 2026-06-14T02:45:17Z   branch: develop   worktree: /home/drdave/Desktop/meta/.worktrees/<fresh>/envctl (create off origin/develop)
cycle_budget: 3   cycles_total: 9   cycles_this_session: many (library + engine wiring landed)
last_item: TASK-0013 (engine wiring)   next_item: parity-verifier pass ([~]→[x]) THEN TASK-0014 (CLI/GUI front-end)
orchestrator_phase: DONE-through-engine (merge-ledger 0 [ ] remaining)   last_agent: invariant-guardian (PASS)
gate_status: PASS (498 workspace tests; no-c/shape/enable green)   pr_url: #78 (MERGED)

**Resume with:** `/harness:rust-port-merge` (or `/feature-forge` for TASK-0014). State lives in
`.handoff/loop/rust-port/` (namespaced — NOT the flat `.handoff/loop/`, the forge-loop's).

## Where it stands (all on origin/develop)
The kasetto absorption is **structurally COMPLETE through the Engine**. `crates/agent-env` = 18-module
pure-Rust port of **pivoshenko/kasetto v3.2.0**; `crates/engine/src/agent/*` = the 6 `Engine::agent_*`
methods. **merge-ledger: 102 `[~]` merged / 0 `[ ]` to-merge / 13 `[≠]` front-end / 22 `[x]` parity-verified.**

landed_this_session:
  - PR #71  agent-env seed + model/* port
  - PR #72  rust-port-merge harness eject + verify-merge classification
  - PR #73  MCP additive-never-clobber merge (MC-01/MC-02) — left-behind sweep catch
  - PR #75  command transforms (PR-01) + config_edit (FE-*) + fsops resolution
  - PR #76  source discovery + runtime/profile/config_path/sync — LIBRARY COMPLETE
  - PR #78  engine wiring — Engine::agent_{sync,add,remove,lock,list,clean} (TASK-0013)
  - (peer) meta PR #31 retarget kasetto repo; kasetto fork origin/main force-synced to v3.2.0
  - #74 was a duplicate of #71 (closed by owner — work already on develop)

## next_item — two tracks (either order)
1. **Parity-verifier pass:** drive the 80 remaining `[~]` → `[x]` by extending
   `crates/agent-env/tests/parity_vs_kasetto.rs` (golden vectors VERBATIM from kasetto v3.2.0's own
   `#[cfg(test)]` modules — `cargo test` in meta/kasetto @ ec01cca passes 216 of them). 22 done.
2. **TASK-0014 (front-end, the 13 `[≠]`):** CLI verbs `envctl agent {sync,add,remove,lock,list,clean}`
   (clap) + GUI parity. THIN ADAPTER over the engine methods — build `Agent*Spec`, call
   `Engine::agent_*`, drain the `EventSink` to render the tree/grid, map `report.summary.failed>0` →
   exit code, `--json` serializes the (already `Serialize`) return. The engine API was designed for this.

## findings / decisions_and_dead_ends (don't re-litigate)
- **Source of truth = pivoshenko/kasetto v3.2.0** (the `upstream` remote), NOT the FlexNetOS fork (was
  v3.0.0+divergent). Fork renamed env_manager_agent→FlexNetOS/kasetto, origin/main force-synced to
  v3.2.0; pre-sync divergence preserved (branch `flexnetos-divergence-backup-2026-06-13` + git bundle
  in `meta/.archives/`). Do NOT downgrade meta/kasetto.
- **Two locks are DELIBERATELY separate:** engine FNV-1a component lock (`crates/engine/src/lock.rs`,
  `envctl.lock`) vs agent-asset SHA-256 lock (`agent_env::lock`, `agent-env.lock`). Do NOT unify — that's
  the later TASK-0017. `crate::agent::lock` never imports `crate::lock`.
- **Engine is non-printing:** kasetto's `print_*`/`ui.rs`/`process::exit` are DROPPED (FRONTEND-04
  `[≠]`); agent verbs emit `Event::Agent*` + return `Serialize` data; front-end maps failed>0→exit.
- **Preview-default fail-closed:** `apply:false` = ZERO writes; `lock_mode=Locked` = zero-network; MCP
  merge additive never-clobber (broker/repowire/weave survive); never-prune-on-failure (remove_stale
  only when summary.failed==0). These are guardian-tested — keep them.
- **PR workflow (IMPORTANT):** auto-merge + fast CI squash-merges a PR in ~1-2 min, deleting its branch;
  a later push recreates it diverged. WORK ONE PR PER CYCLE OFF FRESH origin/develop. Recover a diverged
  branch via fresh-branch + cherry-pick of ONLY the net-new commits (`git diff origin/develop...branch
  --stat`), never deleting concurrent sibling files.
- **never-discard (owner directive):** stale/orphaned/uncommitted work is INCOMPLETE work to complete +
  carry forward, never `git restore`/delete as drift (the parity harness was carried forward this way).

icm_stored: context-envctl, errors-resolved, decisions-envctl (recall these on resume)

## verify_on_resume (run FIRST to confirm green)
```
cd <fresh worktree off origin/develop>
rtk proxy cargo test -p envctl-agent-env            # expect ~226 + parity_vs_kasetto (12)
rtk proxy cargo test -p envctl-engine               # expect agent_sync integration tests pass
bash ci/gates/no-c.sh && bash ci/gates/shape.sh && bash ci/gates/enable.sh   # all PASS
```
Red → write `.handoff/loop/rust-port/NEEDS-HUMAN` and stop.

resume_command: /harness:rust-port-merge   (or /feature-forge for TASK-0014)
