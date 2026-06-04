# env-ctl research — Anthropic API base-URL proxy + streaming

> Research date: 2026-06-02. Assistant knowledge cutoff is Jan 2026; every
> version/API claim below was re-verified against live Anthropic docs, the SDK
> registries/source, and community sources at research time. Items that could not
> be independently confirmed are flagged ⚠️ / ❌.
>
> Scope note: this doc is the **API contract** Anthropic exposes plus its
> implications for env-ctl's broker. env-ctl's own Anthropic adapter is **Phase 4
> (not yet implemented)** — the code at `crates/secretd/src/proxy.rs` and the
> `ProviderAdapter` trait are stubs. Treat the "guidance" section as a Phase-4
> spec, not a description of shipped behavior.

---

## TL;DR — recommendation for env-ctl

1. **Default Anthropic swap mode = `BaseUrlRepoint`.** Both official SDKs honor a
   custom base URL via constructor arg *and* the `ANTHROPIC_BASE_URL` env var, with
   precedence **explicit arg → env var → hardcoded default `https://api.anthropic.com`**
   (verified in the TypeScript SDK source). The daemon hands the client a peer-bound
   relay bearer pointed at the local relay endpoint, then swaps in the real key and
   re-originates verified TLS to `api.anthropic.com`. This is exactly the locked
   model in `ARCHITECTURE.md` §6 (`BaseUrlRepoint`) and decision 5. **API contract
   is ready; env-ctl adapter is Phase 4.**
   <https://platform.claude.com/docs/en/api/sdks/python> ·
   <https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/client.ts>

