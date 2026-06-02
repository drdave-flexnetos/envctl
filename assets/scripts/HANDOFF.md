# HANDOFF — Ubuntu 26.04 dual-RTX-5090 autoinstall build

Paste this whole file as the first message of the next session to continue.

---

## Mission
Build a fully-automated **Ubuntu 26.04 LTS (Resolute Raccoon, x86_64)** developer
workstation from USB, for a machine with **two NVIDIA RTX 5090 (GB202 / Blackwell,
sm_120)** GPUs. One unattended Subiquity autoinstall + a first-login wizard that
sets up the whole dev/AI/GPU toolchain. Today's working date in prior sessions
was 2026-06-02.

## Files (authored on the build host, `/home/drdave/Desktop/`)
- `autoinstall.yaml` — Subiquity autoinstall (cloud-init user-data).
- `yazelix-setup.sh` — first-login wizard (the big one; installs everything that
  needs a real session/network/login).
- `yazelix-config.sh` — OPTIONAL helper: restores yazelix's canonical
  `settings.jsonc` if missing (`--force` to reset). Not run during install.
- `HANDOFF.md` — this file.

## USB layout — CIDATA volume (REQUIRED locations)
Two USB sticks: (1) the Ubuntu 26.04 **Desktop ISO** (flashed normally), and
(2) a **FAT32** stick (or partition) whose filesystem **LABEL is exactly `CIDATA`**.
Put these at the **root of the CIDATA volume**, with `autoinstall.yaml` RENAMED to
`user-data`:

```
CIDATA/
├── user-data          # <- rename of autoinstall.yaml
├── meta-data          # <- empty file (must exist)
├── yazelix-setup.sh   # copied by autoinstall to /usr/local/bin/yazelix-setup.sh
├── yazelix-config.sh  # copied by autoinstall to /usr/local/bin/yazelix-config.sh
└── authorized_keys    # your SSH public key(s); imported to ~/.ssh/authorized_keys
```
Boot the installer with BOTH USBs inserted. Append `autoinstall` to the GRUB entry
if it does not start automatically.

### Where things land on the installed system
- `/usr/local/bin/yazelix-setup.sh` and `/usr/local/bin/yazelix-config.sh`
  (copied from CIDATA by `late-commands`).
- `/usr/local/bin/yazelix-setup-launch.sh` (written by cloud-init `write_files`).
- `~/.config/autostart/yazelix-setup.desktop` (first-login wizard autostart;
  the wizard deletes it on clean finish).
- Wizard generates at runtime: `~/.local/bin/yazelix-gpu-verify.sh`,
  `~/.local/bin/yazelix-gpu-verify-launch.sh`,
  `~/.config/autostart/yazelix-gpu-verify.desktop` (one-shot, self-disables).

## Pre-flight edits REQUIRED before flashing (in `autoinstall.yaml` `identity:`)
- `hostname`, `realname`, `username`
- `password` = a **SHA-512 crypt hash** (`mkpasswd --method=SHA-512` or
  `openssl passwd -6`) — NOT plaintext.
- `timezone` if not America/Los_Angeles.
- If the target has >1 disk, uncomment `storage.layout.match` and pin the device
  (it WIPES the whole disk, `direct` ext4 layout).

