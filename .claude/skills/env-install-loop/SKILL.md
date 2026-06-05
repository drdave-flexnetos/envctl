---
name: env-install-loop
description: "Drive the WHOLE workstation to a fully-installed, healthy, drift-free state — looping across sessions until done. ALWAYS use when asked to: 'install everything / all the toolchains', 'set up / provision the box', 'fix all toolchains, PATH, and env vars', 'make doctor green', 'loop until installed', 'finish the environment setup unattended', or 'resume the env install'. Discovers gaps from `envctl doctor`/`auto-detect`, works a durable backlog one component per cycle, verifies PATH/env/toolchains, checkpoints, and hands off to a fresh session at the cycle budget. Do NOT use for a SINGLE component install (use env-toolchain-install directly), a drift/lock check only (use env-stabilize), or building Rust code features (use feature-forge / forge-loop)."
---

# Environment Install Loop (Ralph, for provisioning)

You drive the **environment** — not Rust features — to a fully-installed, healthy, drift-free
state, looping until everything is correct. Same Ralph shape as `forge-loop`: durable backlog on
disk, one item per iteration, checkpoint, re-fire; hand off to a fresh session at the cycle budget
so the run survives context rot + token burn. The difference is the *work*: each cycle installs or
repairs one declared component (toolchain / wiring / PATH / env var) using **envctl's own verbs**
and the **`env-toolchain-install`** skill — it does **not** spawn the Feature Forge code crew.

## Why this shape
envctl exists to bring the box to a *declared* state idempotently. A long single session that
installs everything rots and gets expensive; a loop that keeps its truth in durable files
(backlog + checkpoints + commits) can be carried across many short sessions. So **write state down
every iteration; never hold the install plan only in your head.**

## Skills & verbs this loop drives
- **`env-toolchain-install`** — the per-component *how* (detect→install→verify→fix→remove lifecycle,
  idempotency). Read it; it owns the install discipline.
- **`env-stabilize`** — drift/doctor/lock discipline; how to prove the env is reproducible.
- **envctl verbs** — `doctor` (health), `auto-detect [--json]` (inventory + drift), `install <id>`,
  `auto-fix <id>`, `lock --check`. Destructive verbs are **dry-run by default**; act only with
  `--apply`/`--build`.

## Durable state (the loop's memory) — under the worktree's `_workspace/`
- **`_workspace/backlog.md`** — the source of truth. Ordered checklist, one item per gap:
  `- [ ] <id>: <what's missing/broken>` → `- [x]` healthy, `- [!] blocked: <reason>` if stuck.
- **`_workspace/loop_state.md`** — ledger: `cycle_budget` (default 3), `cycles_this_session`,
  `cycles_total`, `last_item`, `status`, `session_started` (UTC — you supply it; scripts can't read
  the clock).

## DISCOVER first (build the backlog from REAL state — never hallucinate)
Before looping, in the worktree:
1. `cargo run -p envctl -- doctor` and `cargo run -p envctl -- auto-detect --json` (the `EnvReport` +
   drift). These tell you what's missing / detected-but-unhealthy / drifted.
2. Enumerate declared components (`manifest/*.toml`) and diff against detect/drift.
3. Check each toolchain present at the right version (Rust per `rust-toolchain.toml`, bun/node, the
   CUDA/GPU stack + driver, ai-clis, nix-yazelix, secretd) and that required **PATH entries and env
   vars** are actually present/exported (inspect the components' `wiring`).
4. Write `_workspace/backlog.md` (one item per gap, most foundational first — e.g. apt-base before
   things that depend on it; follow the dependency graph, `envctl graph`) and seed `loop_state.md`.

## One iteration (the loop body)
1. **Read state.** backlog + loop_state; confirm worktree clean + on the loop branch.
2. **Stop checks (in order):** backlog has no `- [ ]` → **DONE** (see criteria); 
   `cycles_this_session >= cycle_budget` → **HAND OFF** (invoke `session-relay`, then stop).
3. **Pick** the top unchecked item (respect dependency order — install prerequisites first).
4. **Install / repair it the declared, idempotent way:**
   - Prefer `cargo run -p envctl -- install <id>` or `auto-fix <id>` — **dry-run first** to preview,
     then `--apply`/`--build` to act. Or the component's lifecycle hooks per `env-toolchain-install`.
   - Destructive ops are **fail-closed + dry-run by default**; never force past a refusing guard.
5. **VERIFY (cross-boundary, not existence-only):** re-run the component's `verify` / `envctl doctor`;
   confirm the binary is actually **on PATH** and its **env vars/paths are set in a fresh shell**
   (source the rc or open a new shell — a tool installed but not wired is not done). Re-run
   `auto-detect` to confirm the component now detects healthy with no drift.
6. **Write state back:** tick `- [x]` (or `- [!] blocked: <reason>` and move on — don't thrash a
   stuck item), bump `cycles_this_session`/`cycles_total`, update `last_item`/`status`, append a
   one-line note. **Commit** (`git commit`, area-prefixed subject e.g. `env:`/`gpu:`/`nix:`/`docs:`).
7. **Re-fire** to continue (see Self-pacing).

## Self-pacing
Default **dynamic /loop**: `ScheduleWakeup` to re-enter this skill for the next iteration, passing
the same `/env-install-loop …` prompt verbatim. Choose the delay by what you're waiting on (a long
apt/CUDA install → a longer poll; back-to-back light steps → a short warm-cache delay ≤270s). When
you HAND OFF or finish, **omit** the ScheduleWakeup to end the loop. A cycle counts only when an
install/repair attempt **completes** (healthy or blocked).

