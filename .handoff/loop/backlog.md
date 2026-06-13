# Loop backlog — envctl agenticOS consolidation (2026-06-12)

> Source: owner directive (2026-06-12) — handoff full-sync + meta-portability + **kasetto
> full-feature unification into envctl** (no downgrades, no feature lost, upgrade-only), plus the
> follow-up work surfaced when consolidating all WIP branches into `develop`. Design + research
> cross-references: `.handoff/decisions/ADR-0001-kasetto-handoff-portability-unification.md`.
>
> Workflow: `develop` is the integration branch; `master` is its protected mirror (auto-synced).
> Each item below is picked up in a FRESH worktree off `develop` (`git worktree add … -b <slug> develop`).
>
> Legend: `- [ ]` todo · `- [x]` done · `- [!]` blocked (reason) · `- [?]` needs investigation
> · `- [!!]` SUPERVISED/CRITICAL (never auto-run).

## North star

envctl is the **agenticOS**: it owns the meta environment boundary (PATH, dotfiles, `~/.local`,
the canonical `home/` tree), holds + auto-injects secrets, provisions the agent environment, and
carries the handoff continuity kernel. Everything meta consumes resolves inside meta; user-global
holds ONLY symlinks into meta; configs reference meta via PATH (bare names) or `$META_ROOT` (from
the `.meta.yaml` marker) — never hardcoded paths. **HEAL not harm · NEVER delete (archive) ·
NEVER downgrade (sync meta source UP first) · pure-Rust, no C in the trust boundary.**

---

## Epic A — Handoff continuity full-sync (bring `.handoff` to Tier-A)

Research: `meta/handoff` kernel vs `envctl/.handoff` (~30% Tier-B stub). Per-repo `.handoff` holds
git-committed TEXT ONLY; events flow to the shared `meta/.handoff/ledger.db` (ADR-0004). Packets
are **rendered by `hf`, never hand-written**.

- [x] **TASK-0001 (P0):** Build & install the `hf` kernel binary from `meta/handoff` (not on PATH
  today — keystone blocker). Relocate per Epic B procedure (symlink into meta). Verify
  `hf resume/claim/checkpoint/handoff` run from envctl against `meta/.handoff/ledger.db`.
  - DONE 2026-06-13 (forge-loop cycle 1, `handoff-kernel-engineer` agent + `handoff-sync` Step 1).
    Built `cargo build --release -p hf` (3.6 MB ELF); installed `~/.local/bin/hf` → SYMLINK into
    `meta/handoff/target/release/hf` (meta convention; rebuilds propagate). `which hf` resolves the
    meta symlink; `hf --help` runs (verbs: init|seed|status|session|claim|release|checkpoint|
    sync-cards|done|task mint|ship|review|handoff|resume — no `hf drift`/`hf policy`). Residency
    guard PASSES before+after: no per-repo `ledger.db` under any envctl tree; `hf status` from
    `$META_ROOT` reads the shared `meta/.handoff/ledger.db` read-only (md5 unchanged).
  - GO-LIVE for the wired-but-DORMANT continuity hook: `.claude/settings.json` +
    `.claude/hooks/hf-checkpoint.sh` are already wired (Stop + PreCompact, fleet-ledger-resident,
    self-resolves `$META_ROOT`) but no-op until `hf` exists + supports `checkpoint --auto --quiet`.
    Acceptance: after `hf` lands, a Stop fires `hf checkpoint --auto` writing a witnessed event to
    `$META_ROOT/.handoff/ledger.db` (NOT a per-repo ledger), proving "auto-update .handoff after
    every task" (the `/verify` 2026-06-13 finding — currently FALSE, this makes it TRUE).
    - HOOK NOW LIVE: fired the Stop hook with `CLAUDE_PROJECT_DIR`=envctl worktree → exit 0,
      resolves `hf` via PATH, runs `hf checkpoint --auto --quiet` from `$META_ROOT`, creates NO
      per-repo ledger. The witnessed-event WRITE is correctly a no-op today (`hf checkpoint --auto`
      → "no task id … `--auto` with an active task"; 0 cards seeded). **End-to-end witnessed-event
      proof therefore defers to TASK-0002** (which seeds + mints + claims a task) — correct
      dependency ordering, not a regression. Hook go-live (resolve+run+residency-safe) = DONE here.
  - NOTE (carried → Open findings / Epic A): the `hf` binary's `ledger` crate pulls
    **`rusqlite`/`libsqlite3-sys` (bundled C SQLite, statically linked)**. Does NOT violate
    envctl's `no-c.sh` (separate `meta/handoff` workspace, not an envctl crate), but is relevant to
    Epic A's "pure-Rust, no C in the trust boundary" north star if the kernel itself must be C-free.
