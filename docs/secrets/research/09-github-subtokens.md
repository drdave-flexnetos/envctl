# env-ctl research — GitHub native scoped sub-tokens

> Research date: 2026-06-02. Assistant knowledge cutoff Jan 2026; all version/API claims below verified against the live web on the research date. Items that could not be verified are flagged **OPEN** or **UNVERIFIED**.

---

## TL;DR — recommendation for env-ctl

- **Use GitHub App installation access tokens** for the GitHub provider in env-ctl's `NativeSubToken` swap mode. They are the **only** native GitHub upstream API that mints scoped, short-lived credentials on demand.
- **Do NOT use fine-grained PATs for broker minting.** Fine-grained PATs **cannot be created via any REST/GraphQL API** — they are UI-only. They are usable only as a long-lived *master* credential, not as broker-minted sub-tokens.
- **TTL is fixed at 1 hour** for installation tokens. This sits comfortably under env-ctl's `<=24h USB-gated relay bearer` ceiling; the broker can re-mint at swap time. Early revocation is supported via `DELETE /installation/token`.
- **Token format is changing mid-2026.** A new stateless JWT-style `ghs_` format is rolling out April 27 → late June 2026. env-ctl's broker must accept **both** the legacy opaque and new stateless formats. Suggested detector regex: `ghs_[A-Za-z0-9.\-_]{36,}` (stateless has 2 dots; legacy opaque has none).
- **Per-repo scoping** via `repositories` / `repository_ids` (max 500 repos), constrained by undocumented "token complexity" limits (Feb 2024). Relay profiles should keep scope modest and have a fallback.
- **Rust client:** `octocrab` 0.49.7 (latest released) or `octorust` 0.10.0, plus `jsonwebtoken` 10.4.0 (RS256 via `rsa` backend) for App JWT signing.

---

## Key facts (with inline sources)

### GitHub App installation tokens (the recommended path)

- Minted via `POST /app/installations/{installation_id}/access_tokens`, authenticated with a short-lived **RS256 JWT** signed by the App private key — https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app
- **TTL = 1 hour, fixed** (not 8h) — https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app
- **Per-repository scoping** via `repositories` (names) or `repository_ids`, **max 500 repos** per token; `permissions` can further down-scope below the installation's grant — https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app
- **Early revocation:** `DELETE /installation/token` invalidates the token before its 1h expiry — https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/token-expiration-and-revocation
- **App-auth JWT claims:** `iat` (recommend 60s in the past for clock skew), `exp` (max 10 min in the future), `iss` (App ID or client ID), `alg` = `RS256` (mandatory) — https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app
- **Token complexity limits** (Feb 22, 2024): requests combining many repos with many permissions may be rejected. Workarounds: reduce repo count, reduce permission count, request **all** repos (drop the explicit list), or omit `permissions`. GitHub states the current limit is roughly 8x above any known app's needs (no exact numeric limit published) — https://github.blog/changelog/2024-02-22-new-limits-on-scoped-token-creation-for-github-apps/
- **Resource restrictions:** App tokens cannot reach some enterprise-level resources (e.g. `GET /enterprise/settings/license`); certain git operations require a *user* access token rather than an installation token — https://docs.github.com/en/apps/creating-github-apps/about-creating-github-apps/deciding-when-to-build-a-github-app

### New stateless installation token format (mid-2026 rollout)

- New format `ghs_APPID_JWT` (~520 chars, two `.` separators), self-describing/stateless — rolling out **Phase 1 from April 27, 2026** — https://github.blog/changelog/2026-04-24-notice-about-upcoming-new-format-for-github-app-installation-tokens/
- **Per-request override** via `X-GitHub-Stateless-S2S-Token: enabled` header to opt a single request into the new format during transition. This header is **temporary and slated for deprecation** ("coming weeks") — do NOT hard-depend on it — https://github.blog/changelog/2026-05-15-github-app-installation-tokens-per-request-override-header/
- Phase 2 (all installation tokens default to the new format) runs mid-May → late June 2026 — https://github.blog/changelog/2026-04-24-notice-about-upcoming-new-format-for-github-app-installation-tokens/

