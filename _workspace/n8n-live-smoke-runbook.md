# n8n-mcp live-smoke runbook — BLOCKED on human gate

**Status:** BLOCKED. Phase 1 scaffolding (component, kasetto wiring, mcp_hub refine, lock,
gates) is GREEN and key-less. The live management tier (create/activate workflows) is fenced
behind a human-minted n8n API key and is intentionally NOT wired into `verify`. This runbook
documents the steps to execute once a human mints + stores the key.

## Why this is blocked

- The docs-only tier (7 core tools) works with NO credentials and is what the component's
  `verify` hook proves (MCP `initialize` handshake → `n8n-documentation-mcp`).
- The management tier (13 tools incl. `n8n_create_workflow`) requires `N8N_API_URL` +
  `N8N_API_KEY`. There is no `n8n-api-key` secret in the vault yet
  (`secretctl secret list` shows none) — this is the HUMAN GATE.
- Per scope: do NOT mint, store, or hardcode any key in this phase. No config in git contains
  a real key; the launch wrappers substitute it at runtime via `secretctl`.

## Preconditions (verify first)

1. n8n is running and reachable: `curl -sS -o /dev/null -w '%{http_code}\n' http://localhost:5678/api/v1`
   should return `401` (up, unauthenticated).
2. `n8n-mcp` installed: `npm ls -g n8n-mcp` (or run `envctl install n8n-mcp` / `auto-fix`).
3. Docs-only verify passes (key-less):
   `printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}' | npx -y n8n-mcp | grep n8n-documentation-mcp`

## Steps (run by the human + agent once the gate opens)

### 1. HUMAN: mint the n8n API key
- In the n8n UI: **Settings → n8n API → Create an API key**. Copy the value.

### 2. HUMAN: store it in the secretd vault (no plaintext in git)
```bash
secretctl secret add n8n-api-key --provider n8n --value-stdin
# paste the key on stdin, then Ctrl-D
```
Confirm it landed (metadata only, value stays sealed):
```bash
secretctl secret list   # should now show n8n-api-key
```

### 3. Reload the MCP server so it picks up the key
The wrapper resolves the key at launch via
`export N8N_API_KEY="$(secretctl secret get n8n-api-key --reveal --apply)"`, so a restart of
the MCP client (Claude Code / Codex) is sufficient — no config edit. After reload, the 13
management tools should appear alongside the 7 docs-only tools.

### 4. AGENT: live smoke — create + activate a workflow against :5678
- `n8n_health_check` → expect healthy / instance reachable.
- `n8n_create_workflow` with a minimal workflow (e.g. a Manual Trigger → NoOp/Set node).
- Activate it (`n8n_update_partial_workflow` setting `active: true`, or the activate path the
  tool exposes).
- `n8n_list_workflows` → confirm the new workflow is present and active.
- (Cleanup) `n8n_delete_workflow` to leave the instance clean, if desired.

## Acceptance (live smoke PASS criteria)
- [ ] `n8n-api-key` secret exists in the vault (human-stored).
- [ ] MCP client exposes the 13 management tools after reload.
- [ ] An agent-created workflow appears in n8n and reports `active: true`.
- [ ] No plaintext key ever written to any git-tracked file (wrapper substitution only).

## Invariant reminders
- The key lives only in the secretd vault; configs in git carry the `secretctl secret get`
  substitution, never a value.
- `verify` stays key-less and fail-closed: an absent key must neither falsely pass nor falsely
  fail the docs-only check. Do not move any live-tier assertion into the component's verify.