- [x] **TASK-0002 (P0):** Seed envctl `.handoff` via `hf` — render `policy.toml`, `hooks/hooks.toml`,
  `policies/rules.toml`, `active.md`, `packets/latest.md`, `skills/`. Do NOT create a per-repo
  `ledger.db`; do NOT hand-write packets.
  - **DONE 2026-06-13 (resume cycle 4).** Per ADR-0004 §7 the Tier-A layer is authored as git-text
    (NOT via `hf init`/`hf seed` — both would plant a per-repo `ledger.db` / irrelevant HFTASK cards;
    kernel-source verified). Landed: refreshed `context/capsule.json` `next_command`; seeded the
    OPTIONAL autonomous-loop descriptors `hooks/hooks.toml` + `policies/rules.toml` +
    `skills/session-resume.skill.md` from the design-bundle templates (residency-safe text, with a
    `$META_ROOT`-residency header so ledger-mutating verbs never run in-member); **compiled**
    `packets/latest.md` via `hf fleet render envctl` (rendered, not hand-written); corrected
    `.handoff/README.md` (FLEET ledger = `meta/.handoff/ledger.db`, member packets via `hf fleet
    render`, active loop). Residency verified: 0 `*.db` under `.handoff`, `.gitignore`
    `.handoff/**/ledger.db` guard present, `hf fleet status` shows envctl with **no `⚠ stray
    ledger.db (P7)`**. Gates: no-c/shape/enable PASS; drift test green. `tasks/` stays empty — cards
    are minted via `hf task mint --from-kb` once kb task docs exist for envctl (packet degrades to
    "no open cards"); that + the `hf sync` `.kb` GO-LIVE (run at `$META_ROOT`, never in-member) are
    follow-ups, tracked under TASK-0003.
  - **BLOCKED 2026-06-13 (cycle 2, REVISED): installed `hf` is the S1 spike, missing the fleet
    verbs → NEEDS-DECISION.** The design is SETTLED (ADR-0004 §2/§3/§4 + PRD v2): per-repo
    `.handoff/` is **text-only, no `ledger.db`**; events live in the **fleet** ledger
    (`meta/.handoff/ledger.db` — cycle-1's target was correct; `meta/handoff/.handoff/ledger.db` is
    the separate KERNEL ledger w/ 23 HFTASK cards); per-repo packets/cards are joined centrally via
    **`hf fleet status`**. The blocker: the installed binary (S1 spike) lacks **`fleet`/`policy`/
    `drift`/`sync`** (only `sync-cards`), which ADR-0001 §22 documents and ADR-0004 §76 cards as
    "to implement" (HFTASK-0007 `session`+`policy.toml`, HFTASK-0011 `hf sync` `.kb` mirror, plus
    `hf fleet status` + fleet-aware packet render). Fix = **build those verbs in `meta/handoff`**
    (kernel scope), then re-run TASK-0002. NOTE: envctl's REQUIRED Tier-A text core
    (`context/capsule.json`+`README`+`tasks/`+`packets/`) already exists; only OPTIONAL
    `hooks/policies/skills` (residency-safe, no kernel dep) + the rendered/minted/synced parts
    remain. v1's "add a `--ledger`/`HANDOFF_DIR` flag" is RETRACTED. Full analysis + 3 options
    (A: build kernel fleet verbs [recommended]; B: seed the text subset now, defer the rest;
    C: rescope to required-text-core + central `hf fleet` render) →
    `.handoff/decisions/FINDING-0002-hf-ledger-residency-vs-repo-tier-a.md`.
  - **UNBLOCKED 2026-06-13 (resume, owner "check now"): FINDING-0002 RESOLVED via Option A.** The
    kernel built the fleet verbs — `meta/handoff` PR **#17** (`feat: fleet verbs hf fleet
    status/render, hf sync`); installed `hf` rebuilt 2026-06-13 04:29. Verified live from `$META_ROOT`:
    `hf fleet status` (fleet ledger present, 64 members enumerated), `hf fleet render envctl` (wrote
    `packets/latest.md`), `hf sync --dry-run` (one-way `.kb` mirror). TASK-0002 is now executable as
    written. Next Epic A cycle: seed the OPTIONAL `hooks/policies/skills` text + run
    `hf fleet render envctl` / `hf sync` properly inside a worktree cycle and commit the artifacts.
- [x] **TASK-0003 (P1) — DONE 2026-06-13 (cycle 6): `p7-conformance` gate landed.** Added
  `ci/gates/p7.sh`, a fail-closed grep-based bash gate mirroring `ci/gates/{shape,enable}.sh`
  (dependency-free; validates the COMMITTED Tier-A, never runs a ledger-mutating `hf` verb in-member).
  - **Schema validation ✓:** asserts the `schema` tag on `context/capsule.json`
    (`handoff.context_capsule.v1`), `policies/rules.toml` (`handoff.policy.rules.v1`),
    `hooks/hooks.toml` (`handoff.hooks.v1`), every `tasks/*.task.json` (`handoff.task.v1`), and that
    `packets/latest.md` (the `hf fleet render` artifact) is a `handoff.packet.v2`.
  - **Residency invariant ✓:** asserts **no per-repo `ledger.db`** is git-tracked OR present on disk
    under `.handoff`, and that `.gitignore` carries the `.handoff/**/ledger.db` guard. Fail-closed.
  - **Wired** into the loop verify-on-resume (`.handoff/loop/HANDOFF.md`) + `CLAUDE.md` gate list.
    Verified: positive PASS on the seeded Tier-A; negative tests (stray `*.db`, bad packet/capsule
    schema) all fail closed (exit 1). GO-LIVE + card-minting split to **TASK-0024**.
- [ ] **TASK-0024 (P2, Epic A) — `hf sync` `.kb` GO-LIVE + envctl card-minting** (split from TASK-0003):
  - **GO-LIVE for `.handoff`↔`.kb` auto-sync (run at `$META_ROOT`, NEVER in-member — `hf sync` opens a
    CWD-relative ledger):** verify the kernel's `hf sync` one-way `.kb` write-back (ADR-0003
    HFTASK-0011) so the loop's `.handoff` cards/checkpoints mirror into GitKB. The broken `.kb`
    SessionStart hook is already FIXED (`meta/.claude/settings.json`: `git kb service` → guarded
    `git kb serve`, meta main `bf68d57`). Acceptance: `hf sync` reflects a checkpoint into `.kb` (the
    `/verify` finding's "auto-sync to .handoff and .kb" becomes TRUE).
  - **Card-minting:** populate `envctl/.handoff/tasks/` via `hf task mint --from-kb <slug>` once kb
    task docs exist for the envctl backlog (today `tasks/` is empty; packet degrades to "no open
    cards"). Reference: `meta/handoff/FLEET_GUIDE.md` for verb usage. CAUTION: use the installed `hf`;
    do not build/commit in `meta/handoff` while a kernel session may be active.

## Epic B — Meta-portability / env-ownership (`$META_ROOT`)

`~/.local/bin` must hold ONLY symlinks into meta. Per-tool relocation procedure: (1) confirm
provenance, (2) build meta source `--release`, (3) **if meta < installed → UPGRADE meta source
FIRST** (never relocate to older), (4) smoke-test, (5) archive installed copy (timestamped, never
delete), (6) symlink `~/.local/bin/<tool>`→meta build, (7) re-verify + ROLLBACK on failure, (8)
verify env health.

- [x] `envctl env` — discover meta-root via `.meta.yaml` marker (`engine::dashboard::locate_meta_file`),
  emit `export META_ROOT=…` + meta tool dirs on PATH; `--toolchains`/`--materialize` (merged from
  feat/envctl-env, 2026-06-12).
- [x] **TASK-0004 (P0):** Wire `META_ROOT` into the env Claude inherits (login/session env envctl owns).
  - DONE 2026-06-13 (resume cycle): added a top-level `"env": { "META_ROOT", "META_FILE" }` block to
    `home/.claude/settings.json.tmpl` (rendered per-machine to absolute paths by the existing
    `claude-global-links` `sed` render — the same path TASK-0005 uses); re-rendered the committed
    `settings.json`. Claude Code applies settings `env` to every session, so every repo+meta session
    now inherits `META_ROOT`/`META_FILE` with no hardcoding. Added a Rust drift-guard test
    (`settings_json_matches_rendered_tmpl_no_drift`) asserting `settings.json == render(tmpl, root)`
    + the env-block wiring (host-independent via the statusline anchor) — a guard that did not exist
    before. Gate green: build (395 crates), `cargo test -p envctl` 7 pass, no-c/shape/enable PASS.
- [x] **TASK-0005 (P1):** Heal the 3 hardcoded `home/.claude/settings.json` refs via `$META_ROOT`/
  per-machine templating: statusline script + 2 plugin-marketplace dirs (HIGH — live source-of-truth file).
  - DONE 2026-06-13: `home/.claude/settings.json.tmpl` + `claude-global-links` per-machine render
    (byte-identical, non-breaking). PR **envctl#37 MERGED → develop** (`bf29acd`). (Git>backlog: confirmed merged.)
- [ ] **TASK-0006 (P2):** Point global `home/.config/kasetto/kasetto.yaml` mcps source at in-meta
  agent-skills (not `github.com/FlexNetOS/agent-skills`); genericize MED shell/nushell hardcodes
  (`shell_nu.nu`, `shell_bash.sh`, `config.nu`). Fix stale `Documentation=` URL in `manifest/env-ctl.toml`.
- [ ] **TASK-0007 (P2):** `envctl doctor`/env boundary-refusal when a real FlexNetOS install is found
  outside meta; idempotent `~/.local/bin` symlink regen from `META_ROOT`.
- [ ] **TASK-0008 (P2):** Relocate **meta-mcp** → `meta/meta_mcp` (lowest risk; first proof of procedure).
- [!] **TASK-0009 (P2):** Relocate **kasetto + kst** — superseded by Epic C (kasetto becomes built-in;
  no external binary to relocate once absorbed). Until C lands: meta source v3.0.0 < installed v3.1.0.
- [x] **TASK-0010 (P2):** Relocate **rtk + rtk-monitor** — DONE 2026-06-13 (human-supervised session,
  per rtk-tokenkill weave report). `FlexNetOS/rtk-tokenkill#1` (sync upstream 0.42.4, rusqlite 0.40 kept)
  MERGED → develop; rtk built canonically → `meta/target/release/rtk`; `~/.local/bin/rtk` now a SYMLINK
  into meta (0.42.4); live hook verified; old 0.42.2 archived; meta `Cargo.lock` locked to 0.42.4.
  (Was `- [!!]` SUPERVISED — correctly NOT auto-run by the loop; resolved by a human, as designed.)

