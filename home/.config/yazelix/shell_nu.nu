# Yazelix-managed Nushell hook
# Add Nushell-only commands for Yazelix sessions here

# n8n docker boot helpers (added by envctl) — work inside yazelix/nushell
def n8n-up [...rest] { ^/home/drdave/.local/bin/n8n-up ...$rest }
def n8n-down [...rest] { ^/home/drdave/.local/bin/n8n-down ...$rest }

# === rtk (Rust Token Killer) auto-routing ================================
# Defs live in a shared module so yazelix sessions and standalone login
# nushell stay in sync. See that file for coverage limits and rationale.
source "/home/drdave/.config/nushell/rtk-wrappers.nu"
# =========================================================================

# === rtk monitor pane (live coverage + savings) ==========================
# `rtk-mon` opens it on demand; it also auto-opens ONCE per zellij session.
# Opt out: set $env.RTK_MONITOR_AUTOSTART = "0" before nu starts.
def rtk-mon [] { ^zellij run --name rtk --direction down -- /home/drdave/.local/bin/rtk-monitor }
if ("ZELLIJ_SESSION_NAME" in $env) and (($env.RTK_MONITOR_AUTOSTART? | default "1") != "0") {
    let marker = $"/tmp/rtk-monitor-($env.ZELLIJ_SESSION_NAME).lock"
    if not ($marker | path exists) {
        touch $marker
        do { ^zellij run --name rtk --direction down -- /home/drdave/.local/bin/rtk-monitor } | complete | ignore
    }
}
# =========================================================================
