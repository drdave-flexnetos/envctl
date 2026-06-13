# Loop backlog — env-ownership + Phase-2 tool relocation (runaway-containment mission)

> Source: runaway-session containment mission (2026-06-12). The auto-loop ("deliver 2") works
> this backlog. Prior loop (dashboard forge-loop, terminal-DONE) archived in `_done/`.
>
> Legend: `- [ ]` todo · `- [x]` done · `- [!]` blocked (reason) · `- [?]` needs investigation
> · `- [!!]` SUPERVISED/CRITICAL (never auto-run).

## North star

`~/.local/bin` must hold ONLY symlinks into meta; every FlexNetOS-built tool resolves inside
meta. Configs reference meta via PATH (bare names) or `$META_ROOT` (from the `.meta.yaml`
marker) — never hardcoded paths. HEAL not harm · NEVER delete (archive) · NEVER downgrade
(sync meta source UP first).

## Per-tool relocation procedure (every slice follows this)

1. Confirm provenance (maps to a meta repo/build).
2. Build the meta source `--release`.
3. Version-compare vs the installed copy. **If meta < installed → UPGRADE the meta source to
   ≥ installed FIRST** (sync/port the newer code). Never relocate to an older binary.
4. Smoke-test the meta build (`--version` / `--help` / a real exercise).
5. Archive the installed real copy (timestamped, cold storage) — never delete.
6. Replace `~/.local/bin/<tool>` with a symlink into the meta build.
7. Re-verify via the symlink; **ROLLBACK** (restore the archived copy) on any failure.
8. Verify env health (commands still run) before the next slice.

## Phase 0 — env-ownership build-out (prerequisite; unblocks `$META_ROOT` healing)

- [ ] envctl: add `envctl env` — discover meta-root via the `.meta.yaml` marker (reuse
  `engine::dashboard::locate_meta_file`), emit `export META_ROOT=…` + meta tool dirs on PATH.
  Respect envctl invariants (non-printing engine; print in CLI; clippy -D warnings; CI gates).
- [ ] Wire `META_ROOT` into the env Claude inherits (login/session env that envctl owns).
- [ ] Heal the 3 remaining hardcoded `settings.json` refs via `$META_ROOT` / per-machine
  templating: statusline script + 2 plugin-marketplace dirs.
- [ ] envctl boundary-refusal: `envctl doctor`/env refuses when a real FlexNetOS install is
  found outside meta; idempotently regenerates `~/.local/bin` symlinks from `META_ROOT`.

## Phase 2 — relocation slices (meta-built tools only)

- [ ] **meta-mcp** → `meta/meta_mcp`. Relocatable (meta-built; debug build exists). Build
  release, verify equivalence to installed (Jun-2 copy, 1.7M), then relocate. LOWEST risk
  (not on a universal hook path) — good first proof of the procedure.
- [!] **kasetto + kst** → `meta/kasetto`. BLOCKED: meta source v3.0.0 < installed v3.1.0
  (downgrade). Sync/upgrade `meta/kasetto` source to ≥ v3.1.0 FIRST, build, verify, then
  relocate both (`kst` is the same binary / alias).
- [!!] **rtk + rtk-monitor** → `meta/rtk-tokenkill`. SUPERVISED/CRITICAL — DO NOT auto-relocate:
  (a) on the live `rtk hook claude` PreToolUse path — a broken rtk breaks EVERY command;
  (b) meta source v0.42.0 < installed v0.42.2 (downgrade). Requires: sync rtk-tokenkill to
  ≥0.42.2, build, verify, then swap with an immediate rollback test, ideally from a session
  NOT dependent on the rtk hook. Owner-flagged critical.

## Not relocation targets (leave as-is)

- **git-kb** (v0.2.10): GitKB tool from the upstream `gitkb` org — NOT a `.meta.yaml` project.
  External until/unless a separate decision brings GitKB into meta as a project.
- **forge** (v2.13.4): ForgeCode (forgecode.dev) — third-party commercial AI agent. Vendor.
- Vendor toolchains: uv, uvx, mise, node, npm, npx, bat, fd, fzf, kimi, kimi-cli, claude,
  devin, junie, sqld. Not FlexNetOS source.
- `envctl-dashboard-pane`, `envctl-open-claude`: envctl's own scripts, installed as copies by
  `manifest/dashboard.toml`. Align via the manifest (symlink-or-copy policy), not ad-hoc.

## Key finding

Most meta-built tools' installed binaries are NEWER than their committed meta sources
(kasetto 3.1.0>3.0.0, rtk 0.42.2>0.42.0) → meta is OUT OF SYNC with what's deployed. The
loop's real work is **sync-meta-source-UP-then-relocate**, not a quick symlink sweep. This is
the same "envctl underdeveloped / unsynced with meta" gap the owner flagged.