## Epic C — Kasetto full-feature unification into envctl (no downgrade)

kasetto is already pure-Rust + passes no-c gate (only drop `mimalloc`). envctl already ported §2
lock / §16 runtime / doctor / lock --check. Absorb the rest as a pure-Rust crate. NO-DOWNGRADE
checklist in ADR-0001 (all 11 verbs incl v3.1 add/remove/lock; 6-key+extends schema; 21-agent
preset; multi-host resolver; 5 cmd + 4 MCP-merge additive transforms; 3 lock modes).

- [ ] **TASK-0011 (P1):** Refresh `docs/KASETTO-FEATURES.md` to v3.2.0 (full verb/schema inventory +
  no-downgrade checklist; current doc is stale at v3.0.0).
- [ ] **TASK-0012 (P0 of C):** New pure-Rust crate `crates/agent-env` — config model (6 keys +
  `extends`), multi-host source resolver, SHA-256 hash, lock. Drop `mimalloc`. no-c gate clean.
- [ ] **TASK-0013:** Engine `agent_env` module + Engine methods + Events (agent_sync/add/remove/lock);
  non-printing, front-end parity.
- [ ] **TASK-0014:** CLI verbs `envctl agent {sync,add,remove,lock,list,clean}` (--dry-run/--json/--locked)
  + GUI parity.
