# Feature Forge HANDOFF — 2026-06-13T08:00Z (UTC) · refreshed after cycles 1–2

> Loop: **agenticOS-consolidation** (`forge-loop` over `.handoff/loop/backlog.md`, Epics A–E).
> **UPDATE 2026-06-13 (cycles 1–2 landed on develop=master=`8b7e2c8`):** `hf` is now **BUILT +
> INSTALLED** on PATH (`~/.local/bin/hf` → `meta/handoff/target/release/hf`, cycle 1 / TASK-0001).
> The Stop/PreCompact `hf-checkpoint` hook is **LIVE** but its witnessed-event WRITE is a no-op
> until a task is active in the shared ledger (that closure is gated on the now-BLOCKED TASK-0002).
> This file is still the **markdown-fallback cold-start package** — per-repo packets are NOT yet
> hf-rendered (see TASK-0002 blocker below). Read this file + `.handoff/loop/{backlog,loop_state}.md`
> + `.handoff/decisions/FINDING-0002-*.md` to resume cold.
>
> **Epic A is BLOCKED pending an OWNER/KERNEL decision (`FINDING-0002`).** TASK-0001 DONE;
> TASK-0002 (seed Tier-A) + TASK-0003 (p7 gate) `[!]` blocked — the shipped `hf` is strictly
> CWD-relative (no `--ledger`/`HANDOFF_DIR`), so envctl's Tier-A layer can't be hf-rendered against
> the shared meta ledger without a forbidden per-repo `ledger.db`. Fix = a kernel feature in
> `meta/handoff`. **Next unblocked pick = Epic C TASK-0012** (`crates/agent-env`).

## Mission (north star)
Owner directive (2026-06-12): treat **envctl as an agenticOS** — it owns the meta environment
boundary (PATH, dotfiles, `~/.local`, canonical `home/` tree), holds + auto-injects secrets,
provisions the agent env, and carries the handoff continuity kernel. Three pillars: **handoff
full-sync + meta-portability (`$META_ROOT`) + kasetto full-feature unification into envctl
(NO downgrades, no feature lost, upgrade-only)**. Design corpus:
`.handoff/decisions/ADR-0001-kasetto-handoff-portability-unification.md`. Gates: **HEAL not harm ·
NEVER delete (archive) · NEVER downgrade (sync meta source UP first) · pure-Rust, no C in trust
boundary · ledger-residency ($META_ROOT only) · packets rendered, never hand-written.**

## Resume command
```
/forge-loop resume from .handoff/loop/HANDOFF.md (branch <new-slug> off develop) — verify baseline
green; FIRST surface FINDING-0002 (Epic A blocker) for an owner decision, then run the next
UNBLOCKED pick TASK-0012 (crates/agent-env) via feature-architect → rust-implementer →
invariant-guardian. (TASK-0001 DONE, TASK-0002/0003 BLOCKED — do NOT retry them until FINDING-0002
is decided. cycles_this_session resets to 0 on resume.)
```
Do NOT work in any stale worktree. Create a FRESH worktree off `develop`:
`git worktree add ../envctl-<slug> -b <slug> develop` (or `meta git worktree create <slug>`),
PR to `develop` → auto-promotes to `master` via `.github/workflows/sync-master.yml`.

## Worktree
- This checkpoint written from: `/home/drdave/Desktop/meta/.worktrees/relay-handoff/envctl`
  (branch `relay-handoff`, **clean**, at `3b1d41e` = `origin/develop`). This worktree is the
  handoff staging area only — successor opens a NEW one.
- Per-cycle convention: `meta/.worktrees/<slug>/envctl` (or `../envctl-<slug>`) off `develop`.
- Workflow rule: `develop` = integration branch + GitHub DEFAULT; `master` = protected MIRROR
  auto-synced from develop (4 clean runs). Both at **3b1d41e**, IN SYNC. Branch protection on
  master = force-push/delete bans ONLY (NOT linear-history, NOT PR-required — those break the
  token ff-mirror; do not re-enable them).

## Backlog (mirror of `.handoff/loop/backlog.md`)
**Source of truth = `.handoff/loop/backlog.md`. Cards do NOT yet exist** (`hf task mint` lands in
TASK-0002); until then this markdown backlog owns ordering.

- **[x] TASK-0001 (P0, Epic A) — DONE (cycle 1).** `hf` built `--release` + installed
  (`~/.local/bin/hf` → `meta/handoff/target/release/hf`). On PATH; residency-correct (shared ledger
  only); the Stop/PreCompact hf-checkpoint hook is LIVE (witnessed-event write defers to TASK-0002).
