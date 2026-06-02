#!/usr/bin/env bash
# =============================================================================
# First-login developer setup wizard  (Ubuntu 26.04 LTS)
#
# Order (yazelix wants Nix + home-manager + Ghostty in place before yazelix):
#   Nerd Fonts -> AI CLIs (claude/codex/gemini/kimi/devin) -> rtk
#   -> Nix (Determinate) -> /etc/nix/nix.custom.conf edits + yazelix Cachix
#      cache + restart nix-daemon -> home-manager -> yazelix -> yzx desktop/doctor
#
# NOT installed here: nushell + mise. The yazelix runtime bundles both (use them
# via `yzx env` or from inside yazelix). Ghostty + Node come from apt at install.
#
# Auto-launched once on first GUI login via ~/.config/autostart; removes its own
# autostart entry on a clean finish. Re-run any time: yazelix-setup.sh
# Runs as the logged-in user; uses sudo only for the few system steps.
# Each step is best-effort: failures are logged and the wizard keeps going.
# =============================================================================
set -uo pipefail

LOG="$HOME/yazelix-setup.log"
AUTOSTART="$HOME/.config/autostart/yazelix-setup.desktop"
exec > >(tee -a "$LOG") 2>&1

c_ok()   { printf '\033[1;32m  ✓ %s\033[0m\n' "$*"; }
c_warn() { printf '\033[1;33m  ! %s\033[0m\n' "$*"; }
c_step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
fail=()
run() {  # run "<label>" cmd...   -> records failures, never aborts the wizard
  local label="$1"; shift
  c_step "$label"
  if "$@"; then c_ok "$label"; else c_warn "FAILED: $label (see $LOG)"; fail+=("$label"); return 1; fi
}
load_nix() { . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh 2>/dev/null || true
             export PATH="$HOME/.nix-profile/bin:$PATH"; }

cat <<'BANNER'

  ┌────────────────────────────────────────────────────────┐
  │   First-login developer environment setup                │
  │   AI CLIs · rtk · Nix · yazelix · home-manager           │
  │   (mise + nushell come bundled inside the yazelix runtime)│
  └────────────────────────────────────────────────────────┘

  Downloads a lot of tooling; needs the internet (~15–30 min).
  Safe to re-run. Full log: ~/yazelix-setup.log

BANNER
read -rp "  Press Enter to begin (Ctrl-C to cancel)… " _ || true

# --- 0. Keep sudo warm -------------------------------------------------------
sudo -v || { c_warn "sudo required"; exit 1; }
( while true; do sudo -n true; sleep 50; done ) 2>/dev/null &
SUDO_KEEPALIVE=$!
trap 'kill "$SUDO_KEEPALIVE" 2>/dev/null' EXIT

# Sanity: Ghostty should already be present from the apt install stage.
# (Node is provided via Bun in section 2 — no apt nodejs in this build.)
command -v ghostty >/dev/null && c_ok "ghostty present (apt)" || c_warn "ghostty missing — install it before yazelix"

# --- 1. Nerd Fonts ----------------------------------------------------------
run "Nerd Fonts (JetBrainsMono, FiraCode)" bash -c '
  set -e
  FD="$HOME/.local/share/fonts"; mkdir -p "$FD"; cd "$(mktemp -d)"
  base="https://github.com/ryanoasis/nerd-fonts/releases/latest/download"
  for f in JetBrainsMono FiraCode; do
    curl -fsSL -o "$f.zip" "$base/$f.zip"
    unzip -oq "$f.zip" -d "$FD/$f-NerdFont"
  done
  fc-cache -f >/dev/null'

# --- 2. JS runtime (Bun) + AI coding CLIs -----------------------------------
# "node via Bun": Bun is the JS runtime AND package manager. We install Bun
# first, then expose `node` as a symlink to Bun — Bun runs in Node-compat mode
# when invoked as `node`, so `#!/usr/bin/env node` shebangs resolve to Bun and
# no separate Node.js install is needed. Codex/Gemini are installed with
# `bun install -g` (their bins run via Bun). Claude/Kimi/Devin use their own
# native installers (independent of Node).
run "Bun (JS runtime + package manager)" bash -c 'curl -fsSL https://bun.sh/install | bash'
export BUN_INSTALL="$HOME/.bun"; export PATH="$BUN_INSTALL/bin:$HOME/.local/bin:$PATH"
grep -q '.bun/bin' "$HOME/.bashrc" 2>/dev/null || \
  echo 'export PATH="$HOME/.bun/bin:$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
