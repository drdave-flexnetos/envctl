# env-ctl research — OpenAI base-URL + scoped keys

> Research date: 2026-06-02. Assistant knowledge cutoff is Jan 2026; version/API
> claims below were verified against live OpenAI docs and community sources at
> research time. Items that could not be confirmed are flagged ⚠️ / ❌.

---

## TL;DR — recommendation for env-ctl

1. **Default OpenAI swap mode = `BaseUrlRepoint`.** Both the Python and Node SDKs
   honor `OPENAI_BASE_URL` (env) and a `base_url` / `baseURL` constructor arg, and
   all OpenAI auth is a plain `Authorization: Bearer <token>` header — including
   streaming (SSE) and Workload Identity Federation tokens. The env-ctl daemon can
   hand the client a short-lived relay bearer pointed at the local relay, then
   inject the real key and re-originate TLS to `api.openai.com`. No MITM CA, no SDK
   patching, real key stays daemon-only. This matches the locked decision in
   `ARCHITECTURE.md` §6. **Production-ready now.**
   <https://developers.openai.com/docs/api-reference/authentication>
   <https://openai.github.io/openai-agents-python/config/>

2. **`NativeSubToken` is feasible but narrower than hoped (Phase 2).** There is
   **no Admin API endpoint to CREATE project API keys** — project keys are
   dashboard-only (LIST / GET / DELETE exist programmatically). The *programmable*
   path is **service accounts**: `POST /v1/organization/projects/{project_id}/service_accounts`
   creates a service account and returns an unredacted `sk-svcacct-*` key. So
   env-ctl can mint scoped credentials, but the unit of scope is a *service
   account within a project*, not a per-agent key, and revocation is still binary.
   <https://developers.openai.com/api/reference/resources/organization/subresources/projects/subresources/service_accounts/methods/create>

3. **`ProxyMitm` stays the fallback** for tools that hardcode `api.openai.com`
   (and for non-OpenAI hosts). Keep it feature-gated (`mitm-ca`) with DEK-protected
   CA key and NameConstraints, per `ARCHITECTURE.md`.

4. **Do NOT plan programmatic *project* API key minting.** It does not exist and
   there is no published roadmap for it. Design the mint seam around service
   accounts (or ephemeral/WIF tokens), not project keys.

---

## Key facts (with inline source URLs)

### Authentication & transport
- **Bearer-token auth everywhere.** OpenAI authenticates with
  `Authorization: Bearer <API key | short-lived token>`. Same header for
  inference, streaming, and federated tokens.
  <https://developers.openai.com/docs/api-reference/authentication>
- **Org / project selection headers.** `OpenAI-Organization` and `OpenAI-Project`
  headers disambiguate when a key/token spans multiple orgs or projects.
  <https://developers.openai.com/docs/api-reference/authentication>
- **Streaming = SSE.** Responses stream as Server-Sent Events
  (`Content-Type: text/event-stream`); auth is the identical bearer header, so a
  relay that forwards the bearer + body works transparently for streaming.
  <https://developers.openai.com/api/docs/guides/streaming-responses>

### Base-URL override (the load-bearing fact for env-ctl)
- **Env var:** `OPENAI_BASE_URL` is read by both the Python and Node SDKs.
  <https://openai.github.io/openai-agents-python/config/>
  <https://github.com/openai/openai-python/issues/745>
- **Constructor arg:** Python `OpenAI(base_url="...")`; Node `new OpenAI({ baseURL: "..." })`
  (note camelCase in Node).
  <https://github.com/openai/openai-python/issues/745>

