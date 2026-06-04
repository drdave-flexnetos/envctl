#!/usr/bin/env bash
# Install envctl-gui as a desktop application (binary + launcher + icon).
# Idempotent: safe to re-run. User-scoped (no sudo) by default.
#
#   bash packaging/install-desktop.sh           # build (release) + install for current user
#   bash packaging/install-desktop.sh --no-build # install an already-built binary
#   bash packaging/install-desktop.sh --uninstall
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_NAME="envctl-gui"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"

uninstall() {
  rm -f "$BIN_DIR/$BIN_NAME" \
        "$APP_DIR/$BIN_NAME.desktop" \
        "$ICON_DIR/$BIN_NAME.svg"
  echo "Removed $BIN_NAME desktop integration."
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APP_DIR" || true
  exit 0
}

BUILD=1
for arg in "$@"; do
  case "$arg" in
    --uninstall) uninstall ;;
    --no-build)  BUILD=0 ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

if [[ "$BUILD" == 1 ]]; then
  ( cd "$REPO_ROOT" && cargo build -p "$BIN_NAME" --release )
fi

SRC_BIN="$REPO_ROOT/target/release/$BIN_NAME"
[[ -x "$SRC_BIN" ]] || { echo "missing binary: $SRC_BIN (build first, or drop --no-build)" >&2; exit 1; }

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"
install -m 0755 "$SRC_BIN" "$BIN_DIR/$BIN_NAME"
install -m 0644 "$REPO_ROOT/packaging/$BIN_NAME.svg" "$ICON_DIR/$BIN_NAME.svg"
install -m 0644 "$REPO_ROOT/packaging/$BIN_NAME.desktop" "$APP_DIR/$BIN_NAME.desktop"

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APP_DIR" || true
command -v gtk-update-icon-cache  >/dev/null 2>&1 && \
  gtk-update-icon-cache -f -t "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" >/dev/null 2>&1 || true

echo "Installed:"
echo "  binary  -> $BIN_DIR/$BIN_NAME"
echo "  launcher-> $APP_DIR/$BIN_NAME.desktop"
echo "  icon    -> $ICON_DIR/$BIN_NAME.svg"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "note: $BIN_DIR is not on PATH";; esac