# Provide `node` via Bun (Bun detects argv0=node and runs in Node-compat mode).
if command -v bun >/dev/null; then
  ln -sf "$(command -v bun)" "$BUN_INSTALL/bin/node"
  command -v node >/dev/null && c_ok "node -> bun ($(node --version 2>/dev/null))" \
    || c_warn "node symlink created but not yet on PATH (new shell will pick it up)"
fi

run "Claude Code CLI"          bash -c 'curl -fsSL https://claude.ai/install.sh | bash'
run "Codex + Gemini (via bun)" bash -c '
  export BUN_INSTALL="$HOME/.bun"; export PATH="$BUN_INSTALL/bin:$PATH"
  bun install -g @openai/codex @google/gemini-cli'
run "Kimi CLI"                 bash -c 'curl -LsSf https://code.kimi.com/install.sh | bash'
run "Devin CLI"                bash -c 'curl -fsSL https://cli.devin.ai/install.sh | bash'

# --- 3. rtk (Rust Token Killer) via rustup ----------------------------------
if ! command -v cargo >/dev/null; then
  run "Rust toolchain (rustup)" bash -c \
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
fi
. "$HOME/.cargo/env" 2>/dev/null || export PATH="$HOME/.cargo/bin:$PATH"
run "rtk (cargo install)" bash -c '
  . "$HOME/.cargo/env" 2>/dev/null || export PATH="$HOME/.cargo/bin:$PATH"
  cargo install --git https://github.com/rtk-ai/rtk'

# --- 3b. CUDA + cuda-oxide + NVIDIA Container Toolkit + PyTorch --------------
# Only on machines with an NVIDIA GPU. Per NVIDIA's official CUDA install guide
# (docs.nvidia.com/cuda/cuda-installation-guide-linux), we add the ubuntu2604
# network repo and install CUDA 13.3 (cuda-toolkit-13-3, version-locked) plus the
# GA-matched OPEN-module driver (nvidia-open 610.43.02 — required for Blackwell
# and so cuda-oxide's emitted PTX JITs correctly at runtime). Assumes UEFI Secure
# Boot is DISABLED (NVIDIA's driver is not Canonical-signed; with Secure Boot on
# this would need interactive MOK enrollment). PyTorch installs separately in an
# isolated venv from the self-contained cu132 wheels. cuda-oxide also needs
# LLVM 21+ (llc auto-discovered), clang-21 + libclang dev headers, Rust nightly.
if lspci 2>/dev/null | grep -qiE 'nvidia'; then
  GPU="$(lspci 2>/dev/null | grep -iE 'vga|3d' | grep -i nvidia | sed 's/.*: //' | head -n1)"
  c_step "NVIDIA GPU detected (${GPU:-unknown}) — installing CUDA 13.3 + nvidia-open + cuda-oxide + container toolkit + PyTorch"

  # NVIDIA network repo (official) -> CUDA 13.3 toolkit + nvidia-open 610 driver,
  # plus LLVM/clang + libclang dev headers cuda-oxide's bindgen needs (clang-21
  # alone is not enough — bindgen needs the clang resource-dir headers).
  run "NVIDIA repo + CUDA 13.3 + nvidia-open 610 + LLVM 21 + clang-21" bash -c '
    set -e
    cd "$(mktemp -d)"
    curl -fsSLO https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2604/x86_64/cuda-keyring_1.1-1_all.deb
    sudo dpkg -i cuda-keyring_1.1-1_all.deb
    sudo apt-get update -y
    sudo apt-get install -y cuda-toolkit-13-3 nvidia-open \
      llvm-21 llvm-21-tools clang-21 \
      libclang-21-dev libclang-cpp21-dev libclang-common-21-dev'

  # Resolve CUDA home + an llc-21/22 binary, then persist env (guarded block).
  CUDA_HOME=/usr/local/cuda
  [ -d "$CUDA_HOME/bin" ] || CUDA_HOME="$(ls -d /usr/local/cuda-* 2>/dev/null | sort -V | tail -n1)"
  LLC="$(command -v llc-22 || command -v llc-21 || true)"
  [ -z "$LLC" ] && LLC="$(ls /usr/lib/llvm-2*/bin/llc 2>/dev/null | sort -V | tail -n1)"
  export PATH="$CUDA_HOME/bin:$PATH"; export CUDA_OXIDE_LLC="$LLC"
  if ! grep -q 'BEGIN cuda env' "$HOME/.bashrc" 2>/dev/null; then
    cat >> "$HOME/.bashrc" <<EOF