### Fine-grained PATs (NOT usable for broker minting)

- **Generally available since March 18, 2025** — https://github.blog/changelog/2025-03-18-fine-grained-pats-are-now-generally-available/
- **Cannot be created via REST API.** The `/rest/orgs/personal-access-tokens` endpoints are governance-only: list / approve / deny / revoke — there is no create endpoint — https://docs.github.com/en/rest/orgs/personal-access-tokens
- **No introspection endpoint.** Classic PATs expose scopes via the `X-OAuth-Scopes` response header; fine-grained PATs do not surface their permissions this way — https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens
- **GraphQL supported** for fine-grained PATs (and GitHub Apps) since April 27, 2023 — https://github.blog/changelog/2023-04-27-graphql-improvements-for-fine-grained-pats-and-github-apps/
- **Known limitations:** no multi-org access in a single token, outside-collaborator push limitations, some enterprise APIs unsupported — https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens

### Reference implementation

- **Octo-STS** (Chainguard) mints GitHub App tokens via OIDC federation, validating `.github/chainguard/*.sts.yaml` trust policies. Demonstrates that stateless JWT + 1h TTL + per-repo scoping is a viable production STS pattern — https://github.com/octo-sts/app

---

## Current versions / APIs (Rust toolchain), as of 2026-06-02

| Crate | Version | Role for env-ctl | Source |
|-------|---------|------------------|--------|
| `octocrab` | **0.49.7** (released ~Mar 30, 2026; docs.rs stable lists 0.49.3) | GitHub API client with App JWT auth + installation token minting | https://github.com/XAMPPRocky/octocrab/releases · https://docs.rs/crate/octocrab/latest |
| `octorust` | **0.10.0** (latest, Jun 2026) | Alternate GitHub client, supports App auth | https://docs.rs/octorust/latest/octorust/ |
| `jsonwebtoken` | **10.4.0** (latest) | RS256 JWT signing for App auth (pair with `rsa`) | https://docs.rs/jsonwebtoken/latest/jsonwebtoken/ |

**Version caution:** Earlier drafts cited `octocrab 0.49.9`. That version is **not** an officially released tag (it appears only as an "upstream" reference in the Debian package tracker). Pin to **0.49.7** until a newer release is confirmed on the GitHub releases page — https://github.com/XAMPPRocky/octocrab/releases · https://tracker.debian.org/pkg/rust-octocrab

**Key endpoints:**
- `POST /app/installations/{installation_id}/access_tokens` — mint scoped installation token (1h TTL)
- `DELETE /installation/token` — early-revoke the current installation token
- `GET /app/installations` — enumerate installations to find the right `installation_id`

---

## Security tradeoffs

- **Fixed 1h TTL is a feature, not a limit.** It caps blast radius on leak. env-ctl's broker re-mints per swap, so the user-facing relay bearer (`<=24h`) can outlive a single 1h upstream token; the broker refreshes transparently. Add early `DELETE /installation/token` on `relay_revoke` for fast kill.
- **Stateless tokens cannot be remotely revoked the same way.** The new stateless `ghs_` format is self-validating; this is a deliberate scalability tradeoff GitHub is making. Confirm revocation semantics for the new format before relying on `DELETE` post-Phase-2 (**OPEN**, see below).
- **App private key is the crown jewel.** Whoever holds the App private key can mint tokens for every installation. env-ctl must store the App private key inside the same XChaCha20-Poly1305 / argon2id-protected vault as other master credentials, never on disk in plaintext, and gate minting behind USB-partition-UUID unlock like other master creds.
- **Down-scoping is best-effort, not enforced upstream beyond the installation grant.** The installation's granted permissions are the hard ceiling; `permissions`/`repositories` in the mint request only narrow within that ceiling. Over-broad installation grants undermine per-relay scoping — keep installations narrowly scoped at install time.
- **No introspection for fine-grained PATs** means env-ctl cannot programmatically validate the scope of a user-supplied FGPAT master credential. Treat FGPAT scope as opaque/declared, not verifiable.
- **Token complexity rejection is a runtime failure mode.** A scoped mint can fail if the (repos x permissions) request is too "complex." env-ctl must handle the error gracefully (fallback below), not surface a hard relay failure.

