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
and the **`env-toolchain-install`** skill. Beyond closing install gaps it also **researches** each
component and auto-appends upgrade/hardening items to the backlog (see *Research* below) — but it
still does **not** build features itself: genuine feature work it discovers is *routed* to
`feature-forge`/`forge-loop`, not implemented here.

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

## Durable state (the loop's memory) — under the worktree's `.handoff/loop/`
- **`.handoff/loop/backlog.md`** — the source of truth. Ordered checklist, one item per gap:
  `- [ ] <id>: <what's missing/broken>` → `- [x]` healthy, `- [!] blocked: <reason>` if stuck.
  Research-discovered items live in their own section and carry a class prefix + owner:
  `harden:` / `fix:` / `upgrade:` (loop-fixable, declarative) and `feature:` (route to
  `feature-forge`) — see *Research*.
- **`.handoff/loop/loop_state.md`** — ledger: `cycle_budget` (default 3), `cycles_this_session`,
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
4. Write `.handoff/loop/backlog.md` (one item per gap, most foundational first — e.g. apt-base before
   things that depend on it; follow the dependency graph, `envctl graph`) and seed `loop_state.md`.
5. Kick off the **Research** pass (below) to append upgrade/hardening items. Install gaps are worked
   first (foundational); research items after.

## Research each component — deep audit that auto-appends upgrades/hardening to the backlog
`detect`/`verify` only prove **presence**. A provisioned box is "installed, *current*, *deeply*
verified, and correctly *wired*" — so beyond closing install gaps, research every declared component
the way you'd investigate a suspicious one, and **auto-append what you find to the backlog**. (This
is exactly how the pytorch gaps surfaced: `import torch` reported "healthy" while the gate never
checked `cuda.is_available()`, torchvision, the `sm_120` arch list, the verify-hook side-effect, or
the toolkit-13.3-vs-driver-13.2 skew.)

**Engine — fan out, keep only conclusions.** Offload per-component probing to subagents (the
`deep-research` skill for currency/advisories; `Explore`/general-purpose for the code/manifest audit)
so it doesn't burn the loop's context. Findings MUST be **evidence-based and sourced** (a real probe
output or an upstream cite) and **adversarially sanity-checked** before they enter the backlog — a
hallucinated "upgrade available" wastes a cycle. This is read-only and safe.

**Per component, probe past the gate:**
- **Exercise real function end-to-end** (not existence): run the actual op across the boundary
  (import + run a kernel, round-trip a request, compile + execute) and confirm it truly works — the
  cross-boundary check the `verify` hook skips.
- **Gate quality:** would `detect`/`verify` pass a CPU-only / broken / half-wired install? Too
  shallow → `harden:`.
- **Version currency:** installed vs upstream latest stable; security advisories; deprecated install
  source. Newer-stable-available → `upgrade:`.
- **Cross-component skew:** version/ABI/compat mismatch between related components (toolkit vs driver,
  runtime vs build, pinned nightly vs MSRV).
- **Hook hygiene:** side-effects / non-idempotency / weakened guards in install/verify (e.g. a
  `verify` that mutates state) → `fix:`.
- **Wiring reach:** env/PATH present where it must be — interactive **and** the non-interactive
  shells that scripts/systemd use.

**Append to a dedicated backlog section, classified by owner so routing is automatic:**
```
## Upgrades & hardening (research-discovered — evidence + owner)
- [ ] harden:pytorch-venv — detect/verify only `import torch`; never asserts cuda.is_available()
      or torchvision. Evidence: <probe>. Owner: loop (deepen the manifest gate).
- [ ] upgrade:<id> — installed X; upstream stable Y (cite). Owner: loop (idempotent install --apply).
- [ ] fix:<id> — <latent hook/manifest bug + evidence>. Owner: loop (manifest fix).
- [ ] feature:<id> — <needs engine/CLI/new-component work>. Owner: route:feature-forge (or human).
```
`harden:`/`fix:`/`upgrade:` are **loop-fixable** — declarative manifest/install changes worked
through the normal iteration body (dry-run → apply → re-verify → re-lock → commit), same fail-closed
ladder + guards as installs. `feature:` items the loop **adds and surfaces only** — it routes them to
`feature-forge`/`forge-loop` (or `route:human` for judgment calls) and does **not** build them
itself (charter: install/repair, not the code crew). They do **not** block provisioning DONE.

**Re-run research** opportunistically — on resume and again at the DONE-check — so newly-released
upgrades and advisories get caught. Bound cost: one fan-out research pass counts as **one cycle**.
Arg: `/env-install-loop research=on|off|only` (default `on`; `only` = audit pass, no installs).

## One iteration (the loop body)
1. **Read state.** backlog + loop_state; confirm worktree clean + on the loop branch.
2. **Stop checks (in order):** backlog has no `- [ ]` → **DONE** (see criteria); 
   `cycles_this_session >= cycle_budget` → **HAND OFF** (invoke `session-relay`, then stop).
3. **Pick** the top unchecked item (respect dependency order — install prerequisites first). Order:
   install/repair gaps → loop-fixable research items (`harden:`/`fix:`/`upgrade:`). For a `feature:`
   item, don't implement it — route it to `feature-forge` (or leave it for the human) and move on.