# >>> BEGIN cuda env (added by yazelix-setup.sh) >>>
export PATH="$CUDA_HOME/bin:\$PATH"
export LD_LIBRARY_PATH="$CUDA_HOME/lib64:\${LD_LIBRARY_PATH:-}"
export CUDA_OXIDE_LLC="$LLC"
# <<< END cuda env <<<
EOF
    c_ok "CUDA + llc env added to ~/.bashrc (CUDA_OXIDE_LLC=$LLC)"
  fi

  run "Rust nightly + components for cuda-oxide" bash -c '
    . "$HOME/.cargo/env" 2>/dev/null || export PATH="$HOME/.cargo/bin:$PATH"
    rustup toolchain install nightly-2026-04-03
    rustup component add rust-src rustc-dev llvm-tools --toolchain nightly-2026-04-03'

  run "cargo-oxide (latest, from git)" bash -c '
    . "$HOME/.cargo/env" 2>/dev/null || export PATH="$HOME/.cargo/bin:$PATH"
    cargo install --git https://github.com/NVlabs/cuda-oxide.git cargo-oxide'

  # NVIDIA Container Toolkit + CDI so rootless Podman can use the GPUs.
  run "NVIDIA Container Toolkit (+ Podman CDI)" bash -c '
    set -e
    curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey \
      | sudo gpg --dearmor -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg
    curl -fsSL https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list \
      | sed "s#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://#g" \
      | sudo tee /etc/apt/sources.list.d/nvidia-container-toolkit.list >/dev/null
    sudo apt-get update -y
    sudo apt-get install -y nvidia-container-toolkit
    # Generate a CDI spec (the rootless-Podman path: podman run --device nvidia.com/gpu=all)
    sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml || true'

  # PyTorch in an isolated venv. VERIFIED no-conflict (June 2026): the cu132
  # channel ships torch 2.12.0+cu132 / torchvision 0.27.0+cu132 with cp314
  # (Python 3.14, matching 26.04) x86_64 manylinux_2_28 wheels that include
  # Blackwell sm_120 kernels. The wheels bundle their own CUDA 13.2 runtime, so
  # they never touch system CUDA 13.3; they only need a recent driver (610 OK).
  # Activate with: source ~/.venvs/torch/bin/activate
  run "PyTorch (cu132) in ~/.venvs/torch" bash -c '
    set -e
    python3 -m venv "$HOME/.venvs/torch"
    "$HOME/.venvs/torch/bin/pip" install --upgrade pip
    "$HOME/.venvs/torch/bin/pip" install torch torchvision --index-url https://download.pytorch.org/whl/cu132'

  if command -v nvidia-smi >/dev/null && nvidia-smi -L >/dev/null 2>&1; then
    c_ok "$(nvidia-smi -L 2>/dev/null | head -n2 | tr '\n' ';')"
  else
    c_warn "NVIDIA driver not active yet — REBOOT for it to load, then test cuda-oxide"
  fi
else
  c_warn "No NVIDIA GPU detected — skipping CUDA + cuda-oxide + PyTorch"
fi

# --- 3e. Extra dev toolchains: gh, Vite, wasmer, uv -------------------------
# (Bun + node->bun were set up in section 2, before the AI CLIs.)
# cargo is already present (rustup, from the rtk step) — confirm it.
. "$HOME/.cargo/env" 2>/dev/null || export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null && c_ok "cargo $(cargo --version 2>/dev/null | awk '{print $2}')" \
  || c_warn "cargo missing (rustup step may have failed)"

# GitHub CLI — official apt repo for the latest gh (newer than apt's 2.46).
run "GitHub CLI (gh, official repo)" bash -c '
  set -e
  sudo install -dm 755 /etc/apt/keyrings
  curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
    | sudo tee /etc/apt/keyrings/githubcli-archive-keyring.gpg >/dev/null
  sudo chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
    | sudo tee /etc/apt/sources.list.d/github-cli.list >/dev/null
  sudo apt-get update -y
  sudo apt-get install -y gh'

export BUN_INSTALL="$HOME/.bun"; export PATH="$BUN_INSTALL/bin:$PATH"
# Vite via Bun (global). Use `bunx create-vite` to scaffold projects.
run "Vite (via bun add -g)" bash -c '
  export BUN_INSTALL="$HOME/.bun"; export PATH="$BUN_INSTALL/bin:$PATH"
  bun add -g vite'

