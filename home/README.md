# home/ — the canonical home tree (ADR-0006)

This directory is the **single source of truth for user-global, non-secret configuration** on a
FlexNetOS workstation. The portability principle (locked 2026-06-12): *real file in meta, symlink
outside, never the reverse* — `$HOME` paths are symlinks into this tree, wired by the
`portability-links` components (`manifest/components.d/portability-links.toml`).

```
$HOME/.gitconfig                    -> envctl/home/.gitconfig
$HOME/.claude/settings.json         -> envctl/home/.claude/settings.json     (claude-global-links)
$HOME/.config/yazelix/settings.jsonc-> envctl/home/.config/yazelix/...       (home-config-links)
$HOME/.config/systemd/user/*.service-> envctl/home/.config/systemd/user/...  (home-config-links)
$HOME/.local/bin/<tool>             -> ~/Desktop/meta/<repo>/target/release/<tool>  (meta-tool-links)
```

## Rules (review gates — this repo is PUBLIC)

1. **No secrets, ever.** Credentials delegate outward (`.gitconfig` uses `gh auth git-credential`;
   `~/.claude/.credentials.json`, `~/.config/gh/hosts.yml`, keyrings are NEVER added). The envctl
   secrets stack / relay is the sanctioned channel for secret material.
2. **No state.** Histories, caches, sessions, `vox.db`, piper voices, `~/.local/share/*` stay
   machine-local; bootstrap regenerates them.
3. **Archive-first.** The wiring components move any pre-existing real file to
   `~/Desktop/_archives/home-links-<date>/` before linking — originals are never deleted.
4. **Every file is reviewed individually** before it lands here (no bulk `cp -r` of live dirs).

## Layering

- **envctl** (this repo) = OS/toolchain/box layer — owns this tree and the symlink wiring.
- **kasetto** = agent layer (skills/MCP into `.claude`/`.codex`) — its *global manifest* lives here
  (`.config/kasetto/kasetto.yaml`) but its outputs are kasetto-managed, not tree-linked.
- **meta** = repo/workspace layer — `meta/scripts/bootstrap.sh` sequences rustup → clone → build →
  `envctl install` → `kasetto sync --locked` → `envctl doctor && envctl lock --check`.

## Known portability residue (v1, recorded honestly)

- Absolute `/home/drdave/...` paths remain inside `settings.json` (statusline, plugin marketplaces),
  `nushell/config.nu` (source line), and `yazelix/shell_bash.sh` (rtk-monitor pane) — they work on
  this box; a template/substitution pass at link time is the follow-up (tracked in
  PORTABILITY-AUDIT.md at the meta root).
- `repowire.service` is carried for the record but disabled on the box (binary missing — see header).