2. **Auth is `x-api-key`, NOT `Authorization: Bearer`.** This is the single most
   important difference from OpenAI. The Anthropic Messages API authenticates with
   the `x-api-key` header plus a mandatory `anthropic-version` header. The env-ctl
   swap must (a) strip the client's relay bearer, (b) inject `x-api-key: <real key>`,
   and (c) NOT add an `Authorization` header. ⚠️ OAuth/subscription tokens
   (`sk-ant-oat-*`) **also** require `x-api-key` and are rejected over Bearer — and
   are reportedly rejected by the Messages API entirely ("OAuth authentication is
   currently not supported") regardless of header. Design the mint seam around real
   API keys (`sk-ant-api03-*`), not OAuth tokens.
   <https://platform.claude.com/docs/en/api/messages> ·
   <https://github.com/badlogic/pi-mono/issues/2751> ·
   <https://github.com/anthropics/claude-code/issues/37205>

3. **Streaming is SSE — pass it through UNBUFFERED.** Anthropic streams with
   `"stream": true` over Server-Sent Events. The relay MUST forward bytes without
   buffering or re-chunking and MUST preserve response headers (especially the
   `anthropic-ratelimit-*` and `retry-after` set) so the client's own backoff and
   SDK accumulation work. SSE events are delimited by **a blank line (`\n\n`, LF per
   the W3C SSE spec)**, not `\r\n\r\n`. Do not parse or rewrite the event body in
   the relay — treat it as an opaque byte stream.
   <https://platform.claude.com/docs/en/api/streaming> ·
   <https://html.spec.whatwg.org/multipage/server-sent-events.html>

4. **Provider-pin the upstream host.** Per HF-11 / decision 5, the Anthropic relay
   policy's `upstream_base` must be validated at swap time against a canonical host
   allowlist whose default is `api.anthropic.com`. A policy re-pointing to
   `attacker.evil` is refused with a durable-audited deny, never propagated. This is
   the one hardening item that has no equivalent in the upstream API and is purely
   env-ctl's responsibility.
   `docs/DESIGN-NOTES.md` HF-11 · `docs/ARCHITECTURE.md` §6

5. **`ProxyMitm` is the fallback** for any tool that hardcodes `api.anthropic.com`
   (Anthropic SDKs all honor the base URL, so this is rarely needed for SDK clients,
   but Claude Code reads `ANTHROPIC_BASE_URL` too — see §"Claude Code"). Keep it
   feature-gated per `ARCHITECTURE.md` §6 (`mitm-ca`).

---

## Key facts (with inline sources)

### Endpoint, auth, versioning
- **Base URL:** `https://api.anthropic.com`; Messages endpoint is `POST /v1/messages`.
  <https://platform.claude.com/docs/en/api/messages>
- **Auth header:** `x-api-key: $ANTHROPIC_API_KEY`. The canonical curl example sends
  `-H "x-api-key: $ANTHROPIC_API_KEY"` — there is **no** `Authorization: Bearer` in
  the documented flow. <https://platform.claude.com/docs/en/api/messages>
- **Mandatory version header:** `anthropic-version: 2023-06-01`. Both official SDKs
  send this automatically as a default header.
  <https://platform.claude.com/docs/en/api/sdks/python> ·
  <https://platform.claude.com/docs/en/api/sdks/typescript>
- **OAuth tokens (`sk-ant-oat-*`):** rejected when sent via `Authorization: Bearer`;
  work via `x-api-key` for some endpoints; ⚠️ reported as rejected by the Messages
  API outright. Not a viable broker credential today.
  <https://github.com/badlogic/pi-mono/issues/2751> ·
  <https://github.com/anthropics/claude-code/issues/37205>

### Streaming (SSE)
- Set `"stream": true` to stream incrementally over SSE.
  <https://platform.claude.com/docs/en/api/streaming>
- **Event flow (strict order):** `message_start` → for each content block:
  `content_block_start` → one or more `content_block_delta` → `content_block_stop`
  → one or more `message_delta` → `message_stop`. `ping` events may appear anywhere;
  `error` events may appear (e.g. `overloaded_error`, the streaming analogue of
  HTTP 529). New event types may be added — clients (and any parser) must ignore
  unknown ones. <https://platform.claude.com/docs/en/api/streaming>
- **Delta types** inside `content_block_delta`: `text_delta` (text), `input_json_delta`
  (tool-use `input` as *partial JSON strings*; the final `tool_use.input` is always
  a complete object), `thinking_delta` + a single `signature_delta` (extended
  thinking; the signature precedes `content_block_stop` and lets clients verify
  block integrity). <https://platform.claude.com/docs/en/api/streaming>
- **`message_delta.usage` token counts are cumulative.**
  <https://platform.claude.com/docs/en/api/streaming>
- **Each SSE message** carries an `event:` name (e.g. `event: message_stop`) and a
  `data:` JSON payload whose `type` matches; messages are separated by a blank line.
  The wire delimiter is LF (`\n\n`) per the WHATWG/W3C SSE spec.
  <https://html.spec.whatwg.org/multipage/server-sent-events.html>
- **Non-streaming** requests return a single JSON `Message` object (no SSE).
  <https://platform.claude.com/docs/en/api/messages>

### Rate-limit response headers (preserve through the relay)
Returned on every response; clients rely on them for backoff:
- `retry-after` — seconds to wait (on 429).
- `anthropic-ratelimit-requests-{limit,remaining,reset}`
- `anthropic-ratelimit-tokens-{limit,remaining,reset}`
- `anthropic-ratelimit-input-tokens-{limit,remaining,reset}`
- `anthropic-ratelimit-output-tokens-{limit,remaining,reset}`
- `anthropic-priority-{input,output}-tokens-{limit,remaining,reset}` (Priority Tier)

`*-reset` values are **RFC 3339** timestamps; `*-remaining` token counts are rounded
to the nearest thousand. Exceeding any limit yields **HTTP 429** with a `retry-after`
header. <https://platform.claude.com/docs/en/api/rate-limits>

### SDK error/timeout behavior (matters for relay timeouts)
- SDK default request timeout is **10 minutes**; the TypeScript SDK scales it up to
  ~60 min for large non-streaming `max_tokens` (`(60*60*maxTokens)/128000`, floored
  at 10 min). The SDKs *require* streaming for very long generations to avoid HTTP
  timeouts. The relay must therefore tolerate long-lived connections and not impose
  a short idle timeout on the SSE path.
  <https://platform.claude.com/docs/en/api/sdks/typescript> ·
  <https://platform.claude.com/docs/en/api/sdks/python>
- SDKs auto-retry (2x default) on connection errors, 408, 409, 429, ≥500.
  <https://platform.claude.com/docs/en/api/sdks/python>

---

## Current versions / APIs (as of 2026-06-02)

| Component | Current | Source |
|---|---|---|
| Python SDK `anthropic` | **0.105.2** (rel. 2026-05-29) ⚠️ one source showed 0.104.1/2026-05-22 | <https://pypi.org/project/anthropic/> |
| TypeScript SDK `@anthropic-ai/sdk` | **0.100.1** (≈2026-05-29) | <https://www.npmjs.com/package/@anthropic-ai/sdk> |
| Go SDK `anthropic-sdk-go` | min Go 1.23+; current via repo | <https://platform.claude.com/docs/en/api/sdks/go> |
| Java SDK `com.anthropic:anthropic-java` | **2.35.0** (shown in install docs) | <https://platform.claude.com/docs/en/api/client-sdks> |
| API version header | `2023-06-01` (unchanged) | <https://platform.claude.com/docs/en/api/messages> |
| Docs host | **Migrated** from `docs.anthropic.com` → `platform.claude.com/docs` (301) | observed redirect, 2026-06-02 |

**Base-URL config, verbatim from the SDKs:**

```python
# Python — anthropic/Anthropic constructor
client = Anthropic(
    # Or use the `ANTHROPIC_BASE_URL` env var
    base_url="http://my.test.server.example.com:8083",
)
```
<https://platform.claude.com/docs/en/api/sdks/python>

```typescript
// TypeScript — @anthropic-ai/sdk, from src/client.ts:
//   baseURL = readEnv('ANTHROPIC_BASE_URL'),
//   ... baseURL: baseURL || `https://api.anthropic.com`,
// Precedence: explicit `new Anthropic({ baseURL })` → env var → default.
const client = new Anthropic({ baseURL: "http://127.0.0.1:8083" });
```
<https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/client.ts>

### Claude Code CLI env vars (relevant if a Claude Code child is the relay client)
- `ANTHROPIC_BASE_URL` — "Custom base URL for all Anthropic API calls"; default
  `https://api.anthropic.com`.
- `ANTHROPIC_API_KEY` — direct auth.
- `ANTHROPIC_AUTH_TOKEN` — "Alternative bearer token for enterprise proxies that
  reject the standard `ANTHROPIC_API_KEY` header." (Not needed for env-ctl, which
  swaps to `x-api-key` at egress; relevant only if env-ctl ever wanted the *client*
  to present a relay token via `Authorization`.)