- **[!] TASK-0002 (P0, A) — BLOCKED (cycle 2, NEEDS-DECISION → `FINDING-0002`).** Shipped `hf` is
  strictly CWD-relative (no `--ledger`/`HANDOFF_DIR` env), so it can't render envctl's Tier-A layer
  against the shared meta ledger without a forbidden per-repo `ledger.db`; `mint --from-kb` needs
  CWD=child-repo; `hf seed` writes the kernel's own HFTASK cards. Fix = kernel feature in
  `meta/handoff`. **Do not retry until FINDING-0002 is decided** (Option A recommended).
- **[!] TASK-0003 (P1, A) — BLOCKED with TASK-0002** (p7 gate needs a seeded layer; residency
  portion landable independently). Unblocks when FINDING-0002 is decided.
- TASK-0004 (P0, B) — wire `META_ROOT` into the env Claude inherits.
- TASK-0005 (P1, B) — settings.json `$META_ROOT` heal. **STATUS CONFLICT — see Open findings.**
- TASK-0006 / TASK-0007 / TASK-0008 (P2, B) — kasetto.yaml mcps source + shell hardcodes + stale
  URL; doctor boundary-refusal + idempotent symlink regen; relocate **meta-mcp** (first proof).
- TASK-0009 (B) — `[!]` SUPERSEDED by Epic C (kasetto becomes built-in).
- TASK-0010 (B) — **DONE** (rtk+rtk-monitor relocated by a human session, FlexNetOS/rtk-tokenkill#1
  merged; `~/.local/bin/rtk` now a symlink into meta 0.42.4). Was `[!!]` SUPERVISED — correctly NOT
  auto-run by the loop.
- TASK-0011 (P1, C) — refresh `docs/KASETTO-FEATURES.md` to v3.2.0.
- TASK-0012 (P0 of C) — new pure-Rust crate `crates/agent-env` (6-key+extends model, multi-host
  resolver, SHA-256, lock; drop `mimalloc`; no-c gate clean). **Gates TASK-0013..0018.**
- TASK-0013..0018 (C) — engine module/Events → CLI+GUI verbs → provisioning fidelity (additive
  MCP-merge, never-clobber) → lock unification → `extends` composition → retire external kasetto.
- TASK-0019..0022 (D) — secretd RealUsbProbe; github-app-mint ProviderMint seam; node-via-bun
  manifest follow-up; agent-web-access Phases 2–3 (n8n live smoke = HUMAN-ONLY `[!]`).
- TASK-0023 (E) — develop→master auto-sync action + master protection. **Effectively DONE**
  (workflow live, 4 clean runs) — confirm/close.

**Order (updated):** Epic A is BLOCKED at 0002/0003 (FINDING-0002). **Next unblocked pick = Epic C
TASK-0012** (`crates/agent-env`, gates 0013..0018) — large crate, wants fresh-context architect.
Smaller unblocked alternatives if budget/context is tight: TASK-0004 (P0, wire `META_ROOT` into the
env Claude inherits — via the `settings.json.tmpl` per-machine render path) or TASK-0011 (P1, docs
refresh, supports Epic C no-downgrade checklist). After FINDING-0002 is decided, return to Epic A.