# wasmer — WebAssembly runtime (its installer appends its own PATH to ~/.bashrc).
run "Wasmer (WASM runtime)" bash -c 'curl -fsSL https://get.wasmer.io | sh'

# uv — fast Python package/project manager (latest Python toolchain).
run "uv (Python toolchain)" bash -c 'curl -LsSf https://astral.sh/uv/install.sh | sh'

# --- 3f. GPU stack smoke test (runs now + once after the driver reboot) ------
# The NVIDIA driver only becomes active after a reboot, so this verification is
# also installed as a one-shot autostart that re-runs on the next login and
# self-disables once the driver is live and the checks pass.
if lspci 2>/dev/null | grep -qiE 'nvidia'; then
  mkdir -p "$HOME/.local/bin" "$HOME/.config/autostart"

  cat > "$HOME/.local/bin/yazelix-gpu-verify.sh" <<'YZXGPU'
#!/usr/bin/env bash
# GPU stack smoke test: NVIDIA driver, PyTorch CUDA (+ sm_120 kernel), cuda-oxide,
# and Podman CDI. Auto-runs once after the post-install reboot, then self-disables.
set -uo pipefail
AUTOSTART="$HOME/.config/autostart/yazelix-gpu-verify.desktop"
ok(){   printf '\033[1;32m  ✓ %s\033[0m\n' "$*"; }
no(){   printf '\033[1;31m  ✗ %s\033[0m\n' "$*"; }
warn(){ printf '\033[1;33m  ! %s\033[0m\n' "$*"; }
echo; echo "================  GPU stack verification  ================"; echo

DRIVER_OK=0
if command -v nvidia-smi >/dev/null && nvidia-smi -L >/dev/null 2>&1; then
  nvidia-smi -L | sed 's/^/  /'; ok "NVIDIA driver active"; DRIVER_OK=1
  nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader 2>/dev/null | sed 's/^/  /'
else
  no "NVIDIA driver not active yet — REBOOT, then this re-runs automatically at login"
fi

echo; echo "-- PyTorch --"
if [ -x "$HOME/.venvs/torch/bin/python" ]; then
  "$HOME/.venvs/torch/bin/python" - <<'PY' || warn "PyTorch check raised an error"
import torch
print("  torch", torch.__version__, "| CUDA build", torch.version.cuda)
avail = torch.cuda.is_available()
print("  torch.cuda.is_available():", avail)
if avail:
    for i in range(torch.cuda.device_count()):
        cc = ".".join(map(str, torch.cuda.get_device_capability(i)))
        print(f"   - [{i}] {torch.cuda.get_device_name(i)}  (sm_{cc.replace('.','')})")
    x = torch.rand(4096, device="cuda"); y = (x @ x.unsqueeze(1).squeeze()).sum()
    torch.cuda.synchronize()
    print("  sm_120 kernel ran OK; sample sum =", float(y))
else:
    print("  (no CUDA — expected before the driver reboot)")
PY
else
  warn "PyTorch venv not found at ~/.venvs/torch"
fi

echo; echo "-- cuda-oxide --"
export PATH="$HOME/.cargo/bin:$PATH"
if command -v cargo-oxide >/dev/null; then ok "cargo-oxide present ($(cargo-oxide --version 2>/dev/null | head -n1))"
else warn "cargo-oxide not on PATH (open a new shell, or check the install log)"; fi

echo; echo "-- Podman GPU (CDI) --"
if [ -f /etc/cdi/nvidia.yaml ]; then
  ok "CDI spec present: /etc/cdi/nvidia.yaml"
  echo "     test: podman run --rm --device nvidia.com/gpu=all docker.io/nvidia/cuda:13.3.0-base-ubuntu24.04 nvidia-smi"
else
  warn "CDI spec missing — run: sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml"
fi

echo
if [ "$DRIVER_OK" = 1 ]; then
  rm -f "$AUTOSTART" 2>/dev/null && ok "Verification complete — disabled the post-reboot auto-run."
else
  warn "Re-run after reboot:  ~/.local/bin/yazelix-gpu-verify.sh"
fi
echo "=========================================================="
read -rp "  Press Enter to close… " _ 2>/dev/null || true
YZXGPU
  chmod +x "$HOME/.local/bin/yazelix-gpu-verify.sh"

  # Terminal launcher for the post-reboot autostart (same fallback chain as the wizard).
  cat > "$HOME/.local/bin/yazelix-gpu-verify-launch.sh" <<'YZXGPUL'
