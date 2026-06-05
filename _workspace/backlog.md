# Dashboard follow-up backlog — successor auto-loop

Source of truth for the post-feature loop. Built from the Feature Forge synthesis +
guardian notes. Ordered: merge-gated wire-live first, then follow-ups.

Legend: `- [ ]` todo · `- [x]` done · `- [!]` blocked (reason).

## Gate (human/review)
- [!] MERGE PRs — blocked on review. envctl #23 -> develop; meta #7 -> main.
      Auto-merge intentionally NOT enabled. The successor MUST confirm both merged
      (`gh pr view 23 --repo FlexNetOS/envctl`, `gh pr view 7 --repo FlexNetOS/meta`)
      before wiring live, OR work from the merged develop/main once available.

## 1. Wire it live (after merge)
- [ ] `envctl install dashboard` — deploys launcher to ~/.local/bin + the zellij KDL
      layout to ~/.config/yazelix/configs/zellij/layouts/mission-control.kdl.
      (Or `envctl dashboard --deploy --apply` for the layout alone.) Fail-closed/dry-run
      by default — install applies.
- [ ] Verify: `envctl doctor` / `envctl auto-detect` shows the `dashboard` component
      detected+healthy; layout file present; `envctl-dashboard-pane` on PATH.
- [ ] Put `meta-dashboard` (the plugin binary) on PATH so `meta dashboard` resolves
      (build meta_dashboard_cli + install to ~/.local/bin, or wire into the component).
- [ ] Smoke: open yazelix with the mission-control layout; confirm tabs/panes render and
      each pane launches an idle claude session on weave + repowire.

## 2. Follow-ups (feature)
- [ ] Escalate panes from idle agents to autonomous loops via ENVCTL_DASHBOARD_PANE_CMD
      (forge-loop / env-install-loop per repo) — opt-in, document the per-pane override.
- [ ] Refine grouping of UNTAGGED repos: agent, claude-plugins, meta-plugins currently
      fall into the synthetic "meta-core" tab. This is a `.meta.yaml` tag edit (add tags
      so they group correctly) — no code change, no-drift holds.

## Dropped (handled elsewhere)
- ~~Broker unification into the weave bus~~ — weave is already upgrading to merge
  weave+repowire+broker into one bus. Do NOT implement here.

## Audit trail (committed)
- _workspace/01_architect_plan.md — design + the both-surfaces/no-drift resolution
- _workspace/02_implementer_log.md — Pass 1 + Pass 2 implementation logs
- _workspace/03_guardian_report.md — Pass 1 + Pass 2 independent verification
