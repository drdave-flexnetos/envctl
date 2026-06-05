---
name: auto-provision
description: "Fully UNATTENDED, self-restarting provisioning of the whole workstation — the external Ralph loop that gives a FRESH context every cycle (the `/new` effect) and hands off to itself until the box is fully installed. ALWAYS use when asked to: 'run the install unattended / overnight / set-it-and-forget-it', 'keep restarting with fresh context until installed', 'auto-provision the box', 'self-handoff the install', or 'cycle install and reset until done'. Wraps `env-install-loop` with a bundled runner that spawns a fresh `claude -p` per iteration. Do NOT use for a normal in-session loop (use `env-install-loop`), a single component (use `env-toolchain-install`), or code features (use `forge-loop`)."
---

# Auto-Provision (self-restarting fresh-context Ralph loop)

You drive the workstation to fully-installed **unattended**, restarting yourself with a **clean
context every cycle** instead of running one long, rotting session. This is the genuine *Ralph*
pattern and the honest realization of "run `/new` then `/env-install-loop` and hand off to
yourself": the agent **cannot type `/new`** (it's a REPL command, not a tool), but a **new
process** is a clean context — so an external shell loop spawns a fresh `claude -p` each iteration.
Each fresh agent does one cycle-budget of work via **`env-install-loop`**, commits, writes a
durable handoff, and exits; the loop respawns the next clean agent. Truth lives on disk (backlog +
checkpoints + commits), so every restart resumes cold with zero loss.

## When this vs. env-install-loop
- **`env-install-loop`** = the in-session loop (self-paces with ScheduleWakeup, hands off via
  `session-relay`). Good when you're watching and sessions are short.
- **`auto-provision`** = the *outer* wrapper that restarts the whole session repeatedly with fresh
  context, for very long / fully-unattended provisioning. Each spawned process runs
  `env-install-loop`. Use this when you want to set it running and walk away.

## The runner
Bundled at `scripts/ralph-provision.sh`. It is a bounded `while` loop that, each iteration:
1. checks sentinels (kill switch / terminal states) under `_workspace/`,
2. spawns one fresh `claude -p "<resume prompt>"` (clean context) in the worktree,
3. respawns until a terminal sentinel or the max-iterations backstop.

The spawned agent writes **exactly one** sentinel per run, which the runner reads:

| Sentinel (`_workspace/…`) | Meaning | Runner action |
|---------------------------|---------|---------------|
| `HANDOFF.md` | more work remains | spawn the next fresh process |
| `DONE` | provisioned + verified | exit 0 |
| `NEEDS-HUMAN` | sudo/reboot/hardware wall | halt for you (reason inside) |
| `STOP` | kill switch (you `touch` it) | halt |

## Launch it
Run from the provisioning worktree (set one up first — never on dirty `master`):

```bash
# SAFE / attended (default): destructive --apply & reset are REFUSED (headless agents can't answer
# permission prompts), so this is a dry, discovery+plan pass that commits non-destructive progress.
bash .claude/skills/auto-provision/scripts/ralph-provision.sh

# UNATTENDED APPLY: actually modify THIS workstation with no prompts. Opt in deliberately.
RALPH_APPLY=1 bash .claude/skills/auto-provision/scripts/ralph-provision.sh
```

Tunables (env): `RALPH_WORKTREE` (default cwd), `RALPH_BUDGET` (cycles/process, default 3),
`RALPH_MAX_ITERS` (restart backstop, default 50), `RALPH_SLEEP` (default 5s), `RALPH_MODEL`
(default `opus`), `RALPH_RESEARCH` (run the component-research/audit pass, default `1`). Kill switch
any time: `touch _workspace/STOP`.

## Research is inherited from env-install-loop
Each spawned agent also runs `env-install-loop`'s **Research** pass: it deep-probes every declared
component (past `detect`/`verify`) and auto-appends classified upgrade/hardening items to the
backlog — `harden:`/`fix:`/`upgrade:` (loop-fixable, worked in later cycles) and `feature:` (routed
to `feature-forge`, surfaced not built). Read-only and evidence-based; set `RALPH_RESEARCH=0` to skip
it for a pure install pass. `feature:` items are committed to the backlog (audit trail for the human)
and do **not** change the runner's terminal contract: `DONE` still means Tier-1 *provisioned*
(doctor green + gates), with routed upgrades reported alongside.

## Install ↔ reset remediation (the "cycle install and reset" the loop performs)
Per backlog item, the spawned `env-install-loop` walks a remediation ladder, fail-closed at every
rung:
1. **install** — `envctl install <id>` (dry-run → `--apply`). Idempotent; a healthy component is a
   no-op.
2. **auto-fix** — `envctl auto-fix <id>` for a detected-but-unhealthy component.
3. **reset → reinstall** — if it's wedged and auto-fix won't clear it, `envctl reset <id> --apply`
   (remove + unwire; destructive, so dry-run first and respect the `UuidResolves`/`NotLiveDevice`/
   `NotMounted` guards) then `envctl install <id> --apply`, then verify.
4. **verify** — re-run the component verify + `doctor`; confirm on PATH and env vars set in a
   **fresh** shell. Only then tick the backlog.
A rung that needs privilege/reboot/hardware → write `NEEDS-HUMAN` and stop (don't force).

## DONE — the runner exits 0 only when the spawned agent proves ALL:
`envctl doctor` green · `auto-detect` all detected+healthy, zero drift · `lock --check` +
`kasetto sync --locked` clean · `cargo build -p envctl-engine -p envctl` + `no-c`/`shape`/`enable`
gates pass. The agent writes `_workspace/DONE` with this evidence.

## Safety (this modifies a live workstation — non-negotiable)
- **Safe by default.** Without `RALPH_APPLY=1`, headless runs cannot perform destructive applies —
  use a safe pass first to see the plan/backlog the loop builds before letting it act.
- **Bounded.** `RALPH_MAX_ITERS` backstops runaway restarts; `_workspace/STOP` is an always-checked
  kill switch.
- **Fail-closed + rust-native.** Never weaken a guard; treat foreign-language drift as a defect to
  fix. Destructive `reset` is dry-run-first and guard-gated.
- **Human walls stop the loop**, they don't get forced past — `NEEDS-HUMAN` halts the runner with
  the reason so you can run the privileged/interactive step (e.g. via `! <command>`) and relaunch.

## Test Scenarios
**Happy path:** `RALPH_APPLY=1 bash …/ralph-provision.sh` in a clean worktree. Iter 1: fresh agent
DISCOVERs gaps, installs 3 components, writes `HANDOFF.md`, exits. Iter 2: fresh agent resumes from
the committed checkpoint, resets+reinstalls one wedged component, installs the rest, re-runs doctor
→ green, lock+kasetto+gates clean → writes `_workspace/DONE`. Runner exits 0.

**Error path:** A component needs a reboot after a kernel-module install. The spawned agent runs the
dry-run, can't complete unattended, writes `_workspace/NEEDS-HUMAN: reboot required after nvidia-open`
and stops. The runner halts (exit 2) and surfaces the reason instead of spinning. You reboot, then
relaunch the runner — it resumes from the committed checkpoint.
