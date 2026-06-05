#!/usr/bin/env bash
# yazelix-gpu-verify-install.sh — declarative port of yazelix-setup.sh section 3f.
#
# Idempotently (re)creates the GPU stack smoke-test and its post-reboot one-shot
# autostart so envctl can REGENERATE them (the `gpu-verify-scripts` component's
# install/fix runs this), instead of relying on the first-login wizard. GPU-gated:
# a no-op on a non-NVIDIA box. Runs as the logged-in user; writes only under $HOME;
# non-interactive (the smoke test it writes is what prompts, at login, not this).
set -euo pipefail

if ! lspci 2>/dev/null | grep -qiE 'nvidia'; then
  echo "no NVIDIA GPU detected — yazelix-gpu-verify not installed (N/A)"
  exit 0
fi

mkdir -p "$HOME/.local/bin" "$HOME/.config/autostart"

# --- the smoke test (verbatim from yazelix-setup.sh 3f; $HOME stays literal) ---
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

# --- terminal launcher for the autostart (same fallback chain as the wizard) ---
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

# --- one-shot autostart: re-runs the smoke test on next login (post-reboot) ---
# $HOME expands now (the .desktop needs an absolute Exec path).
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

echo "yazelix-gpu-verify installed: ~/.local/bin/yazelix-gpu-verify.sh + launcher + post-reboot autostart"