### Key formats / prefixes
- `sk-proj-*` — project-scoped keys (default since the 2024 Projects launch).
- `sk-svcacct-*` — service-account keys (programmatically mintable; see below).
- `sk-admin-*` — Admin API keys, **org-level only, cannot call inference**.
- `ek_*` — Realtime ephemeral client secrets (browser/mobile).
  <https://developers.openai.com/api/docs/guides/admin-apis>
  <https://platform.openai.com/docs/api-reference/realtime-sessions/create-realtime-client-secret>
  (Prefix overview corroborated by a third-party writeup:
  <https://vibekit.bot/openai-api-key-format> — treat as secondary.)

### Admin API (key/credential management)
- **Admin API exists** and manages orgs, projects, users, audit logs, and keys via
  `sk-admin-*` keys. It cannot perform inference.
  <https://developers.openai.com/api/docs/guides/admin-apis>
- **Project API keys: LIST / GET / DELETE only.**
  `GET/DELETE /v1/organization/projects/{project_id}/api_keys/{key_id}` exist.
  <https://platform.openai.com/docs/api-reference/project-api-keys/list>
- **No CREATE for project API keys** — creation is dashboard-only.
  <https://developers.openai.com/api/reference/administration/overview>
- **Service accounts ARE programmable.**
  `POST /v1/organization/projects/{project_id}/service_accounts` creates a service
  account and returns its (unredacted) `sk-svcacct-*` key.
  <https://developers.openai.com/api/reference/resources/organization/subresources/projects/subresources/service_accounts/methods/create>

### Workload Identity Federation (WIF)
- **Supported.** Mint short-lived OpenAI bearer tokens by exchanging an external
  OIDC JWT (AWS, GCP, Azure, Kubernetes, GitHub Actions). The resulting token is
  used as an ordinary `Authorization: Bearer` value — interchangeable with API keys
  at the transport layer.
  <https://developers.openai.com/docs/guides/workload-identity-federation>

### Realtime ephemeral tokens
- `POST /v1/realtime/client_secrets` mints short-lived `ek_*` tokens for untrusted
  clients (browser/mobile).
  <https://platform.openai.com/docs/api-reference/realtime-sessions/create-realtime-client-secret>
- ⚠️ **TTL is ambiguous.** Docs describe a configurable TTL (default commonly cited
  as ~1 min), but community reports show a real-world ~2-hour hardcoded default and
  no published *upper bound*. Verify empirically before relying on a TTL value.
  <https://community.openai.com/t/question-about-ephemeral-key-ttl-in-realtime-api/1114627>

### Key safety / rotation
- OpenAI recommends rotating keys (~90-day cadence is the common guidance) and
  revoking immediately on leak; **revocation is binary** at the key level.
  <https://help.openai.com/en/articles/5112595-best-practices-for-api-key-safety>

---

## Current versions / APIs (as of research date)

| Surface | Status | Notes / source |
|---|---|---|
| `OPENAI_BASE_URL` env + `base_url`/`baseURL` arg | ✅ live, both SDKs | <https://github.com/openai/openai-python/issues/745> |
| Bearer auth (incl. streaming SSE, WIF) | ✅ live | <https://developers.openai.com/docs/api-reference/authentication> |
| Admin API — project key CREATE | ❌ does not exist | dashboard-only <https://developers.openai.com/api/reference/administration/overview> |
| Admin API — project key LIST/GET/DELETE | ✅ live | <https://platform.openai.com/docs/api-reference/project-api-keys/list> |
| Admin API — service account CREATE (`sk-svcacct-*`) | ✅ live | <https://developers.openai.com/api/reference/resources/organization/subresources/projects/subresources/service_accounts/methods/create> |
| WIF token exchange | ✅ live | <https://developers.openai.com/docs/guides/workload-identity-federation> |
| Realtime ephemeral `ek_*` | ✅ live; ⚠️ TTL unclear | <https://platform.openai.com/docs/api-reference/realtime-sessions/create-realtime-client-secret> |

**env-ctl-side deps (verified in the codebase; current as of early 2025):**
chacha20poly1305 0.10, argon2 0.5, sha2 0.10, hkdf 0.12, rustls 0.23 (ring
backend, frozen Mozilla roots), reqwest 0.12. `reqwest` 0.12 supports bearer auth,
custom base URL, and streaming responses — sufficient for the relay data plane.
*(Re-run `cargo update`/`cargo tree` before release to confirm none have drifted.)*

---

## Security tradeoffs

| Mode | Real key exposure | Scope granularity | Revocation | Client compat | CA / trust setup |
|---|---|---|---|---|---|
| **BaseUrlRepoint** | Daemon-only (best) | Per relay bearer (env-ctl-enforced, ≤24h) | env-ctl revokes bearer instantly; upstream key untouched | SDKs honoring `OPENAI_BASE_URL` | None |
| **NativeSubToken (service acct)** | `sk-svcacct-*` returned unredacted → must be sealed in vault | Per service account / project (coarse) | Binary, per key; no per-op scoping | Any client (it's a real key) | None |
| **ProxyMitm** | Daemon-only | Per relay bearer | Instant (bearer) | Any client incl. hardcoded host | Local CA must be trusted by client |

Notes:
- **BaseUrlRepoint** keeps the upstream credential off the client entirely and lets
  env-ctl enforce its own short-lived (≤24h, USB-gated) bearer policy — the
  strongest fit for the broker model. Residual risk: a relay bearer is a valid
  upstream proxy grant for its TTL; bind it tightly (see open question on token
  binding).
- **NativeSubToken** hands a *real, long-lived OpenAI credential* (until rotated)
  to the holder. Because creation returns the secret unredacted, env-ctl must seal
  it immediately (XChaCha20-Poly1305 keyslot) and never log it. Coarse scope +
  binary revoke make it weaker than the relay bearer for least-privilege.
- **ProxyMitm** requires CA trust on the client; mis-scoped CA = broad MITM risk.
  Keep NameConstraints + DEK-protected CA key as designed.

---

## Concrete guidance for the env-ctl implementation

1. **Provider table:** keep the OpenAI upstream allowlist pinned to
   `{ api.openai.com }` (matches `ARCHITECTURE.md` §6 / FS review fix). For
   service-account minting you additionally need the Admin host
   (`api.openai.com` Admin endpoints) reachable from the daemon only — never the
   client.
2. **Default path = BaseUrlRepoint.** Inject `OPENAI_BASE_URL=https://<local-relay>`
   (+ `OPENAI_PROJECT`/`OPENAI_ORG` if needed) into the spawned env; relay swaps the
   env-ctl bearer for the real key in the `Authorization` header and re-originates
   TLS to `api.openai.com`. Forward the request body and SSE stream unchanged.
3. **Streaming:** the relay must pass through `text/event-stream` responses without
   buffering to keep token streaming intact; do not strip/transform SSE frames.
4. **`ProviderMint::mint_scoped` for OpenAI (Phase 2):** implement against
   **service accounts**, not project keys:
   `POST /v1/organization/projects/{project_id}/service_accounts` using an
   `sk-admin-*` key held only by the daemon. Seal the returned `sk-svcacct-*`
   immediately; emit only an opaque handle to callers. Until then, leave the
   default `Unsupported` (proxy-swap fallback) as in `seam.rs`.
5. **Revocation:** for service-account keys, wire deletion through the Admin API
   (DELETE on the project api_keys / service-account endpoints) so env-ctl
   "burn-on-expiry" can actually invalidate upstream. Relay bearers remain
   instantly revocable locally regardless.
6. **TTL policy:** keep `MAX_BEARER_TTL_SECS = 86400` for relay bearers
   (`ARCHITECTURE.md` §7). If you ever relay-swap Realtime clients, treat the
   ephemeral-token TTL as unknown/possibly ~2h and add a safety re-mint window
   rather than trusting the documented default.
7. **Do not** build a "create project API key" flow — it isn't an API.

---

## Open questions (unresolved in official docs)

1. **Realtime `ek_*` TTL ceiling** — is the ~2h hardcoded, or configurable beyond
   the documented default? Docs and community reports conflict.
   <https://community.openai.com/t/question-about-ephemeral-key-ttl-in-realtime-api/1114627>
2. **Admin GET on a key** — does
   `GET /v1/organization/projects/{project_id}/api_keys/{key_id}` expose a key's
   scope/permissions, or only metadata? (Affects whether env-ctl can audit minted
   creds.)
3. **Per-request base_url / header override** — can SDK clients override
   `base_url`/custom headers per call, or only at client init? (Init-only would
   force per-target client instances in any helper SDK usage.)
4. **Token binding** — does OpenAI bind a bearer to source IP / TLS client cert,
   i.e., is a leaked relay bearer replayable from an unauthorized host? Not
   documented; assume **no binding** and compensate with short TTL + local egress
   controls.
5. **Programmatic project-key roadmap** — is dashboard-only a permanent constraint
   or a planned Admin API addition? No public roadmap found.

---

## Source list
- Authentication: <https://developers.openai.com/docs/api-reference/authentication>
- Agents SDK config (`OPENAI_BASE_URL`): <https://openai.github.io/openai-agents-python/config/>
- Streaming (SSE): <https://developers.openai.com/api/docs/guides/streaming-responses>
- Workload Identity Federation: <https://developers.openai.com/docs/guides/workload-identity-federation>
- Admin APIs guide: <https://developers.openai.com/api/docs/guides/admin-apis>
- Admin API overview (no project-key CREATE): <https://developers.openai.com/api/reference/administration/overview>
- Project API keys (LIST/GET/DELETE): <https://platform.openai.com/docs/api-reference/project-api-keys/list>
- Service account CREATE: <https://developers.openai.com/api/reference/resources/organization/subresources/projects/subresources/service_accounts/methods/create>
- Realtime client secret (ephemeral): <https://platform.openai.com/docs/api-reference/realtime-sessions/create-realtime-client-secret>
- API key safety / rotation: <https://help.openai.com/en/articles/5112595-best-practices-for-api-key-safety>
- SDK `base_url` issue thread: <https://github.com/openai/openai-python/issues/745>
- Key-format overview (secondary): <https://vibekit.bot/openai-api-key-format>
- Ephemeral TTL community thread: <https://community.openai.com/t/question-about-ephemeral-key-ttl-in-realtime-api/1114627>