## Cycle ledger
**2 cycles this session; 2 total** (`loop_state.md`: `cycle_budget: 3`, `cycles_this_session: 2`).
- cycle 1: TASK-0001 DONE/PASS-WITH-NOTES (landed develop `88e09ed`→merge `7dd2443`, PR #41).
- cycle 2: TASK-0002 + TASK-0003 BLOCKED/NEEDS-DECISION (landed develop `c8fb3b9`→merge `8b7e2c8`,
  PR #43; wrote `FINDING-0002`).
Session **stopped at 2/3 (early, deliberate)** — Epic A blocked on an owner decision + the next item
is a large fresh-context crate. **On resume, reset `cycles_this_session: 0`.**

## In-flight cycle
**None — clean boundary.** Both cycles fully committed + merged to develop=master=`8b7e2c8`; the
cycle-3 item (TASK-0012) was deliberately NOT started. No `.handoff/loop/cycle/03_*.md` in flight.

## Landed this session (already on develop — do NOT redo)
- `3b1d41e` Merge PR #40 / `048e750` harness: wire `.handoff` auto-checkpoint hook (DORMANT until
  hf) + queue go-live under TASK-0001/0002.
- `4a9693b` Merge PR #39 / `8c9077d` harness: add **handoff-kernel-engineer** agent (Epic A) + seed
  `loop_state.md` to forge-loop schema + reconcile backlog.
- `d69f452` Merge PR #38 / `1db91ae` harness: migrate `_workspace`→`.handoff/loop` +
  kasetto-absorption ref + handoff-sync skill + hf-aware continuity + no-c.sh Gate 3.5 (mimalloc).
- `bf29acd` PR #37: heal 3 hardcoded settings.json refs via `${META_ROOT}` tmpl (TASK-0005).
- `20c2aee` ci: develop→master auto-sync (protected-mirror).
- `d7a866f` docs: consolidate agenticOS backlog (Epics A–E) + ADR-0001.
- Plus the WIP-branch reconciliation: develop made superset of master+develop+5 WIP branches
  (feat/envctl-env, github-app-mint, fix-secretd, node-via-bun-fix, agent-web-access); verified
  green (build, 197 tests, no-c/shape/enable, fmt, clippy); 58 manifest components; stale branches
  deleted. Safety bundle: `/tmp/envctl-pre-reconcile-2026-06-12.bundle`.
- Meta repo (separate git): main `bf68d57` fixed broken `.kb` hook (`git kb service` →
  guarded `git kb serve`).

## Open findings
- **TASK-0005 status conflict (record both, ledger/git wins):** the backlog marks TASK-0005 `[~]`
  "IN REVIEW … PR envctl#37 → develop. Merge to close", but git shows commit **`bf29acd` ("heal 3
  hardcoded settings.json refs … (#37)") is ALREADY MERGED into develop**. Per State precedence
  (Git > backlog), **treat TASK-0005 as effectively DONE** and update the backlog checkbox to `[x]`
  on resume — verify the merge content matches the task's acceptance (settings.json.tmpl +
  claude-global-links per-machine render, byte-identical) before closing.
- No guardian FAIL / NEEDS-DECISION (no cycle ran).
- Two standing **needs-human / supervised** decisions (from `loop_state.md`, NOT loop-fixable):
  (1) bring GitKB into meta as a `.meta.yaml` project? (2) old dashboard-forge-loop GUI smoke test
  in `_done/` is HUMAN-ONLY.

## Decisions & dead ends
- **Key finding (carried):** most meta-built tools' INSTALLED binaries are NEWER than their
  committed meta SOURCES (kasetto 3.1.0>3.0.0). The real work is **sync-meta-source-UP-then-relocate**,
  NOT a symlink sweep. NEVER relocate `~/.local/bin/<tool>` to an older meta build.
- TASK-0009 (relocate kasetto+kst) is a **dead end as written** — superseded by Epic C (kasetto
  becomes a built-in `crates/agent-env`; no external binary to relocate once absorbed). Don't
  attempt the standalone relocation.
- Master branch protection: do NOT add linear-history or PR-required rules — they break the
  token-driven ff-mirror workflow (force-push/delete bans only).
- The `.handoff` auto-checkpoint hook is intentionally DORMANT/no-op until `hf` supports
  `checkpoint --auto --quiet` — that's by design, not a bug; TASK-0001 is its go-live.

## Invariant watch (re-verify for any code-touching cycle)
- **No C in the trust boundary** — `crates/agent-env` (TASK-0012) MUST drop `mimalloc`; no-c.sh
  Gate 3.5 already guards mimalloc.
- **Exactly one rustls, ring-only.**
- **Engine is the single shared non-printing library** — TASK-0013 `agent_env` engine module must
  emit Events (no `println!`), with CLI+GUI parity (TASK-0014).
- **Destructive ops fail-closed / dry-run by default.**
- **Provisioning MCP-merge (TASK-0015) is ADDITIVE, never-clobber** — must preserve the global
  broker/repowire/weave servers.
- **Ledger residency:** all `hf` verbs run from `$META_ROOT`; never create a per-repo `ledger.db`.
- **No-downgrade:** ADR-0001 11-verb / schema / 21-agent-preset checklist must pass before retiring
  external kasetto (TASK-0018).

## Per-repo vector
**n/a — single-repo cycle** (Epic A TASK-0001 builds in `meta/handoff` but is driven as one task by
the handoff-kernel-engineer agent; no A2 multi-repo worktree set is open). If a later cycle goes A2,
capture the meta set name + per-repo table then.

## Verify-on-resume (run FIRST — confirm clean baseline before any cycle)
```bash
# 1. Git baseline (expect 3b1d41e or later; tree clean; develop==master)
git -C <new-worktree> fetch && git rev-parse --short origin/develop   # expect 3b1d41e+
git -C <new-worktree> rev-parse --short origin/master                 # expect == develop
git -C <new-worktree> status --short                                  # expect empty

# 2. Build + CI gates green
cd <new-worktree> && cargo build --workspace \
  && bash ci/gates/no-c.sh && bash ci/gates/shape.sh && bash ci/gates/enable.sh

# 3. Loop preconditions
which hf                       # expect ABSENT → markdown-fallback path (TASK-0001 will fix)
cat .handoff/loop/loop_state.md   # cycle_budget 3, cycles_this_session 0 (reset on resume)

# 4. (optional) confirm the dormant hook + agent are present
ls .claude/hooks/hf-checkpoint.sh .claude/agents/handoff-kernel-engineer.md
```
On confirming green: pick **TASK-0001** via the **handoff-kernel-engineer** agent + **handoff-sync**
skill. **SUPERVISED rule:** never auto-run any `- [!!]` item → write `.handoff/loop/NEEDS-HUMAN`
and stop. (TASK-0010 was the only `[!!]`, already resolved by a human.)
