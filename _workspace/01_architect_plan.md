# 01 — Architect Plan: Agent Web-Access Phase 1 (n8n via czlonkowski/n8n-mcp)

VERDICT: GO — config-only (zero Rust engine changes, zero Cargo-graph impact). Live smoke is the only blocked item (human API-key gate).

## Verified facts (primary sources)
- `n8n-mcp` npm pkg, bin `n8n-mcp`; run `npx -y n8n-mcp` or `npm i -g n8n-mcp`. Transport **stdio**. License MIT. Version ~2.57.1 (README; FlexNetOS doc stale at 2.56.0 — bump, confirm via `npm view n8n-mcp version`).
- **Key-less "docs-only" mode CONFIRMED** (7 core tools work with no `N8N_API_*`) → basis for the component's key-less verify. Live management tier (13 tools: create/update/run workflows) needs `N8N_API_URL` + `N8N_API_KEY`.
- n8n live on http://localhost:5678 (/api/v1 → 401). `npx` = real npm 10.9.8.
- **secretctl:** `secretctl secret get n8n-api-key --reveal --apply` prints raw value, NO trailing newline (verified `crates/secretctl/src/render.rs`). `secretctl secret list` → no n8n secret (HUMAN GATE absent). `secretctl run` injects relay creds only, not arbitrary env vars → use command-substitution.

## secretd launch wrapper (no plaintext in git)
```
command: bash
args: ["-lc", "export PATH=\"$HOME/.bun/bin:$HOME/.local/bin:$PATH\"; export N8N_API_URL=http://localhost:5678; export N8N_API_KEY=\"$(secretctl secret get n8n-api-key --reveal --apply)\"; exec npx -y n8n-mcp"]
```
Key id: `n8n-api-key`. Human stores via `secretctl secret add n8n-api-key --provider n8n --value-stdin`.

## Deliverables (config-only)
### 1. manifest/n8n-mcp.toml (external Node tool, OUT of trust boundary; model on ai-clis.toml + dashboard.toml)
- id `n8n-mcp`, requires `["node-via-bun"]` (provides node).
- detect: `bash -lc 'export PATH=...; npm ls -g n8n-mcp >/dev/null 2>&1 || command -v n8n-mcp >/dev/null'`
- install: `export PATH=...; npm i -g n8n-mcp` (idempotent)
- verify (KEY-LESS, fail-closed, degrades gracefully): `n8n-mcp --version` or docs-only stdio handshake — must NOT assert live n8n/key path (absent key must not falsely pass).
- fix: `npm i -g n8n-mcp`; remove: `npm rm -g n8n-mcp`.
- Description notes: live tier needs running n8n + `n8n-api-key` secret (human gate); live smoke documented, NOT wired into verify.

### 2. kasetto baseline wiring
- Add `agent-skills/mcps/n8n-mcp.json` with the secretd bash-wrapper (no real key; `N8N_API_URL` inline ok).
- Add `n8n-mcp` to `kasetto.yaml` mcps list (after sequential-thinking).
- `kasetto sync` → renders `.mcp.json` + `.codex/config.toml`; `kasetto sync --locked` clean; commit `kasetto.lock`.
- (Precedent: icm serve MCP was added then reverted because ICM standard mode was preferred; n8n-mcp is the opposite — MCP IS the right surface.)

### 3. mcp_hub registry (REFINE — entry already exists: id n8n-mcp, runner npx, transport stdio, auth api-key)
- Bump `servers/n8n-mcp.md` version 2.56.0 → confirmed current; update tool list if drifted.
- Add `snippets/n8n-mcp-secretd.mcp.json` (the secretd wrapper, localhost) + link in README.md (validator requires it).
- Bump registry.json version/updated. Run `python3 scripts/validate.py` → `✓ catalog OK`.

### 4. Lock/gates
- Regen `manifest/envctl.lock` (`cargo run -p envctl -- lock`) → +1 component (record real before/after, ~49→50); `lock --check` clean.
- `bash ci/gates/{no-c,shape,enable}.sh` all green.

## Invariants (all PASS)
- no-C: external Node tool, not a Cargo dep → graph unchanged.
- single engine: zero engine/CLI/GUI code; config-only.
- fail-closed: install/fix/remove idempotent; live mutation fenced behind human key; verify is key-less and never claims live capability.
- no drift: only sanctioned config assets (TOML/JSON/MD); no JS/Python source vendored.
- lock sync: regen envctl.lock + kasetto.lock before push.

## Scaffoldable NOW vs BLOCKED
- NOW: manifest component (key-less verify), kasetto wiring, mcp_hub refine, lock+gates, docs-only self-check.
- BLOCKED on human gate: mint n8n API key (UI Settings→n8n API), `secretctl secret add n8n-api-key`, live smoke (agent creates+activates a workflow).

## Work breakdown (implementer)
1. manifest/n8n-mcp.toml. 2. lock regen + --check. 3. agent-skills/mcps/n8n-mcp.json + kasetto.yaml. 4. kasetto sync + --locked + commit rendered files. 5. ci/gates. 6. mcp_hub refine + validate.py. 7. live-smoke runbook into _workspace (BLOCKED). 8. NO live smoke; commit per-repo on agent-web-access.

## Open (implementer confirms, not design forks)
- `npm view n8n-mcp version` at build time. Confirm `n8n-mcp --version` is a valid liveness probe; else minimal docs-only stdio call.
