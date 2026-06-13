# Standard nushell config. Loaded by login nushell (`nu -l`, `nu -l -c`) and
# interactive non-yazelix nu. NOT loaded by bare `nu -c` (nushell loads no
# config in that mode) and NOT by yazelix sessions (they use an explicit
# --config; see ~/.config/yazelix/shell_nu.nu which sources the same module).
# Relative source: nushell resolves `source` against the directory of this
# file, so the sibling module loads regardless of $HOME (portability: no
# hardcoded path). Was an absolute /home/drdave path before ADR-0006 wave 2.
source rtk-wrappers.nu
