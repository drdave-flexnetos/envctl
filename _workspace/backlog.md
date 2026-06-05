# env-install-loop backlog — 2026-06-05T22:03Z (resume / re-discover)

Source of truth for the provisioning loop. Rebuilt from REAL state on a fresh worktree off
`origin/master` (tip `fcf3d0c`, PR #24 develop→master merged; manifest now **49 components**):
`envctl doctor` (all toolchains green; only `/etc` not writable = sudo territory) +
`auto-detect --json` (39 detected+healthy, 8 detected/no-verify groups, **2 missing in drift**).

Prior env-install-loop reached DONE at 45–46/46 (cycles_total 18). Since then the dashboard +
grit components landed on master (46→49 declared). Re-discovery shows the box is healthy except
the two drift items below. The committed `_workspace/HANDOFF.md` on master belongs to the
*forge-loop* (dashboard wire-live), NOT this loop — out of scope here.

Legend: `- [ ]` todo · `- [x]` installed+verified · `- [!]` blocked (reason).

## Done this session
- [x] grit — INSTALLED + verified (grit 0.3.0 on PATH at ~/.cargo/bin/grit, fresh login shell;
      detect healthy; install idempotent "skip already present"; drift cleared). Two blockers
      found and fixed the rust-native / declared way:
      1. meta-root Cargo.toml excluded agent/envctl/lane but NOT grit → "package believes it's
         in a workspace when it's not". FIXED: added "grit" to `exclude` in
         /home/drdave/Desktop/meta/Cargo.toml (sibling-repo pattern; grit MUST NOT be a member —
         it links C SQLite, would break no-c.sh). [meta parent repo]
      2. grit's build needs OpenSSL dev headers (openssl-sys via grit's aws/azure SDKs); only the
         OpenSSL runtime was present. User AUTHORIZED `sudo apt-get install -y libssl-dev
         pkg-config`. CODIFIED: new `libssl-dev` component in apt-base.toml (needs_sudo;
         detect/verify via `pkg-config --exists/--modversion openssl`) + added to grit `requires`.
         no-c.sh still PASS (libssl-dev is a system pkg, grit is excluded from the envctl graph).
      Re-locked: envctl.lock 49→50 components, `lock --check` clean.

- [!] libssl-dev component install via `envctl install libssl-dev` emits a harmless
      "sudo: A terminal is required to authenticate / could not pre-authorize sudo" warning when
      run without a TTY, but since libssl-dev is already present it no-ops correctly. NOTE for a
      truly fresh box: the needs_sudo apt install requires a TTY / pre-cached sudo to actually
      run (same privilege-broker constraint as every other apt-base component). Not a blocker here.

## Blocked / by-design (NOT loop actions — surfaced for the human)
- [!] node-via-bun — TABLED by design (carried from prior loop). Bun's `node` shim cannot do
      `node --version` BY DESIGN, so the verify hook can never pass; n8n needs REAL node (v22
      installed, in-range). `reset node-via-bun` is REFUSED by the fail-closed group-ai-clis
      reverse-dep guard. Box is functionally correct; this is cosmetic detect-drift only.
      Manifest design follow-up (NOT a loop action): mark not-applicable when a real Node in
      n8n's range is present, or add a `node-real` component + drop the group-ai-clis edge.

## Notes
- The 8 detected/healthy=None components are aggregator groups / no-verify-hook comps
  (group-boot-repair, group-ai-clis, group-nix-yazelix, group-gpu-stack, boot-repair-diagnose,
  keepassxc, meta-base-sanity, yazelix-shell) — detected=True; healthy=None just means no
  explicit health probe. Not gaps.
- `dashboard` component detects healthy (not in drift) — already wired on this box.
