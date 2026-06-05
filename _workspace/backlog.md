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
- [!] pytorch-venv — needs-human (sudo): `python3 -m venv` FAILS — system Python 3.14 has no
      ensurepip/venv module. Requires `sudo apt install python3.14-venv` FIRST, then re-run.
      (Box has `uv` which could build the venv without python3-venv, but the component declares
      `python3 -m venv`; changing that is a manifest design change, not a loop action.)

## Blocked on privilege wall — needs-human (sudo NOT pre-authorized; doctor: sudo X)
These cannot be completed unattended. A human must `sudo -v` in a real terminal (or run
the privileged installs via `! envctl install <id>`), then re-run the loop to resume.

apt base (direct `needs_sudo = true`, `apt-get`):
- [!] ghostty — needs-human: apt-get install needs sudo (not pre-authorized)
- [!] podman — needs-human: apt-get install needs sudo
- [!] keepassxc — needs-human: apt-get install needs sudo
- [!] virt-stack — needs-human: apt-get install needs sudo

CUDA repo chain (sudo dpkg / apt):
- [!] nvidia-cuda-repo — needs-human: `sudo dpkg -i cuda-keyring` + `sudo apt-get update`
- [!] cuda-toolkit — needs-human: apt needs_sudo + requires nvidia-cuda-repo (blocked)
- [!] llvm-clang — needs-human: apt needs_sudo + requires nvidia-cuda-repo (blocked)

nix system config (writes /etc/nix — /etc not writable):
- [!] nix-yazelix-cache — needs-human: `sudo install/tee /etc/nix/nix.custom.conf` + systemctl

transitively blocked (depend on a needs-human item above):
- [!] ghostty-default-terminal — needs-human: `sudo update-alternatives` + requires ghostty
- [!] cuda-oxide — needs-human: requires cuda-toolkit + llvm-clang (both sudo-blocked)
- [!] nvidia-container-toolkit — needs-human: requires podman (sudo-blocked)
- [!] yazelix — needs-human: requires nix-yazelix-cache + ghostty (sudo-blocked)
- [!] yazelix-config — needs-human: requires yazelix (blocked)
- [!] yazelix-desktop — needs-human: requires yazelix (blocked)

## Notes
- `podman` shows `(absent)` in doctor toolchains; everything else in doctor's toolchain
  list is green. The 14 blocked items are the real gap to a fully-green box and require
  one privileged human session (apt + cuda-keyring + /etc/nix), after which yazelix /
  cuda-oxide / nvidia-container-toolkit unblock and the loop can finish them.
