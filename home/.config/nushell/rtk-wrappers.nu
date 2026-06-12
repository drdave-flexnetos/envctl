# rtk (Rust Token Killer) auto-routing wrappers — single source of truth.
# Sourced from BOTH:
#   - ~/.config/yazelix/shell_nu.nu     (yazelix interactive sessions)
#   - ~/.config/nushell/config.nu       (any login nushell: `nu -l`, `nu -l -c`)
#
# Routes interactive dev-tool commands through `rtk <cmd>` so token-optimized
# output is the default in every nushell context that loads a config.
#
# COVERAGE LIMITS (nushell-inherent, documented honestly):
#   - Bare `nu -c "..."` loads NO config, so wrappers do NOT apply there.
#     Agents that shell out via bare `nu -c` cannot be wired this way.
#   - Claude Code executes commands in a hardcoded `/bin/bash` child (not
#     configurable; see anthropics/claude-code issues #7490, #11475). Claude's
#     rtk coverage is its PreToolUse hook (`rtk hook claude`), NOT this file.
#
# Deliberately NOT wrapped: nushell builtins / structured commands
#   (ls, find, grep, tree, wc, cd) — shadowing breaks `ls | where ...`
#   pipelines and the yazi/starship integrations that depend on them.
# Escape hatch: prefix with `^` to call the real binary raw, e.g. `^git log`.
# Recursion-safe: `rtk git ...` execs the real git binary (external defs are
# invisible to child processes), never these defs.
def --wrapped git           [...rest] { ^rtk git ...$rest }
def --wrapped gh            [...rest] { ^rtk gh ...$rest }
def --wrapped glab          [...rest] { ^rtk glab ...$rest }
def --wrapped gt            [...rest] { ^rtk gt ...$rest }
def --wrapped cargo         [...rest] { ^rtk cargo ...$rest }
def --wrapped go            [...rest] { ^rtk go ...$rest }
def --wrapped pnpm          [...rest] { ^rtk pnpm ...$rest }
def --wrapped npm           [...rest] { ^rtk npm ...$rest }
def --wrapped npx           [...rest] { ^rtk npx ...$rest }
def --wrapped tsc           [...rest] { ^rtk tsc ...$rest }
def --wrapped prettier      [...rest] { ^rtk prettier ...$rest }
def --wrapped jest          [...rest] { ^rtk jest ...$rest }
def --wrapped vitest        [...rest] { ^rtk vitest ...$rest }
def --wrapped playwright    [...rest] { ^rtk playwright ...$rest }
def --wrapped prisma        [...rest] { ^rtk prisma ...$rest }
def --wrapped pip           [...rest] { ^rtk pip ...$rest }
def --wrapped pytest        [...rest] { ^rtk pytest ...$rest }
def --wrapped ruff          [...rest] { ^rtk ruff ...$rest }
def --wrapped mypy          [...rest] { ^rtk mypy ...$rest }
def --wrapped rake          [...rest] { ^rtk rake ...$rest }
def --wrapped rubocop       [...rest] { ^rtk rubocop ...$rest }
def --wrapped rspec         [...rest] { ^rtk rspec ...$rest }
def --wrapped dotnet        [...rest] { ^rtk dotnet ...$rest }
def --wrapped gradlew       [...rest] { ^rtk gradlew ...$rest }
def --wrapped golangci-lint [...rest] { ^rtk golangci-lint ...$rest }
def --wrapped docker        [...rest] { ^rtk docker ...$rest }
def --wrapped kubectl       [...rest] { ^rtk kubectl ...$rest }
def --wrapped aws           [...rest] { ^rtk aws ...$rest }
def --wrapped psql          [...rest] { ^rtk psql ...$rest }
def --wrapped curl          [...rest] { ^rtk curl ...$rest }
def --wrapped wget          [...rest] { ^rtk wget ...$rest }
def --wrapped kimi          [...rest] { ^rtk kimi ...$rest }
def --wrapped ollama        [...rest] { ^rtk ollama ...$rest }
