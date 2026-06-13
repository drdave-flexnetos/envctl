# Dashboard follow-up backlog — successor auto-loop

> **STATUS: COMPLETE (2026-06-05).** All Gate / Wire-live / Follow-up items resolved by
> intervening work (merges of #23+#24+#7, two follow-up commits, and a live install).
> Only the interactive smoke test remains — HUMAN-ONLY, an unattended agent can't open the GUI.
> No further forge-loop cycle is warranted on this backlog. (Separate open items — the
> env-install-loop's `grit-harness-parallel` and the `node-via-bun` manifest follow-up below —
> belong to OTHER tracks and are surfaced for a human, not auto-run by this loop.)

Source of truth for the post-feature loop. Built from the Feature Forge synthesis +
guardian notes. Ordered: merge-gated wire-live first, then follow-ups.

Legend: `- [ ]` todo · `- [x]` done · `- [!]` blocked (reason).

## Gate (human/review) — CLEARED 2026-06-05
- [x] MERGE PRs — BOTH MERGED. envctl #23 → develop, then develop → master via #24
      (master @ fcf3d0c). meta #7 → main (mergedAt 2026-06-05T21:35:10Z, MERGED).
      Dashboard code is on master: dashboard.rs + manifest/dashboard.toml + launcher.

## 1. Wire it live (after merge) — DONE (live deploy verified)
- [x] `envctl install dashboard` — DEPLOYED. Launcher at ~/.local/bin/envctl-dashboard-pane;
      zellij layout at ~/.config/yazelix/configs/zellij/layouts/mission-control.kdl (8.3K).
- [x] Verify — `envctl auto-detect` shows `dashboard ✓ healthy wired`; layout present;
      `envctl-dashboard-pane` on PATH. lock --check clean @ 49 components; gates green.
- [x] `meta-dashboard` on PATH — present at ~/.local/bin/meta-dashboard (`meta dashboard` resolves).
- [!] Smoke (open yazelix, eyeball panes) — HUMAN-ONLY. Interactive/visual; an unattended
      agent cannot open the GUI and confirm panes render. Left for a human to eyeball.

## 2. Follow-ups (feature) — DONE (already shipped by intervening commits)
- [x] ENVCTL_DASHBOARD_PANE_CMD escalation — IMPLEMENTED. Launcher honors the per-pane
      override (`exec bash -c "$ENVCTL_DASHBOARD_PANE_CMD"`), documented inline in the script.
- [x] Refine grouping of UNTAGGED repos — DONE in meta `524af3d`: .meta.yaml now tags
      agent→[ai], claude-plugins→[ai], meta-plugins→[tools]. Config-only, no-drift held.

## Dropped (handled elsewhere)
- ~~Broker unification into the weave bus~~ — weave is already upgrading to merge
  weave+repowire+broker into one bus. Do NOT implement here.

## Audit trail (committed)
- _workspace/01_architect_plan.md — design + the both-surfaces/no-drift resolution
- _workspace/02_implementer_log.md — Pass 1 + Pass 2 implementation logs
- _workspace/03_guardian_report.md — Pass 1 + Pass 2 independent verification
# env-install-loop backlog — 2026-06-05T02:52Z

Source of truth for the provisioning loop. Built from REAL state:
`envctl doctor` + `auto-detect --json` (17 `missing`/medium drift items; zero
config/health drift; every *detected* component healthy). Ordered most-foundational
first per the dependency graph (`requires` edges).

Legend: `- [ ]` todo · `- [x]` installed+verified · `- [!]` blocked (reason).

## Loop-installable (user-space, prerequisites met, no sudo)
- [!] node-via-bun — TABLED (design item; research-resolved 2026-06-05). The box is FUNCTIONALLY
      CORRECT as-is; this is cosmetic detect-drift only. Findings (research agent, sourced):
      * Bun's `node` shim CANNOT do `node --version` BY DESIGN (not a version regression) — it
        only runs `node <script>`. So the component's verify hook can never pass on bun 1.3.x/1.4.
      * n8n CANNOT run on Bun at all (n8n's isolated-vm needs V8; Bun uses JSC). n8n requires
        REAL Node 20–24; the installed real node v22.22.3 (~/.local/bin/node) is correct & in-range.
      * Recommendation (a): keep real node for n8n + bun as `bun`/`bun --bun`; node-via-bun is
        inapplicable on an n8n box. NO PATH change (real node first is exactly what n8n needs).
      * Symlink ~/.bun/bin/node->bun left in place: it's INERT (real node precedes it) and
        `envctl reset node-via-bun` is REFUSED by the fail-closed guard — group-ai-clis declares a
        live reverse-dep on it; removing would cascade the healthy ai-clis stack. Not forced.
      HUMAN/FEATURE-FORGE follow-up (NOT a loop action): make the manifest mark node-via-bun
      not-applicable when a real Node in n8n's range is present (or add a `node-real` component +
      drop the group-ai-clis -> node-via-bun edge), so doctor goes truthfully green.
- [x] env-ctl — INSTALLED + verified: secretd/secretctl on PATH, `secretd --self-check` OK,
      systemd user unit `env-ctl.service` enabled, ~/.bashrc SECRETCTL_SOCK wired, detect
      healthy, off drift. Required fixing a manifest bug first: the MSRV gate in env-ctl.toml
      had reversed `sort -V -C` operands and rejected every cargo >= 1.80 (FATAL on a healthy
      1.96 toolchain). Fixed (put 1.80.0 first). Built with ENV_CTL_REPO=this worktree.
- [x] pytorch-venv — INSTALLED + verified after `sudo apt install python3.14-venv`:
      torch 2.12.0+cu132, torch.cuda.is_available()=True, sees 2 devices (both RTX 5090).
      detect healthy, off drift.

## Blocked on privilege wall — needs-human (sudo NOT pre-authorized; doctor: sudo X)
These cannot be completed unattended. A human must `sudo -v` in a real terminal (or run
the privileged installs via `! envctl install <id>`), then re-run the loop to resume.

apt base (direct `needs_sudo = true`, `apt-get`) — DONE (user authorized sudo):
- [x] ghostty — installed (/usr/bin/ghostty), detect healthy
- [x] podman — installed (/usr/bin/podman), detect healthy
- [x] keepassxc — installed (/usr/bin/keepassxc), detected (no verify hook)
- [x] virt-stack — installed (libvirt/qemu, virt-host-validate present), detect healthy

CUDA repo chain (sudo dpkg / apt) — DONE:
- [x] nvidia-cuda-repo — installed (cuda-keyring; ubuntu2604 sources)
- [x] cuda-toolkit — installed (nvcc release 13.3 V13.3.33)
- [x] llvm-clang — installed (clang-21 / LLVM 21; verify clang-21 --version OK)

nix system config (writes /etc/nix) — DONE:
- [x] nix-yazelix-cache — installed (yazelix.cachix.org substituter in /etc/nix/nix.custom.conf)

transitively unblocked (sudo authorized) — DONE:
- [x] ghostty-default-terminal — installed (ghostty set as x-terminal-emulator alternative)
- [x] nvidia-container-toolkit — installed (/usr/bin/nvidia-ctk), detect healthy
- [x] cuda-oxide — installed (cargo-oxide 0.1.0). Required a manifest fix: the install hook
      didn't pin a toolchain, so it built on STABLE (RUSTUP_TOOLCHAIN leaked from `cargo run`)
      and failed E0554 (cuda-core uses `#![feature(f16)]`, nightly-only). Fixed gpu.toml to
      `cargo +nightly-2026-04-03 install` (overrides env + stable default). Re-locked.
- [x] yazelix — installed (heavy nix build; detect healthy)
- [x] yazelix-config — installed. Required deploying the shipped helper: the component runs
      `/usr/local/bin/yazelix-config.sh` (placed there by autoinstall.yaml at OS-install time),
      absent on this box. Deployed all 3 manifest-referenced shipped scripts from assets/scripts/
      to /usr/local/bin (install -D -m755), mirroring autoinstall.yaml lines 160-161.
- [x] yazelix-desktop — installed (detect healthy)

## Final state (2026-06-05, updated) — 45 of 46 components detected+healthy
DONE for everything the loop+sudo can reach. Only `node-via-bun` remains undetected
(tabled by design). After merging origin/master (gpu-verify port + auto-provision) two
previously by-design gaps became REAL, fixable loop work and were closed:
- [x] gpu-verify-scripts — INSTALLED + verified. Master's PR #17 port shipped
      `yazelix-gpu-verify-install.sh`, but it had a **real SIGPIPE/pipefail bug**: the NVIDIA
      gate `lspci | grep -qiE nvidia` under `set -o pipefail` made lspci die SIGPIPE (141) when
      grep -q closed the pipe early, so pipefail reported failure even though nvidia matched →
      the `! pipeline` flipped it to "no NVIDIA GPU (N/A)" on EVERY real multi-line-lspci box.
      Fixed: `grep -iE nvidia >/dev/null` (consumes all input, no SIGPIPE). Redeployed to
      /usr/local/bin; install regenerates smoke-test + launcher + autostart; verify hook GREEN
      (2x RTX 5090, torch sm_120 kernel, cargo-oxide, Podman CDI). detect healthy.
- [x] group-gpu-stack — now detects truthfully. Root cause was envctl running detect via a
      non-interactive `bash -lc` that hits ~/.bashrc's line-10 interactivity guard and returns
      BEFORE the "cuda env" PATH block, so bare `command -v nvcc` false-negated. Fixed the
      aggregator's detect to resolve nvcc by its installed path via the SAME dynamic CUDA_HOME
      the cuda-toolkit component's own verify uses (`[ -x "$CUDA_HOME/bin/nvcc" ]`). Re-locked.
- doctor: all toolchains green; sudo cached; podman 5.7.0; nvidia driver loaded.
- auto-detect: zero drift; only node-via-bun undetected (by design).
- lock --check clean (46 comps); kasetto sync --locked clean; build + no-c/shape/enable PASS.
- PATH/env verified in a fresh **interactive** shell: nvcc 13.3, CUDA_HOME, CUDA_OXIDE_LLC,
  cargo-oxide, secretd/secretctl, torch+CUDA (2x RTX 5090).

## By-design non-loop items (NOT failures — surfaced for the human)
- node-via-bun — TABLED (see above): inapplicable on an n8n box; real node v22 is correct.
  Still the only undetected component. Manifest design follow-up remains (mark not-applicable
  when a real Node in n8n's range is present, or add `node-real` + drop the group-ai-clis edge).

## Resolved since (were "by-design", now fixed — see Final state)
- gpu-verify-scripts — RESOLVED after merging master's port + fixing its SIGPIPE/pipefail gate.
- group-gpu-stack — RESOLVED by making its detect resolve nvcc by installed path (interactive-
  shell-independent). NOTE: the deeper observation stands but is out of loop scope — envctl
  wires the cuda env into ~/.bashrc AFTER the interactivity guard, so nvcc is not on PATH for
  non-interactive shells/services. A Feature-Forge follow-up could wire CUDA system-wide
  (/etc/profile.d/cuda.sh) so scripts/systemd see nvcc too; detect is now truthful regardless.

## Manifest fixes made by this loop (rust-native, committed, re-locked)
1. env-ctl.toml — reversed MSRV `sort -V -C` gate (rejected every cargo >= 1.80).
2. gpu.toml cuda-oxide — pinned `cargo +nightly-2026-04-03` (was building on stable -> E0554).
3. Deployed shipped scripts assets/scripts/{yazelix-config,yazelix-setup,ubuntu-boot-repair}.sh
   -> /usr/local/bin (mirrors autoinstall.yaml; were missing on this non-autoinstalled box).

## grit adoption (meta-repo-wide parallel-agent coordination)
- [x] grit-component — envctl-manage the `grit` parallel-agent AST git-lock coordinator
      (FlexNetOS/grit, `~/Desktop/meta/grit`) as a declarative manifest component:
      detect/install/verify/fix/remove + `~/.cargo/bin` PATH, installed box-wide via
      `cargo install --path`. NOT a Cargo workspace member / NOT a crate dep — it links C
      (rusqlite bundled) + aws/azure SDKs, so it stays an external *tool* binary (managed as
      data, a TOML component) to keep envctl's no-C trust boundary clean. `grit.toml` added,
      `envctl.lock` re-synced. (branch grit-component)
- [ ] grit-harness-parallel — adopt grit `claim → work → done` in the Feature Forge harness so
      multiple rust-implementer agents run in parallel across meta member repos with zero
      merge conflicts: `grit init` per repo (idempotent), opt-in parallel mode in
      `.claude/skills/{feature-forge,forge-loop}` (function-level claims via `file::symbol`,
      `--queue` for contested symbols, `--with-deps` for dependency-aware locks), meta-wide
      seeding via `meta exec -- grit init`. Local SQLite-WAL backend default; Azure/S3 later.