- `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY` — standard proxy controls.

⚠️ These descriptions come from a third-party documentation mirror, not the
canonical page — `code.claude.com/docs/en/env-vars` was unreachable from this
sandbox (repeated `ECONNREFUSED`). The *names* are corroborated by multiple sources;
re-verify exact wording against the official page before pinning.
<https://code.claude.com/docs/en/env-vars> (canonical, unfetched) ·
<https://claude-codex.fr/en/reference/environment/> (mirror, fetched)

⚠️ Whether Claude Code re-reads `ANTHROPIC_BASE_URL` mid-session or only at startup
could not be confirmed against the official doc. env-ctl's injection model sidesteps
this entirely: `env-ctl run -- <cmd>` injects the base URL into the **child process
env before exec** (`ARCHITECTURE.md` §9), so startup-time reads are exactly what we
want — no mid-session mutation is attempted.

---

## Security tradeoffs

- **`x-api-key` swap is header-replacement, not header-add.** Because Anthropic
  ignores `Authorization` for normal keys but `x-api-key` takes precedence when both
  are present (community-observed, ⚠️ not in official docs), the *safe* rule is:
  delete any inbound `Authorization` AND any inbound `x-api-key`, then set exactly
  one `x-api-key` to the real key. This avoids a client smuggling its own key/garbage
  to the upstream. <https://github.com/BerriAI/litellm/issues/29190>
- **Plain-HTTP client→broker leg is replayable by same-uid processes** (HF-8). The
  relay bearer rides an unencrypted loopback hop in `BaseUrlRepoint`; mitigations are
  peer-binding at swap (`SO_PEERCRED` uid + per-mint pid nonce for ephemerals),
  host/path/method allowlist, quota, and the ≤24h TTL. Document this honestly; do not
  claim confidentiality on that hop. `docs/DESIGN-NOTES.md` HF-8.
- **Upstream egress trust pinning (FS-S7).** The real key must only ever leave over
  TLS verified against the **frozen webpki-roots** store — never the local CA, never
  the OS store. A misconfigured `upstream_base` + a broad trust store would let the
  real key egress to an attacker with a valid public cert; HF-11's host allowlist is
  the second lock. `docs/ARCHITECTURE.md` §6 (FS-S7) · `docs/DESIGN-NOTES.md` HF-11/CF-6.
- **SSE buffering is a correctness AND a leak hazard.** Buffering breaks real-time UX
  and the SDK's keep-alive/timeout logic; it also means the relay holds more model
  output in memory. Stream straight through.
- **Header passthrough vs. rewrite of `anthropic-ratelimit-*`.** Passing them through
  unmodified is simplest and correct for the client's backoff. ⚠️ Open question: if
  env-ctl imposes its *own* quota that is stricter than Anthropic's, the client will
  see Anthropic's (looser) remaining counts and may over-drive into env-ctl's 403s.
  Decide whether to leave headers untouched (recommended default) or down-rewrite
  `*-remaining`/inject `retry-after` to reflect env-ctl's quota.
- **No OAuth credential brokering.** Since `sk-ant-oat-*` tokens are not accepted by
  the Messages API, env-ctl cannot relay a user's Claude *subscription*; only org
  API keys are brokerable. This bounds the feature's scope.

---

## Concrete guidance for the env-ctl implementation (Phase 4 spec)

Target file: `crates/secrets-engine/src/broker/adapter.rs` (`AnthropicAdapter:
ProviderAdapter`) + the relay data-plane in `crates/secretd/src/proxy.rs`.