- [ ] **TASK-0015:** Provisioning fidelity — verbatim skill copy; 5 command-format transforms; 4
  MCP-merge formats (ADDITIVE, never-clobber — must preserve global broker/repowire/weave servers).
- [ ] **TASK-0016:** Lock unification — fold agent assets into `envctl.lock` (SHA-256 section) or keep
  kasetto.lock owned by the subsystem; reframe `manifest/agent-env.toml` external-binary → built-in.
- [ ] **TASK-0017:** Adopt kasetto `extends` config composition for envctl component manifests.
- [ ] **TASK-0018:** Retire the external `kasetto` binary dependency — only after the no-downgrade
  checklist passes end-to-end.

## Epic D — Follow-ups surfaced from the WIP-branch consolidation (2026-06-12)

All WIP branches were merged to develop + verified green (build, 197 tests, no-c/shape/enable,
fmt, clippy). Remaining follow-ups extracted from each:

- [ ] **TASK-0019 (fix-secretd):** U1 USB-unlock path needs a real `RealUsbProbe` (crash-loop +
  durable store + passphrase path already fixed/merged). See `.handoff/loop/_done/secretd-provisioning-runbook.md`.
- [ ] **TASK-0020 (github-app-mint):** Wire the GitHub-App token mint (`secrets-engine/mint_github.rs`,
  merged) through the `ProviderMint` injection seam — secretctl/secretd phases + agent-env auto-inject.
