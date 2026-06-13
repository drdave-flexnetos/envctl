# Plan: Agent Full Read/Write/Edit Access to Web Apps (n8n first)

> Source: deep-research run wf_157d834d-19f (19 confirmed / 6 refuted claims, 20 sources).
> Reframes the user's "Pake + CLI-Anything" vision into the verified-strong architecture.

## Verdict
- **GOAL valid:** agents should have full read/write/edit on web apps; tools belong in the
  hub sub-registries; envctl owns the master registry + install/env management.
- **MECHANISM corrected:** Pake = packaging only (no automation surface, 3-0). CLI-Anything =
  Python, needs source/API, and its own n8n harness uses the **n8n REST API**. The strong path
  is the **native API surface via an MCP server**, with **Playwright MCP** as the API-less fallback.

## Recommended architecture — a 3-tier "agent web access" ladder
1. **Tier 1 — Native API + MCP (preferred):** app has an API → drive it via an MCP server.
   - n8n → **`czlonkowski/n8n-mcp`** (MIT, most mature). Needs `N8N_API_URL=http://localhost:5678`
     + `N8N_API_KEY`. Tools: create/update(partial+full)/validate/execute workflows, executions.
2. **Tier 2 — Playwright MCP (fallback):** API-less web apps. Already in the kasetto baseline.
   Accessibility-tree driven; write actions: click/type/fill_form/select/upload/dialog/evaluate.
3. **Tier 3 — Pake / CLI-Anything (optional):** Pake = desktop-wrap UX only; CLI-Anything = per-app
   CLI where source is available. Both non-Rust, external, sandboxed, OUT of the trust boundary.

## Registry federation (per user's clarification)
- **Master registry (envctl-owned)** federates the 11 hub sub-registries. Today it's *implicit*
  (`.meta.yaml` + envctl `manifest/` + `envctl.lock`); no root `registry.json` exists yet.
  → **Plan item M:** envctl gains a `registry` capability that reads every `*_hub/registry.json`,
  reconciles entries ↔ envctl components, and emits a federated master view (read-only first).
- **Sub-registry placement of the adopted tools:**
  - `mcp_hub/registry.json` ← `n8n-mcp` (and a generic `playwright-mcp` entry; it's already wired).
  - `tool_hub/registry.json` ← `pake` (runner: npm, hosting: registry-only), `cli-anything`
    (runner: pip, hosting: registry-only) — both `status: experimental`, optional.
- Each registry entry binds to an **envctl manifest component** (the executable arm).

## envctl components (install/env-manage non-Rust tools, OUT of trust boundary)
All are EXTERNAL tools (like ghostty/podman/icm) — never added to envctl's Cargo graph, so the
no-C / rust-native invariants are untouched. Each: detect/install/verify/fix/remove.
- `n8n-mcp` — Node/TS. install via `npm i -g` (or `npx` pinned). verify: server responds.
  requires: `node`, a running n8n, `N8N_API_KEY` (via secretd). MCP entry points `command: npx`.
- `pake` (optional) — `npm i -g pake-cli`. verify `pake --version`. bin `pake`.
- `cli-anything` (optional) — pipx/venv isolated Python 3.10+. verify `cli-anything --version`.
- Secret: `N8N_API_KEY` stored/served by **secretd** (envctl secrets stack), not in plaintext env.

## Phased implementation
**Phase 1 — n8n agent control (highest value, mostly wiring):**
1. Create an n8n API key (n8n UI → Settings → n8n API) — human/UI step; store via `secretctl`.
2. Add `n8n-mcp` to `mcp_hub/registry.json` (+ `entries/n8n-mcp.md`); validate.
3. envctl `n8n-mcp` manifest component (install Node server, env: N8N_API_URL + N8N_API_KEY from
   secretd). Detect/verify/fix/remove. Lock + gates.
4. Wire the MCP server into the agent baseline (kasetto for the project, or global ~/.claude),
   pointing at the secretd-served key. Smoke: agent creates+activates a test workflow.

**Phase 2 — generalize (Playwright MCP as the API-less tier):**
5. Add a `playwright-mcp` registry entry (mcp_hub) documenting it as the Tier-2 fallback (already
   installed). Document the 3-tier decision rule for new web apps.

**Phase 3 — master registry federation (the architecture piece):**
6. envctl `registry` capability: aggregate all `*_hub/registry.json` → reconcile vs components →
   read-only federated master view (`envctl registry [--json]`), then `--check` drift gate.

**Phase 4 — optional Tier-3 tools:**
7. tool_hub entries + optional envctl components for `pake` and `cli-anything`, status experimental.

## Risks / security (research-flagged)
- Agent web-write is dangerous: Playwright `browser_evaluate` ≈ arbitrary JS (RCE issue #1495);
  API keys + prompt-injection. → least-privilege API keys, secretd-isolated creds, sandbox the
  Node/Python tools, scope MCP servers to localhost.
- **Credential gap:** no API/MCP path gives full read-back/edit of existing n8n credentials
  (create/list only, 3-0). Don't promise secret-editing via agents.
- Maturity: n8n-MCP ecosystem is young (2025-26); `czlonkowski/n8n-mcp` is the most maintained —
  re-verify before adopting; pin versions.

## Open questions (from research)
- Single MCP server to standardize on for self-hosted localhost:5678 (czlonkowski vs others)?
- Any non-GUI route to full credential editing, or accept create/list only?
- Playwright MCP alone for API-less apps, or hybrid with CLI-Anything where source exists?
- Exact envctl component recipe for isolated Node/Python runtimes kept out of the trust boundary.