1. **Auth swap (egress only, inside the `decide() == Allow` branch):**
   - Remove inbound `Authorization` and inbound `x-api-key`.
   - Set `x-api-key: <real key>` from the DEK-sealed secret (fetched ONLY in the
     Allow branch, per ARCHITECTURE §3 default-deny).
   - Ensure `anthropic-version` is present; if the client omitted it, inject
     `2023-06-01` (the SDKs always send it, so usually a no-op).
   - Never add `Authorization`.

2. **Path/method allowlist (per relay policy):** restrict to the Anthropic surface
   the policy permits (at minimum `POST /v1/messages`; consider
   `POST /v1/messages/count_tokens`, `POST /v1/messages/batches`, `/v1/models`).
   `BaseUrlRepoint` clients send base-URL-relative paths; the broker appends to the
   rewritten host — guard against `/v1` double-prefix by normalizing once.

3. **Upstream host pin:** add an Anthropic row to `policy.rs::canonical_upstreams()`
   = `{ "api.anthropic.com" }` (operator-extensible). Refuse + durably audit any
   `upstream_base` outside the set (HF-11). The proto already carries `upstream_base`
   constrained to this allowlist (`docs/api/control-plane.proto:81`).

4. **Streaming passthrough:** when the request body has `"stream": true` (or always,
   to be safe), forward the upstream response as an opaque byte stream — no buffering,
   no re-chunking, preserve `Content-Type: text/event-stream` and
   `Transfer-Encoding`/chunked framing as received. Do not parse SSE events in the
   relay. (Tool-use partial-JSON deltas and thinking/signature deltas just flow
   through — the relay needs no awareness of them.)

5. **Response header passthrough:** copy `anthropic-ratelimit-*`, `retry-after`,
   `request-id`, and `anthropic-priority-*` verbatim. Default: do **not** rewrite.

6. **Timeouts:** the relay's read timeout on the upstream/SSE leg must exceed the
   SDK's (up to ~60 min for large non-streaming `max_tokens`); prefer no idle
   timeout on an active SSE stream and rely on connection close.

7. **Bearer peer-binding at swap (HF-8):** store `SO_PEERCRED` uid (named relays) and
   uid+child-pid (ephemerals from `env-ctl run`) at mint; re-check at swap time.

8. **Injection (`env-ctl run`):** set `ANTHROPIC_BASE_URL=<local relay>` and the
   relay bearer in the **child env only** (`inject.rs`); never put the real key in
   the child env, shell history, or git (FS-S1).

### Phase-4 test checklist (none exist yet)
- [ ] Real `x-api-key` never appears on the client-facing wire (FS-S1/S6).
- [ ] `Authorization` and client-supplied `x-api-key` are stripped before egress.
- [ ] `upstream_base = attacker.evil` → 403 + audit, no egress (HF-11).
- [ ] SSE streamed unbuffered; byte-for-byte event passthrough incl. `input_json_delta`,
      `thinking_delta`, `signature_delta`, `ping`, `error`.
- [ ] `anthropic-ratelimit-*` + `retry-after` preserved on 200 and 429.
- [ ] Long streaming generation does not trip a relay idle timeout.
- [ ] Out-of-allowlist path/method denied + audited.
- [ ] Bearer peer-bound; same-uid replay outside the bound pid (ephemeral) denied.

---

## Open questions

1. **Rate-limit header policy** when env-ctl's own quota is stricter than Anthropic's:
   pass through (recommended) vs. down-rewrite `*-remaining` / synthesize `retry-after`.
   No precedent in upstream docs — env-ctl design decision.
2. **OAuth/subscription brokering** ❌ not viable today (`sk-ant-oat-*` rejected by
   the Messages API). Re-check if Anthropic ships programmatic subscription access
   (tracked publicly in claude-code#37205).
   <https://github.com/anthropics/claude-code/issues/37205>
3. **Claude Code base-URL read timing** ⚠️ (startup vs. mid-session) — unconfirmed
   from the official `code.claude.com/docs/en/env-vars` page (sandbox could not reach
   it). Re-verify; env-ctl's pre-exec injection makes this moot in practice.
4. **`x-api-key` vs `Authorization` precedence** ⚠️ — observed in community proxies
   (LiteLLM), not stated in Anthropic's official docs. The "strip both, set one"
   rule above is the conservative design regardless of precedence.
   <https://github.com/BerriAI/litellm/issues/29190>
5. **`count_tokens` / batches / models endpoints** in the allowlist — confirm which
   Anthropic surfaces env-ctl wants to broker beyond `/v1/messages`.
6. **SDK version drift** ⚠️ — sources disagreed on the exact latest Python patch
   (0.105.2 vs 0.104.1). Re-pin against PyPI at integration time; the base-URL/auth
   contract is stable across these patches.
7. **env-ctl store backend (OI-1)** remains the upstream blocker — there is no
   persistent vault yet (`inmem-store` is test-only), so no Anthropic key can be
   stored at rest until OI-1 is ruled. `docs/DESIGN-NOTES.md` OI-1.
