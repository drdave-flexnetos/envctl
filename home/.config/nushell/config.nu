# Standard nushell config. Loaded by login nushell (`nu -l`, `nu -l -c`) and
# interactive non-yazelix nu. NOT loaded by bare `nu -c` (nushell loads no
# config in that mode) and NOT by yazelix sessions (they use an explicit
# --config; see ~/.config/yazelix/shell_nu.nu which sources the same module).
source "/home/drdave/.config/nushell/rtk-wrappers.nu"