## What the build installs
**Base (apt, during install):** GNOME desktop (`source id: ubuntu-desktop`),
openssh-server, dev base (git/curl/build-essential/jq/etc.), Ghostty 1.3.0,
KeePassXC, QEMU/KVM stack (qemu-system-x86, libvirt, virt-manager-less—clients,
ovmf, bridge-utils, dnsmasq-base), Podman rootless (podman, podman-compose,
buildah, skopeo, uidmap, slirp4netns). **No apt nodejs/npm** (node comes via Bun).
**No NVIDIA driver in base** (installed by the wizard from NVIDIA's repo).

**Wizard (first login, as user w/ sudo):** Nerd Fonts; **Bun** (JS runtime +
pkg mgr) with `node`→bun symlink (node-compat); AI CLIs — Claude Code, Kimi,
Devin (native curl installers), **Codex + Gemini via `bun install -g`**; Rust via
rustup; **rtk** (cargo); **CUDA 13.3 + nvidia-open 610** from NVIDIA's ubuntu2604
repo (`cuda-keyring` → `cuda-toolkit-13-3 nvidia-open` + llvm-21/clang-21 +
libclang-{21,cpp21,common-21}-dev); **cuda-oxide** (`cargo install --git
NVlabs/cuda-oxide cargo-oxide`, nightly-2026-04-03 + rust-src/rustc-dev/llvm-tools);
**NVIDIA Container Toolkit** + `nvidia-ctk cdi generate` (rootless Podman GPU via
`--device nvidia.com/gpu=all`); **PyTorch** cu132 in isolated venv `~/.venvs/torch`;
**gh** (official apt repo); **Vite** (`bun add -g`); **wasmer**; **uv**; **Nix**
(Determinate, flakes); yazelix Cachix cache in `/etc/nix/nix.custom.conf`
(eval-cores=0); **home-manager**; **yazelix** (`nix profile add --refresh
github:luccahuguet/yazelix#yazelix`, default Ghostty batteries-included variant)
+ `yzx desktop install`; **GPU smoke test** (3f) + post-reboot one-shot.

**Yazelix is the default everyday shell:** wizard appends a guarded auto-enter
block to `~/.bashrc` (loads Nix onto PATH, runs `yzx enter` for top-level
interactive shells; guarded against zellij panes / non-interactive / dumb term).
Ghostty set as default x-terminal-emulator. yazelix's `default_shell` is `nu`,
so nushell + mise come from inside the yazelix runtime (not installed separately).

## Key DECISIONS already made (do not relitigate without reason)
1. **CUDA stack = NVIDIA-official 13.3 + nvidia-open 610**, **Secure Boot OFF**
   (user-confirmed). nvidia-open is GA-matched to 13.3 (so cuda-oxide PTX JITs at
   runtime) and uses OPEN kernel modules (required for consumer Blackwell). It is
   NOT Canonical-signed → only safe unattended because Secure Boot is off.
2. **node via Bun** (user-corrected me): Bun runs in node-compat mode when invoked
   as `node`; apt nodejs removed; Codex/Gemini installed via `bun install -g`.
3. **PyTorch kept** (verified no conflict): cu132 channel ships torch 2.12.0 +
   torchvision 0.27.0 with **cp314** (Python 3.14 — 26.04's version) x86_64
   manylinux_2_28 wheels incl. sm_120; bundles its own CUDA 13.2 runtime in the
   venv (isolated from system CUDA 13.3); glibc 2.43 OK.
4. **yazelix owns its own config** — installer does NOT pre-seed settings.jsonc;
   yazelix's default is batteries-included = "full feature set".
5. **Driver/CUDA NOT frozen** (user said "do not freeze it yet") — no apt pin
   added; `cuda-toolkit-13-3` is version-locked but the driver can move within
   NVIDIA's repo.

## Validation status (all PASSING as of last session)
- `cloud-init schema --config-file user-data` → **Valid** (cloud-init 26.1).
- `bash -n` clean: yazelix-setup.sh, yazelix-config.sh, embedded gpu-verify +
  launcher; PyTorch python block parses.
- Folded-scalar `late-commands`/`runcmd` extracted and `bash -n`'d.
- All version/package facts verified on the live 26.04 box and/or via deep
  research (105-agent run) + NVIDIA's official CUDA Linux install guide.

## Known behaviors / caveats to remember
- **First boot is software-rendered** (nouveau/llvmpipe) until the wizard installs
  nvidia-open and you **REBOOT**. GNOME + the autostart wizard still work under
  software rendering. After reboot: full dual-5090 + GPU smoke test auto-runs.
- Multiple **curl|bash installers** (Claude/Kimi/Devin/Bun/rustup/wasmer/uv/Nix)
  = inherent supply-chain exposure; acceptable per user, could be hardened with
  checksums if asked.
- Version pins are point-in-time (June 2026): Codex/Gemini npm, cuda-oxide nightly
  (nightly-2026-04-03), torch 2.12/cu132 — re-verify at build time.

## OPEN / possible next steps
- (Optional) Add an apt **pin** to freeze the NVIDIA driver — user explicitly
  deferred ("do not freeze it yet").
- (Optional) Harden curl|bash installers with checksums/pinned script versions.
- (Optional) Verify on real hardware: run the install, confirm GPU smoke test
  goes green (nvidia-smi, torch.cuda sm_120 kernel, cargo-oxide, podman CDI).
- (Optional) Re-confirm research's still-unverified items if desired: Secure-Boot
  signing details for -open (moot since SB off), exact qemu-system-x86 vs metas,
  yazelix ordering (Nix+home-manager+Ghostty before yazelix — already ordered).
- Build the USB: copy the 4 files to a CIDATA-labeled FAT32 volume (see layout).

## Reference: the machine
Live build host = an installed 26.04 with the same dual RTX 5090 (GB202). `lspci`
shows 2× "NVIDIA Corporation GB202 [GeForce RTX 5090]". `ubuntu-drivers devices`
recommends `nvidia-driver-595-open` (Ubuntu-signed), but the build deliberately
uses NVIDIA-repo `nvidia-open` 610 instead (see Decision #1).
