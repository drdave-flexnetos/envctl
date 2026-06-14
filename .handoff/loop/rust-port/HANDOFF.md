# HANDOFF — rust-port-merge (kasetto → envctl, Epic C absorption)
closed_utc: 2026-06-14 (parity session, budget 3/3 reached)   branch: develop   worktree: create FRESH off origin/develop
cycle_budget: 3   cycles_total: 10   cycles_this_session: 3 (parity-verifier pass — RESET to 0 on resume)
last_item: PARITY cluster fsops+config_edit (+14 [x])   next_item: PARITY cluster C-* command business logic, THEN one Engine::agent_sync integration cycle (closes the 6 [~] residue), THEN TASK-0014 (CLI/GUI front-end, the 13 [≠])
orchestrator_phase: PARITY-VERIFIER PASS (merge 0 [ ] to-merge; parity 74 [x] / 6 [~] / 22 [ ] / 13 [≠] = DONE-equiv 87/115)
gate_status: PASS every cycle (agent-env 304 tests; no-c/fmt green)

## THIS SESSION (2026-06-14, parity-verifier pass — 3 cycles, all PASS)
- **Cycle 1 — source-resolver** (+11 [x]: S-09/10/11/14/16/17/18/19/20/21,XC-04). **PR #80 MERGED** to develop (3228971).
- **Cycle 2 — model + 21-preset table + config-loader + SHA-256 lock** (+27 [x]: M-01/03/04/06/07/09-14/17/19/20/21/23-26, CFG-01/02, L-01-06). **PR #81** (auto-merge armed; stacked-then-rebased onto develop after #80).
- **Cycle 3 — fsops + config_edit** (+14 [x]: F-03..F-10, FE-01..06; 0 BLOCKED). **PR #82** STACKED on #81.
- parity 33→74 [x]. New parity vectors: suite 12→75 fns. NEVER stubbed; residue recorded honestly.

## ⚠️ PR-STACK — land in order, rebase each onto the prior
#80 MERGED. **#81 → #82 are a stack.** When #81 squash-merges to develop, rebase #82:
```
cd <cycle-3 worktree> && git fetch origin
git rebase --onto origin/develop <#81-tip-sha> task-0012-parity-pass-3   # drop the merged cycle-2 commit
git push --force-with-lease && gh pr merge 82 --auto --squash
```
(Same pattern already used to land #81 onto #80 — see commit history. The conflict is only the
shared loop_state.md / parity-ledger.md section lines; the test-file appends are non-overlapping.)

## 6 [~] residue (NOT faked — close together in ONE Engine integration cycle)
S-07 (tar-slip guard), S-12 (auth_env_inline_help), S-13 (http_fetch_auth_hint), S-15 (main→master
retry), CFG-03 (remote http arm), + M-24 `State`/L-03 `list_installed_*` design-folds. All are
`pub(crate)`/network-only/engine-folded — unreachable via the offline cross-crate public API. Close by
exercising `Engine::agent_sync` end-to-end (it drives materialize→download→merge→lock) in a
`crates/engine/tests/` integration test, OR add a `pub` test seam. Do NOT fake a passing vector.

**Resume with:** `/forge-loop resume the /rust-port-merge` (or `/harness:rust-port-merge`). State lives in
`.handoff/loop/rust-port/` (namespaced — NOT the flat `.handoff/loop/`, the forge-loop's). On resume:
first land the #81→#82 stack, then RESET cycles_this_session to 0 and pick the next cluster (C-*).

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
