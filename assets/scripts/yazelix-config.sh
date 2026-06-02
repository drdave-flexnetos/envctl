#!/usr/bin/env bash
# =============================================================================
# yazelix-config.sh   (OPTIONAL manual helper — not run during install)
#
# Normally you do NOT need this: yazelix seeds its own full default settings
# ("Runtime packages seed new configs from settings_default.jsonc") on first
# launch, with every option present. Letting yazelix do that is the
# no-conflict path, so the installer does not pre-write the config.
#
# This helper only RESTORES that canonical default if settings.jsonc ever goes
# missing or you want to reset it. It makes no value edits of its own.
#
# Baseline source (in priority order):
#   1. yazelix's own shipped settings_default.jsonc from the installed runtime
#      (always version-correct — preferred; identical to what yazelix seeds)
#   2. the embedded copy below (yazelix v17.3 default) only if (1) isn't found
#
# Behaviour:
#   - no settings.jsonc yet  -> restore the canonical default (creates it)
#   - settings.jsonc exists  -> leave it untouched (yazelix's seed / your edits
#                               are preserved); pass --force to reset to the
#                               canonical default (a timestamped backup is kept)
#
# Usage:  yazelix-config.sh [--force]
# =============================================================================
set -uo pipefail

CFG_DIR="$HOME/.config/yazelix"
CFG="$CFG_DIR/settings.jsonc"
FORCE="${1:-}"
mkdir -p "$CFG_DIR"

say() { printf '  %s\n' "$*"; }

# --- 1. Locate the version-correct shipped default --------------------------
find_default() {
  local c
  for c in \
    "$HOME/.local/share/yazelix/settings_default.jsonc" \
    "$(readlink -f "$HOME/.nix-profile" 2>/dev/null)/share/yazelix/settings_default.jsonc"; do
    [ -f "$c" ] && { echo "$c"; return 0; }
  done
  c="$(find -L "$HOME/.nix-profile" "$HOME/.local/share/yazelix" \
        -name settings_default.jsonc 2>/dev/null | head -n1)"
  [ -n "$c" ] && { echo "$c"; return 0; }
  return 1
}

BASE="$(mktemp)"; trap 'rm -f "$BASE"' EXIT
if SRC="$(find_default)"; then
  cp "$SRC" "$BASE"; say "Baseline: yazelix shipped default ($SRC)"
else
  say "Baseline: embedded v17.3 default (shipped default not found)"
  cat > "$BASE" <<'JSONC'