#!/usr/bin/env bash
V="$HOME/.local/bin/yazelix-gpu-verify.sh"
[ -x "$V" ] || exit 0
if   command -v ghostty        >/dev/null; then exec ghostty -e bash -lc "$V"
elif command -v kgx            >/dev/null; then exec kgx -- bash -lc "$V"
elif command -v gnome-terminal >/dev/null; then exec gnome-terminal -- bash -lc "$V"
elif command -v xterm          >/dev/null; then exec xterm -e bash -lc "$V"
else bash -lc "$V"; fi
YZXGPUL
  chmod +x "$HOME/.local/bin/yazelix-gpu-verify-launch.sh"

  # One-shot autostart: re-runs the smoke test on next login (post-reboot).
  cat > "$HOME/.config/autostart/yazelix-gpu-verify.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=GPU Stack Verification
Comment=Verifies NVIDIA driver, PyTorch CUDA, cuda-oxide, Podman GPU after reboot
Exec=$HOME/.local/bin/yazelix-gpu-verify-launch.sh
Terminal=false
X-GNOME-Autostart-enabled=true
X-GNOME-Autostart-Delay=12
EOF

  # Run it once now (pre-reboot: confirms installs; driver shows as not-yet-active).
  c_step "GPU stack smoke test (initial run)"
  bash "$HOME/.local/bin/yazelix-gpu-verify.sh" </dev/null || true
fi

# --- 4. Nix (Determinate Systems installer, flakes enabled) ------------------
if ! command -v nix >/dev/null && [ ! -e /nix ]; then
  run "Nix (Determinate Systems)" bash -c \
    "curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install --no-confirm"
fi
load_nix
command -v nix >/dev/null && c_ok "nix $(nix --version 2>/dev/null)" || { c_warn "nix not on PATH; aborting yazelix steps"; }

# --- 5. Nix env/profile edits + yazelix Cachix cache (per yazelix docs) ------
# /etc/nix/nix.custom.conf: parallel eval + the yazelix binary cache. Using
# extra-* keys so the default cache.nixos.org substituter is preserved.
if command -v nix >/dev/null; then
  run "Nix custom.conf: eval-cores + yazelix Cachix cache" bash -c '
    set -e
    sudo install -dm 755 /etc/nix
    sudo touch /etc/nix/nix.custom.conf
    add() { grep -qF -- "$1" /etc/nix/nix.custom.conf || echo "$1" | sudo tee -a /etc/nix/nix.custom.conf >/dev/null; }
    add "eval-cores = 0"
    add "extra-substituters = https://yazelix.cachix.org"
    add "extra-trusted-public-keys = yazelix.cachix.org-1:ZgxIjQvaP0VTWL8Racx27mpUNzDJ97xC2y7QWYjmGNM="
    sudo systemctl restart nix-daemon 2>/dev/null || true'
  # Verify Nix actually sees the cache (yazelix docs check).
  c_step "Verify yazelix cache is active"
  if nix config show 2>/dev/null | grep -qE 'yazelix\.cachix\.org'; then
    c_ok "yazelix.cachix.org substituter active"
  else
    c_warn "yazelix cache not visible yet (yazelix will build from source)"
  fi
fi

# --- 6. home-manager (before yazelix) ---------------------------------------
if command -v nix >/dev/null; then
  run "home-manager" bash -c '
    . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh 2>/dev/null || true
    nix profile install nixpkgs#home-manager'
  load_nix
  command -v home-manager >/dev/null && c_ok "home-manager $(home-manager --version 2>/dev/null | head -n1)"
fi

# --- 7. yazelix (Ghostty variant; --refresh pulls from the cache) -----------
if command -v nix >/dev/null; then
  run "yazelix (nix profile add --refresh)" bash -c '
    . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh 2>/dev/null || true
    nix profile add --refresh github:luccahuguet/yazelix#yazelix'
  load_nix
  # NOTE: settings.jsonc is intentionally NOT pre-written here. yazelix seeds its
  # own full default (all options) on first launch — letting it do so avoids any
  # conflict with how yazelix installs. The optional helper /usr/local/bin/
  # yazelix-config.sh can restore that default later if it ever goes missing.
  # Desktop launcher entry + health check (per yazelix docs).
  command -v yzx >/dev/null && run "yzx desktop install" bash -c 'yzx desktop install' || c_warn "yzx not on PATH yet (open a new shell)"
  command -v yzx >/dev/null && { c_step "yzx doctor"; yzx doctor || true; }
