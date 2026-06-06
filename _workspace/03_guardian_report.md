# Verification report: Agent Web-Access — Phase 1 (n8n-mcp config-only scaffolding)

## Verdict — PASS

Config-only Phase 1 is independently verified. All CI gates, locks, kasetto, and the mcp_hub
validator are green; the no-C trust boundary is unchanged; credential handling is fail-closed
with zero plaintext key in any file; the verify hook is a genuine key-less liveness probe.

## Gate results (envctl worktree)
| Gate | Command | Exit |
|------|---------|------|
| no-c | `bash ci/gates/no-c.sh` | **0 PASS** — `rustls=['0.23.40'] on ring=['0.17.14']; zero aws-lc/openssl/C-SQLite` |
| shape | `bash ci/gates/shape.sh` | **0 PASS** |
| enable | `bash ci/gates/enable.sh` | **0 PASS** |

## cargo / tooling
| Check | Command | Exit |
|-------|---------|------|
| lock --check | `cargo run -p envctl -- lock --check` | **0** — `✓ envctl.lock matches the manifest (50 components)` |
| component parse | `cargo run -p envctl -- auto-detect \| grep n8n` | **0** — `n8n-mcp ... wired`; detect = `Missing: declared but not installed` |
| kasetto sync --locked | `kasetto sync --locked` | **0** — 10 items audited, all unchanged (no drift) |
| mcp_hub validate | `python3 scripts/validate.py` | **0** — `✓ catalog OK — 11 servers` |

No `cargo fmt/clippy/test` run: Phase 1 adds zero Rust (config-only); these are N/A by design.

## Invariant checks
1. **No C in trust boundary — PASS.** `git diff --stat` shows NO `Cargo.toml`/`Cargo.lock`
   change; only `.codex/config.toml`, `.mcp.json`, `kasetto.lock`, `kasetto.yaml`,
   `manifest/envctl.lock` (+ untracked `n8n-mcp.toml`, `agent-skills/mcps/n8n-mcp.json`).
   `no-c.sh` PASS; rustls/ring unchanged. n8n-mcp is an external npm tool, enters nothing in
   the resolved graph (like the AI CLIs / podman). **The key invariant for this phase holds.**
2. **Code-shape — PASS.** `shape.sh` exit 0.
3. **secretd enable — PASS.** `enable.sh` exit 0.
4. **Engine purity — N/A/PASS.** No engine/CLI/GUI Rust touched; no `println!` added (no Rust diff).
5. **Front-end parity — PASS.** No new Engine method. Both front-ends (`.mcp.json` for Claude,
   `.codex/config.toml` for Codex) carry `n8n-mcp` identically — confirmed `n8n-mcp` present in
   both rendered files via the same kasetto source (`agent-skills/mcps/n8n-mcp.json`).
6. **Fail-closed / dry-run — PASS.** Live-mutation tier (workflow create/activate) is fenced
   behind the human `n8n-api-key` gate and is NOT wired into `verify`; documented in the runbook.
   See verify-hook section below.
7. **Rust-native, no drift — PASS.** Only config assets (TOML/JSON/YAML/MD). No new
   `.rs/.js/.ts/.py` in either worktree. No banned dep added; no foreign source vendored.
8. **Lock honesty — PASS.** `envctl.lock` 49→50 (`[components.n8n-mcp]` entry,
   `content_hash=f1cc110ba3589365`, `requires=["node-via-bun"]`); `lock --check` clean.
   `kasetto.lock` gained `mcp::./agent-skills::n8n-mcp.json`; `kasetto sync --locked` clean.

## Parity check
No Engine method added. MCP-baseline parity instead:
- kasetto source: `agent-skills/mcps/n8n-mcp.json:4`
- Claude surface: `.mcp.json:27` (`"n8n-mcp"`)
- Codex surface: `.codex/config.toml:48` (`[mcp_servers.n8n-mcp]`)
All three carry the identical secretd bash-wrapper. No divergence.