// Yazelix settings.
// Edit your active ~/.config/yazelix/settings.jsonc directly or use `yzx config`.
// Runtime packages seed new configs from settings_default.jsonc.
{
  "core": {
    "debug_mode": false,
    "skip_welcome_screen": false,
    "show_macchina_on_welcome": true,
    "welcome_style": "random",
    "game_of_life_cell_style": "full_block",
    "welcome_duration_seconds": 4.0
  },
  "helix": {
    "external": null,
    "steel_plugins": {
      "enabled": [
        "splash",
        "spacemacs_theme"
      ],
      "extra": []
    }
  },
  "editor": {
    "command": "",
    "hide_sidebar_on_file_open": false
  },
  "workspace": {
    "left_sidebar": {
      "command": "yzx",
      "args": [
        "sidebar",
        "yazi"
      ],
      "width_percent": 20
    },
    "right_sidebar": {
      "command": "codex",
      "args": [],
      "width_percent": 40
    }
  },
  "shell": {
    "default_shell": "nu"
  },
  "terminal": {
    "terminals": [
      "ghostty",
      "wezterm"
    ],
    "config_mode": "yazelix",
    "transparency": "medium"
  },
  "zellij": {
    "disable_tips": true,
    "pane_frames": true,
    "rounded_corners": true,
    "support_kitty_keyboard_protocol": false,
    "theme": "default",
    "widget_tray": [
      "editor",
      "shell",
      "term",
      "cursor",
      "codex_usage",
      "cpu",
      "ram"
    ],
    "tab_label_mode": "full",
    "claude_usage_display": "both",
    "codex_usage_display": "quota",
    "opencode_go_usage_display": "both",
    "codex_usage_periods": [
      "5h",
      "week"
    ],
    "opencode_go_usage_periods": [
      "5h",
      "week",
      "month"
    ],
    "claude_usage_periods": [
      "5h",
      "week"
    ],
    "custom_text": "",
    "popup_program": [
      "lazygit"
    ],
    "popup_commands": {
      "bottom_popup": [
        "lazygit"
      ],
      "top_popup": [
        "yzx",
        "config",
        "ui"
      ],
      "menu": [
        "yzx",
        "menu"
      ],
      "btm": [
        "btm"
      ]
    },
    "popup_width_percent": 90,
    "popup_height_percent": 90,
    "screen_saver_enabled": false,
    "screen_saver_idle_seconds": 300,
    "screen_saver_style": "random",
    "default_mode": "normal",

    // Curated native Zellij key policy used by Yazelix.
    // Set any entry to [] to disable that one bind/unbind. Arbitrary native
    // Zellij keymaps still belong in ~/.config/yazelix/zellij.kdl.
    "native_keybindings": {
      "move_tab_left_unbind": [ "Alt i" ],
      "move_tab_left": [ "Ctrl Alt H" ],
      "move_tab_right_unbind": [ "Alt o" ],
      "move_tab_right": [ "Ctrl Alt L" ],
      "new_pane_unbind": [ "Alt n" ],
      "go_to_tab_1": [ "Alt 1" ],
      "go_to_tab_2": [ "Alt 2" ],
      "go_to_tab_3": [ "Alt 3" ],
      "go_to_tab_4": [ "Alt 4" ],
      "go_to_tab_5": [ "Alt 5" ],
      "go_to_tab_6": [ "Alt 6" ],
      "go_to_tab_7": [ "Alt 7" ],
      "go_to_tab_8": [ "Alt 8" ],
      "go_to_tab_9": [ "Alt 9" ],
      "toggle_focus_fullscreen": [ "Alt Shift F" ],
      "previous_tab": [ "Alt q" ],
      "next_tab": [ "Alt w" ],
      "move_pane_down": [ "Ctrl Alt J" ],
      "move_pane_up": [ "Ctrl Alt K" ],
      "selection_cycle_unbind": [ "Alt (", "Alt )" ],
      "toggle_pane_in_group_unbind": [ "Alt p" ],
      "toggle_pane_in_group": [ "Ctrl Alt p" ],
      "toggle_group_marking": [ "Ctrl Alt Shift P" ],
      "locked_mode_unbind": [ "Ctrl g" ],
      "locked_mode": [ "Ctrl Alt g" ],
      "scroll_mode_unbind": [ "Ctrl s" ],
      "scroll_mode": [ "Ctrl Alt s" ],
      "session_mode_unbind": [ "Ctrl o" ],
      "session_mode": [ "Ctrl Alt o" ],
      "tmux_mode_unbind": [ "Ctrl b" ]
    },

    // Semantic remaps for Yazelix-owned Zellij actions.
    "keybindings": {
      "open_workspace_terminal": [ "Alt m" ],
      "popup": [],
      "bottom_popup": [ "Alt Shift J" ],
      "top_popup": [ "Alt Shift K" ],
      "menu": [ "Alt Shift M" ],
      "btm": [ "Alt Shift B" ],
      "config": [ "Alt Shift C" ],
      "move_focus_left_or_tab": [ "Alt h", "Alt Left" ],
      "move_focus_right_or_tab": [ "Alt l", "Alt Right" ],
      "toggle_editor_sidebar_focus": [ "Ctrl y" ],
      "toggle_editor_right_sidebar_focus": [ "Ctrl Shift Y" ],
      "toggle_left_sidebar": [ "Alt Shift H" ],
      "open_codex_agent_right": [ "Alt Shift L" ],
      "smart_reveal": [ "Alt r" ],
      "previous_family": [ "Alt [" ],
      "next_family": [ "Alt ]" ]
    }
  },
  "yazi": {
    "command": "",
    "ya_command": "",
    "plugins": [
      "git",
      "starship"
    ],
    "theme": "default",
    "sort_by": "alphabetical",
    "keybindings": {
      "open_directory_as_workspace_pane": [ "<A-p>" ],
      "open_zoxide_in_editor": [ "<A-z>" ]
    }
  }
}
JSONC
fi

# --- 2. Install (only if missing) / optionally reset the active config -------
# No value edits are made; the file is the canonical yazelix default verbatim.
if [ -f "$CFG" ] && [ "$FORCE" != "--force" ]; then
  say "settings.jsonc already present — left untouched (use --force to reset to the canonical default)."
else
  if [ -f "$CFG" ]; then
    BK="$CFG.bak.$(date +%Y%m%d%H%M%S)"; cp "$CFG" "$BK"; say "Backed up existing -> $BK"
  fi
  cp "$BASE" "$CFG"; say "Wrote canonical yazelix settings.jsonc (all items present, verbatim)."
fi

say "Done. Active config: $CFG  (edit it yourself: hx $CFG)"
