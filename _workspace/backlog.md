# env-install-loop backlog — 2026-06-05T02:52Z

Source of truth for the provisioning loop. Built from REAL state:
`envctl doctor` + `auto-detect --json` (17 `missing`/medium drift items; zero
config/health drift; every *detected* component healthy). Ordered most-foundational
first per the dependency graph (`requires` edges).

Legend: `- [ ]` todo · `- [x]` installed+verified · `- [!]` blocked (reason).

## Loop-installable (user-space, prerequisites met, no sudo)
- [!] node-via-bun — CONFLICT/needs-human: symlink ~/.bun/bin/node->bun was created, but a
      REAL node v22.22.3 (~/.local/bin/node -> ~/.local/node/bin/node) precedes ~/.bun/bin on
      PATH, so detect (`command -v node` -> readlink grep bun) can never pass. Also bun 1.3.14's
      node wrapper FAILS `node --version` ("does not support a repl"), so the verify hook can't
      pass even if it won PATH. Declared bun-symlink-node conflicts with the installed real node.
      Decision needed: (a) keep real node, drop this component (envctl reset node-via-bun); or
      (b) remove ~/.local/bin/node so the bun symlink wins (and fix bun node-compat).
- [ ] env-ctl — cargo build secretd/secretctl from workspace -> ~/.cargo/bin + XDG dirs
      + systemd user unit (requires: rustup OK). Needs ENV_CTL_REPO=this worktree.
- [ ] pytorch-venv — python venv ~/.venvs/torch + pip torch/torchvision cu132
      (no requires). User-space but multi-GB download (~10-15 min; disk 2.5T free, OK).

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