## Cycle budget (handoff trigger)
Per-session budget is **cycles-only** (no token-meter guessing — there is no live meter): default
**3** completed cycles per session unless the user sets `/env-install-loop budget=N …`. At the
budget, invoke **`session-relay`** (which spawns `continuity-steward` to write
`_workspace/HANDOFF.md`, commits it, broadcasts a weave heartbeat, and best-effort schedules a
successor), then stop. The successor resets `cycles_this_session` and continues from the backlog.

## Resume (entering mid-loop from a handoff)
If a `_workspace/HANDOFF.md` exists (or the prompt says "resume"): follow `session-relay`'s RESUME
protocol — the **committed `HANDOFF.md` is the authoritative signal** (not the weave inbox; cron is
best-effort). Read it, run its verify-on-resume baseline, broadcast `env-install:resumed`, reset
`cycles_this_session`, then continue the iteration body at the backlog's current item.

## DONE — stop the loop only when ALL hold (report with evidence)
- `envctl doctor` fully **green**; `auto-detect` shows every declared component **detected +
  healthy**; **zero drift**.
- All toolchains present at correct versions; **PATH + env vars verified in a fresh shell**.
- `cargo run -p envctl -- lock --check` clean **and** `kasetto sync --locked` clean (reproducible).
- `cargo build -p envctl-engine -p envctl` + the 3 CI gates (`no-c`/`shape`/`enable`) pass.
- Any `- [!]` blocked items are surfaced for the human with their reason.

## Guardrails
- **Idempotent + declarative + rust-native.** Treat a non-Rust source/package file that appears as
  drift to fix, not accept (see CLAUDE.md). Re-running a healthy component must be a no-op.
- **Preview destructive ops** before `--apply`; never weaken a fail-closed guard to make a step pass.
- **Privilege / interactive / reboot wall:** if a step needs sudo, interactive auth, a reboot, or
  hardware you can't drive, **STOP and ask the human** (suggest they run it via `! <command>`)
  rather than spinning the loop on an item you cannot complete. Mark it `- [!] needs-human` and move
  on if other items remain.
- Keep `_workspace/` as the audit trail; commit every cycle so a fresh session resumes cold.

## Stop conditions (end the loop — no re-fire)
DONE (all criteria met) · cycle budget reached (hand off, then stop) · a hard blocker the loop can't
route around (dirty worktree, repeated failure on the same item with no others left) · user interrupt.

## Test Scenarios
**Happy path:** `/env-install-loop budget=3` on a box where doctor reports nix-yazelix missing +
two PATH entries unwired. DISCOVER writes a 3-item backlog. Cycles 1-3 each install/wire one item
(dry-run → `--apply`), verify it on PATH in a fresh shell, tick the backlog, commit. Budget hits →
`session-relay` writes+commits `HANDOFF.md`, broadcasts `env-install:handoff`, stops. Successor
resumes from the committed checkpoint, finds the backlog empty after re-running doctor → DONE:
doctor green, lock + kasetto clean, gates pass; opens a PR.

**Error path:** an item needs a kernel module + reboot. The loop runs the dry-run, sees it can't
complete unattended, marks `- [!] needs-human: requires reboot after nvidia-open install`, commits,
and continues to the next item. At DONE-check it reports the blocked item for the human instead of
claiming a green environment it can't prove.