- [ ] **TASK-0021 (node-via-bun):** Manifest design follow-up — mark node not-applicable when a real
  node in the n8n range is present, or add a `node-real` component + drop the group-ai-clis edge
  (cosmetic detect-drift only; truth-telling fix already merged).
- [ ] **TASK-0022 (agent-web-access):** Phases 2–3 of the agent web-access ladder (Phase 1 n8n-mcp +
  kasetto wiring merged). `- [!]` n8n live smoke test is HUMAN-ONLY (see
  `.handoff/loop/_done/n8n-live-smoke-runbook.md`).

## Epic E — Workflow infrastructure

- [ ] **TASK-0023:** develop→master auto-sync GitHub Action (ff master on develop push) +
  enable branch protection on master (PR-only for humans; action token bypass). [in progress 2026-06-12]
- [x] **TASK-0025 (P1, Epic E) — DONE 2026-06-13 (cycle 7): CI required checks on `develop` so
  auto-merge gates fail-closed.** Added `.github/workflows/ci.yml` (4 jobs: **rustfmt · clippy
  (workspace, default features) · test (`--test-threads=1`) · gates (no-c/shape/enable/p7)**) — no
  `--all-features` (mutually-exclusive `remote`/`embedded`). Enabled repo `allow_auto_merge` +
  `develop` branch protection requiring those 4 contexts (strict=false so concurrent sessions aren't
  serialized; no required reviews; admins not enforced). Fixed a real isolation bug TASK-0004 exposed:
  `dashboard::tests::locate_walks_up` leaked the inherited `$META_FILE` → made hermetic. `test` runs
  serial in CI to kill the `XDG_CACHE_HOME`/`$META_FILE` parallel env-race flakiness. Verified all 4
  green locally before requiring them. (Master protection / develop→master mirror = TASK-0023, separate.)

## Key finding (carried)

Most meta-built tools' installed binaries are NEWER than their committed meta sources
(kasetto 3.1.0>3.0.0, rtk 0.42.2>0.42.0) → meta is OUT OF SYNC with what's deployed. The real
work is **sync-meta-source-UP-then-relocate**, not a symlink sweep.