4. **Install / repair it the declared, idempotent way** — walk this remediation ladder, fail-closed
   at every rung, dry-run before any `--apply`:
   - **install** — `cargo run -p envctl -- install <id>` (idempotent; a healthy component is a no-op).
   - **auto-fix** — `auto-fix <id>` for a detected-but-unhealthy component.
   - **reset → reinstall** — if it's wedged and auto-fix won't clear it, `reset <id> --apply` (remove
     + unwire; destructive, so preview first and respect the `UuidResolves`/`NotLiveDevice`/
     `NotMounted` guards) then `install <id> --apply`. This is the "cycle install and reset" path.
   - Or the component's lifecycle hooks per `env-toolchain-install`. Never force past a refusing guard.
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

### External-runner (auto-provision) mode — write a sentinel, don't self-pace
When launched by the **`auto-provision`** runner (a fresh `claude -p` process per cycle — the
prompt will say so), do **not** ScheduleWakeup. Run up to one cycle-budget of work, commit each
cycle, then write **exactly one** sentinel under `.handoff/loop/` and exit so the runner decides
whether to respawn a fresh-context process:
- everything verified DONE → `.handoff/loop/DONE` (with the DONE-criteria evidence);
- privilege/reboot/hardware wall → `.handoff/loop/NEEDS-HUMAN` (with the reason);
- more work remains → write `.handoff/loop/HANDOFF.md` (via `session-relay`/`continuity-steward`) and exit.
Also honor `.handoff/loop/STOP` as a kill switch: if present, stop immediately.

## Cycle budget (handoff trigger)
Per-session budget is **cycles-only** (no token-meter guessing — there is no live meter): default
**3** completed cycles per session unless the user sets `/env-install-loop budget=N …`. At the
budget, invoke **`session-relay`** (which spawns `continuity-steward` to write
`.handoff/loop/HANDOFF.md`, commits it, broadcasts a weave heartbeat, and best-effort schedules a
successor), then stop. The successor resets `cycles_this_session` and continues from the backlog.

## Resume (entering mid-loop from a handoff)
If a `.handoff/loop/HANDOFF.md` exists (or the prompt says "resume"): follow `session-relay`'s RESUME
protocol — the **committed `HANDOFF.md` is the authoritative signal** (not the weave inbox; cron is
best-effort). Read it, run its verify-on-resume baseline, broadcast `env-install:resumed`, reset
`cycles_this_session`, then continue the iteration body at the backlog's current item.

## DONE — two tiers (stop when BOTH hold; report each with evidence)
**Tier 1 — Provisioned** (the loop's core goal):
- `envctl doctor` fully **green**; `auto-detect` shows every declared component **detected +
  healthy**; **zero drift**.
- All toolchains present at correct versions; **PATH + env vars verified in a fresh shell**.
- `cargo run -p envctl -- lock --check` clean **and** `kasetto sync --locked` clean (reproducible).
- `cargo build -p envctl-engine -p envctl` + the 3 CI gates (`no-c`/`shape`/`enable`) pass.
- Any `- [!]` blocked items are surfaced for the human with their reason.

**Tier 2 — Upgrades resolved/routed:**
- Every loop-fixable `harden:`/`fix:`/`upgrade:` research item is `- [x]` (worked + re-locked + the
  Tier-1 gates still pass), OR explicitly `- [!]` with a reason.
- Remaining `feature:`/`route:*` items are **surfaced** — handed to `feature-forge`/`forge-loop` or
  listed for the human. These do **NOT** block Tier-1; report them so nothing is silently dropped.

Stop only when Tier 1 holds AND no loop-fixable research item is left undone. A final research pass
returning nothing new is the signal the upgrade backlog is drained.

## Guardrails
- **Idempotent + declarative + rust-native.** Treat a non-Rust source/package file that appears as
  drift to fix, not accept (see CLAUDE.md). Re-running a healthy component must be a no-op.
- **Preview destructive ops** before `--apply`; never weaken a fail-closed guard to make a step pass.
- **Privilege / interactive / reboot wall:** if a step needs sudo, interactive auth, a reboot, or
  hardware you can't drive, **STOP and ask the human** (suggest they run it via `! <command>`)
  rather than spinning the loop on an item you cannot complete. Mark it `- [!] needs-human` and move
  on if other items remain.
- Keep `.handoff/loop/` as the audit trail; commit every cycle so a fresh session resumes cold.

## Stop conditions (end the loop — no re-fire)
DONE (both tiers met — provisioned + upgrades resolved/routed) · cycle budget reached (hand off, then
stop) · a hard blocker the loop can't route around (dirty worktree, repeated failure on the same item
with no others left) · user interrupt.

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

**Research path:** Tier-1 is already green. The research pass fans out a subagent per component;
the pytorch probe finds `import torch` "healthy" hides that the gate never checks CUDA/torchvision
and that nvcc is 13.3 while the driver caps at 13.2. It appends `harden:pytorch-venv` (deepen the
verify to assert `cuda.is_available()` + torchvision) and `fix:gpu-verify-scripts` (verify hook
deletes the autostart — make it non-mutating), both Owner: loop, plus `feature:cuda-skew-doctor`
(a new doctor check for toolkit/driver skew) Owner: route:feature-forge. Next cycles work the two
loop-fixable items (manifest change → re-lock → gates still green → commit); the `feature:` item is
handed to `feature-forge` and surfaced. A final research pass returns nothing new → Tier-2 drained →
DONE reports both tiers + the routed feature item.