else
  c_warn "Skipping home-manager + yazelix (nix unavailable this session)"
  fail+=("yazelix (nix unavailable)")
fi

# --- 8. Make yazelix the default everyday host shell ------------------------
# Append a guarded auto-enter block to ~/.bashrc so opening ANY terminal drops
# you into yazelix (default_shell = nu). Nix is sourced first because GUI
# terminals start NON-login shells that skip /etc/profile.d. Guards prevent
# re-entry from zellij panes, non-interactive shells, and dumb terminals.
if command -v yzx >/dev/null; then
  c_step "Set yazelix as the default everyday shell (~/.bashrc auto-enter)"
  if grep -q 'BEGIN yazelix auto-enter' "$HOME/.bashrc" 2>/dev/null; then
    c_ok "auto-enter block already present"
  else
    cat >> "$HOME/.bashrc" <<'YZXRC'

# >>> BEGIN yazelix auto-enter (added by yazelix-setup.sh) >>>
# Load Nix into interactive (incl. non-login) shells so `yzx` is on PATH.
if [ -e /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]; then
  . /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
fi
export PATH="$HOME/.nix-profile/bin:$PATH"
# Enter yazelix for top-level interactive shells; return to bash on exit.
if [[ $- == *i* ]] \
   && [[ -z "${ZELLIJ:-}" ]] \
   && [[ -z "${YAZELIX_ACTIVE:-}" ]] \
   && [[ "${TERM:-dumb}" != "dumb" ]] \
   && command -v yzx >/dev/null 2>&1; then
  export YAZELIX_ACTIVE=1
  yzx enter
fi
# <<< END yazelix auto-enter <<<
YZXRC
    c_ok "auto-enter block added to ~/.bashrc"
  fi
  # Best-effort: make Ghostty the default terminal emulator for the desktop.
  if command -v ghostty >/dev/null; then
    sudo update-alternatives --install /usr/bin/x-terminal-emulator x-terminal-emulator "$(command -v ghostty)" 60 >/dev/null 2>&1 || true
    sudo update-alternatives --set x-terminal-emulator "$(command -v ghostty)" >/dev/null 2>&1 || true
    c_ok "Ghostty set as default x-terminal-emulator (best effort)"
  fi
fi

# --- Done -------------------------------------------------------------------
c_step "Setup summary"
if [ ${#fail[@]} -eq 0 ]; then
  c_ok "All steps completed."
  rm -f "$AUTOSTART" && c_ok "Disabled first-login autostart."
else
  c_warn "Completed with ${#fail[@]} item(s) needing attention:"
  for f in "${fail[@]}"; do printf '      - %s\n' "$f"; done
  echo
  echo "  Autostart kept ON so you can re-run after fixing. To disable manually:"
  echo "    rm -f $AUTOSTART"
fi

cat <<'NEXT'

  yazelix is now your DEFAULT shell: open a new terminal and it auto-enters
  (nushell + mise live inside it). To get a plain bash prompt instead, run:
  YAZELIX_ACTIVE=1 bash   — or comment out the block in ~/.bashrc.

  First-run notes:
    • AI logins:     claude / codex / gemini / kimi   (run /login as prompted)
    • Devin:         cd <project> && devin
    • rtk:           rtk gain ; rtk init -g
    • GPU verify:    runs automatically after you REBOOT (auto-disables once OK);
                     or manually: ~/.local/bin/yazelix-gpu-verify.sh
    • cuda-oxide:    cargo oxide run <example>   (reboot first if driver just installed)
    • GPU check:     nvidia-smi
    • PyTorch:       source ~/.venvs/torch/bin/activate ; python -c 'import torch;print(torch.cuda.is_available())'
    • GPU + Podman:  podman run --rm --device nvidia.com/gpu=all <image> nvidia-smi
    • Bun/Vite:      bun --version ; bunx create-vite my-app
    • wasmer:        wasmer --version
    • uv (python):   uv --version
    • gh:            gh auth login
    • Containers:    podman info           (rootless, no daemon)
    • yazelix help:  yzx help ; yzx doctor
    • home-manager:  home-manager init --switch   (optional, to go declarative)

  Full log: ~/yazelix-setup.log
NEXT
read -rp "  Press Enter to close… " _ || true