## SECRET-LEAK check (critical) — PASS
Grepped both worktrees (committed + working, excluding archived `_workspace_prev`):
- Every `N8N_API_KEY=` assignment in envctl is the command-substitution
  `"$(secretctl secret get n8n-api-key --reveal --apply)"` —
  `grep -rn 'N8N_API_KEY=' | grep -v '<substitution>'` returned **no matches** (exit 1) in envctl.
- mcp_hub: the only non-substitution `N8N_API_KEY=` is `servers/n8n-mcp.md:95` →
  `N8N_API_KEY=<key>` (a doc placeholder, not a value). Other refs are the
  `<your-n8n-api-key>` placeholder (`registry.json`, `snippets/n8n-mcp.mcp.json`,
  `servers/n8n-mcp.md`).
- No real/realistic key value exists anywhere in `.mcp.json`, `.codex/config.toml`,
  `agent-skills/mcps/n8n-mcp.json`, `kasetto.lock`, or any mcp_hub snippet/doc.
Credential handling is fail-closed: the key is resolved only at launch from the secretd vault.

## Fail-closed verify hook — PASS
`manifest/n8n-mcp.toml` `[component.verify]` drives a minimal MCP `initialize` JSON-RPC
handshake over stdio and asserts `grep -q 'n8n-documentation-mcp'`. It:
- does NOT reference `N8N_API_KEY`/`n8n-api-key` or the live n8n API — an absent key can neither
  falsely pass nor falsely fail it;
- is a genuine liveness probe (docs-server identity), NOT the falsely-passing `--version`
  (correctly rejected — see implementer log; `--version` exits 0 on STDIN close without printing
  a version).
Behavior verified live, key-less:
- Ran the exact hook command → exit **0** (PASS): `npx -y n8n-mcp` fetched on demand, answered
  the handshake, identity matched.
- Fail-closed confirmed: when the docs identity is not emitted, `grep -q` returns non-zero, so
  verify fails (no false pass).
- Install state in this env: npm/npx/node present; `n8n-mcp` is NOT globally installed and NOT
  on PATH → `detect` correctly reports `Missing: declared but not installed`, consistent with
  `verify` passing only because `npx -y` fetches it transiently.

## Scope guard — PASS
- No Rust engine/CLI/GUI code changed (no `.rs` diff).
- Nothing committed by the implementer: envctl HEAD is the pre-existing loop commit
  `4471949` (no n8n commit); all Phase 1 changes are in the working tree. mcp_hub n8n changes
  likewise uncommitted. Nothing pushed.
- Real `.meta.yaml` untouched.
- No live smoke run: BLOCKED on the human key gate; runbook present at
  `_workspace/n8n-live-smoke-runbook.md`; no `n8n-api-key` secret minted/stored.

## Findings
- **Note (non-blocking):** The `_workspace` directory was reshuffled — prior crew outputs
  (`01_architect_plan.md`, `02_implementer_log.md`, prior `03_guardian_report.md`, HANDOFF,
  backlog) were moved to `_workspace_prev/_workspace/` and the current Phase 1 inputs re-created
  under `_workspace/`. This is a workspace-hygiene side effect, not a Phase 1 deliverable, and
  appears in `git status` as renames. No impact on the feature; flag for the orchestrator so the
  per-repo commit scopes the intended files (the n8n config assets) and does not accidentally
  sweep the `_workspace_prev` churn into the feature commit.
- **Note (non-blocking):** `servers/n8n-mcp.md:95` contains `N8N_API_KEY=<key>` as a CLI doc
  example. It is a literal-looking placeholder (`<key>`), not a secret — acceptable, but worth a
  glance if anyone tightens the leak-scan regex later.

## Re-test needed
None for a PASS. If any config is edited before commit, re-run:
```
# envctl worktree
bash ci/gates/no-c.sh ; bash ci/gates/shape.sh ; bash ci/gates/enable.sh
cargo run -p envctl -- lock --check
kasetto sync --locked
# mcp_hub worktree
python3 scripts/validate.py
# leak re-scan (both worktrees)
grep -rnI 'N8N_API_KEY=' . | grep -v 'secretctl secret get n8n-api-key --reveal --apply'
```