---

## Concrete guidance for the env-ctl implementation

This aligns with `ARCHITECTURE.md §6`, where `NativeSubToken` is defined for "providers that mint scoped sub-creds (GitHub fine-grained PAT / App token...)". **Correction for that doc:** the "fine-grained PAT" path is **not API-mintable**; GitHub App tokens are the only viable native sub-token source.

1. **Provider mode:** GitHub provider runs in `NativeSubToken` swap mode using **App installation tokens**. The master credential is the **App private key** (+ App ID + installation ID), stored in the vault.
2. **Mint seam (`ProviderMint`):** Build an RS256 JWT with `iat` (now − 60s), `exp` (now + 9 min, under the 10-min cap), `iss` = App ID; sign with `jsonwebtoken` 10.4.0 + `rsa`. Call `POST /app/installations/{id}/access_tokens` with `repository_ids` and `permissions` from the relay profile.
3. **Dual-format handling:** Accept both legacy opaque and new stateless `ghs_` tokens. Detector: `ghs_[A-Za-z0-9.\-_]{36,}` (2 dots ⇒ stateless; 0 dots ⇒ legacy). Do not parse/validate the token body in the broker — pass through to the HTTPS relay data plane.
4. **Do NOT depend on `X-GitHub-Stateless-S2S-Token`** — it is transitional and will be removed.
5. **Scoping policy:** Map each relay profile to an explicit `repository_ids` set and minimal `permissions`. Keep repo count well under 500 and avoid max-permissions + max-repos combos to dodge complexity rejection.
6. **Complexity fallback:** On a complexity-limit error, fall back to (a) fewer repos, (b) fewer permissions, or (c) an unscoped (all-repos) installation token, and emit a relay-policy warning. Never silently widen scope without logging.
7. **Revocation:** Wire `relay_revoke` to `DELETE /installation/token` for early kill; default behavior remains 1h expiry. Verify this still works for stateless tokens after Phase 2 (OPEN).
8. **Refresh cadence:** Re-mint per relay swap or when the cached token is within ~5 min of `exp`. The 1h TTL means a long relay session needs periodic re-mint behind the user's longer-lived bearer.
9. **Client lib:** Pin `octocrab = "0.49.7"`; verify its App-auth/JWT module is production-ready, or use raw `reqwest` + `jsonwebtoken` if finer control over headers/format is needed during the format transition.
10. **Rate-limit budget:** Plan for, but verify, the installation rate limit (5,000/hr ≈ 1.4 tokens/sec) on the mint endpoint (OPEN — see below).

---

## Open questions (flagged unverified)

- **OPEN — Rate limit on `POST /app/installations/{id}/access_tokens`.** Not explicitly documented. The general GitHub App installation rate limit (5,000/hr + scaling) likely applies, but the mint endpoint itself is not called out — https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/rate-limits-for-github-apps . **Action:** test empirically under prod load or confirm with GitHub. Assume 5,000/hr as a conservative upper bound.
- **OPEN — Deprecation date for `X-GitHub-Stateless-S2S-Token`.** GitHub says "coming weeks" with no firm date — https://github.blog/changelog/2026-05-15-github-app-installation-tokens-per-request-override-header/
- **OPEN — Revocation semantics for stateless `ghs_` tokens.** Whether `DELETE /installation/token` reliably invalidates a self-validating stateless token before `exp` is not clearly documented post-Phase-2. Verify before relying on early revocation.
- **UNVERIFIED — Exact REST/GraphQL coverage gaps for fine-grained PATs as of Jun 2026.** GitHub documents endpoint-by-endpoint support piecemeal; no single authoritative matrix found.
- **UNVERIFIED — Whether a future fine-grained PAT *creation* API is on the roadmap.** GitHub's security posts mention intent to add approve/revoke APIs but list no creation endpoint and no committed timeline. Treat programmatic FGPAT minting as nonexistent until GitHub ships it.
