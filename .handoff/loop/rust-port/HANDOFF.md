# HANDOFF — rust-port-merge + TASK-0014 (kasetto → envctl, Epic C)
closed_utc: 2026-06-14 (session-relay-wrap-up)   branch: develop (08d7086)   worktree: create FRESH off origin/develop
cycle_budget: 3   cycles_total: 17   cycles_this_session: parity all merged + TASK-0014 CLI merged
last_item: TASK-0014 agent CLI (merged #90) + /verify human-render fix (#91 armed)
next_item: **TASK-0014b** — GUI agent panel in `crates/gui/src/main.rs` (egui), thin adapter over the SAME `Engine::agent_*` methods (route via feature-forge). Then optional: close **S-15** (fetch-DI seam / HTTPS endpoint) + the 4 pre-existing `--all-targets`-only lints in `crates/engine/tests/agent_sync_parity.rs`.
orchestrator_phase: Epic C COMPLETE THROUGH THE CLI (parity 101 [x] / 1 [~] / 13 [≠]; CLI front-end shipped; GUI = TASK-0014b)
last_agent: invariant-guardian (TASK-0014 PASS-WITH-NOTES) + /verify fix
gate_status: PASS (agent-env 330 + engine 96 + cli 20 tests; clippy --workspace -D warnings / no-c / shape / enable / fmt green)   pr_url: #89 parity MERGED, #90 CLI MERGED, #91 verify-fix armed

landed_this_session:
  - #89  parity-verifier pass consolidated (verbs + C-12-FIX + residue close) — 101 [x], MERGED
  - #90  cli: envctl agent {sync,add,remove,lock,list,clean} command group — MERGED
  - #91  cli: render agent return in human mode (/verify fix: list/remove showed header only) — armed
decisions_and_dead_ends:
  - Deep PR stacking under fast auto-merge is fragile → CONSOLIDATE >2-3 deep (cherry-pick net-new
    commits onto fresh develop, one PR). Did this for #85-#88 → #89. (ICM decisions-forge-loop.)
  - `--json`-only tests pass the guardian but miss the human surface for return-value verbs (the list
    bug /verify caught). A verb whose data is in the RETURN (not events) needs explicit human rendering
    — now `AgentResult::render_human()` + 2 regression tests.
  - S-15 is honest live-network residue (HTTPS-hardcoded, no fetch DI seam) — do NOT fake; close with a
    seam. Found+fixed one real downgrade (C-12 engine remote-config rejection) en route.
  - GUI deliberately split to TASK-0014b — engine parity guarantees zero engine churn for it.
icm_stored: decisions-forge-loop, context-envctl, errors-resolved, decisions-envctl (recall on resume)
verify_on_resume: |
  git -C <fresh worktree off origin/develop> rev-parse --short origin/develop   # expect 08d7086+ (or #91-merge)
  cargo build -p envctl-engine -p envctl && cargo test -p envctl                 # cli 20 pass
  bash ci/gates/no-c.sh && bash ci/gates/shape.sh && bash ci/gates/enable.sh     # PASS
  target/debug/envctl agent --help                                              # 6 verbs
resume_command: /forge-loop TASK-0014b   (GUI) — or /session-relay-resume from .handoff/loop/rust-port/HANDOFF.md

## ⭐ STATUS: the kasetto→envctl ABSORPTION + PARITY is COMPLETE through the engine AND the CLI.
TASK-0014 shipped the `envctl agent {sync,add,remove,lock,list,clean}` CLI (thin adapter over the
verified engine; #90) + the human-render fix (#91). Remaining front-end work = **TASK-0014b** (GUI panel).
The 13 `[≠]` rows are front-end (envctl owns rendering; verb semantics already `[x]` via the C-* engine
tests). The ONLY unverified parity row is **S-15** (`materialize_source` live main→master HTTP retry) —
CODE matches kasetto `src/source/mod.rs:93-100` line-for-line, but archive URLs are HTTPS-hardcoded with no
fetch DI seam, so a std-only `TcpListener` mock can't reach it offline (honest residue — never faked).

## SESSION-2 (2026-06-14 successor, parity-verifier pass — 3 cycles, all PASS)
First landed session-1's stack (#80/#81/#82 all MERGED). Then:
- **Cycle 1 — leaves** (+6 [x]: XC-03, ST-01/02, P-01/02, CP-01; envctl renames verified). **PR #83**.
- **Cycle 2 — C-* sync engine** (+6 [x]: C-01..C-06) via NEW `crates/engine/tests/agent_sync_parity.rs` (+15). MCP additive/never-clobber + never-prune verified. **PR #84**.
- **Cycle 3 — C-* verbs** (+7 [x]: C-07/08/09/10/11/13/14) via NEW `crates/engine/tests/agent_command_parity.rs` (+22). **C-12 → [~]** (remote-config-reject GAP = **C-12-FIX**, a real no-downgrade engine fix — see parity-ledger top). **PR #85** stacked.
- parity 74→93 [x] (DONE-equiv 106/115). Engine tests 59→96. NEVER stubbed; the C-12 gap recorded, not hidden.

## ⚠️ REMAINING TO FULL DONE (9 rows, all network/engine residue + 1 fix)
2 `[ ]`: **M-22** (resolve_scope file-read fallback, engine path), **S-15** (main→master retry, network).
7 `[~]`: **S-07/S-12/S-13** (pub(crate)/network), **CFG-03** (remote http arm), **C-12** (engine remote-reject GAP).
Plan: ONE Engine/network integration cycle (exercise `Engine::agent_sync` materialize/download end-to-end)
closes S-07/S-15/CFG-03 + M-22 at once; **C-12-FIX** (make `resolve_local_config_path` return Result +
reject remote, edit.rs:352 + call sites :53,:151) + a `pub` test seam for S-12/S-13 closes the rest.
THEN **TASK-0014** = the 13 `[≠]` front-end (CLI `envctl agent {sync,add,remove,lock,list,clean}` + GUI;
thin adapters over the already-verified engine methods).

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
