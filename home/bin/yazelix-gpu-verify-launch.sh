#!/usr/bin/env bash
V="$HOME/.local/bin/yazelix-gpu-verify.sh"
[ -x "$V" ] || exit 0
if   command -v ghostty        >/dev/null; then exec ghostty -e bash -lc "$V"
elif command -v kgx            >/dev/null; then exec kgx -- bash -lc "$V"
elif command -v gnome-terminal >/dev/null; then exec gnome-terminal -- bash -lc "$V"
elif command -v xterm          >/dev/null; then exec xterm -e bash -lc "$V"
else bash -lc "$V"; fi
