# env-ctl research — Sender-constrained tokens without peer-cred

> Scope: how to bind a relay bearer to a specific *remote* caller when `SO_PEERCRED` is unavailable (i.e. over the HTTPS data plane, not the local UDS control plane).
> Verified against the live web on 2026-06-02. Anything unverified is flagged inline with ⚠.

---

## TL;DR — recommendation for env-ctl

1. **Keep `SO_PEERCRED` uid/pid binding as the primary constraint for the local plane** (UDS control + any local-loopback relay). It is the strongest binding env-ctl has and it costs nothing. Its residual — PID reuse — is already acknowledged in the threat model (A2/A10) and bounded by the ≤24h ceiling + allowlist + quota + audit, not prevented.
2. **For genuinely *remote* callers (no UDS, no peer-cred), do NOT ship a plain bearer.** A bearer over HTTPS is replayable for its full TTL by anyone who captures it. Add a **cryptographic sender constraint**. Two standards-track options:
   - **DPoP (RFC 9449)** — app-layer proof-of-possession JWT, signed per request with a key the client holds. Best fit for env-ctl: no PKI rollout, works through the local-CA MITM relay, ~1–5 ms/req CPU. **Recommended default for remote.**
   - **mTLS-bound tokens (RFC 8705, `x5t#S256`)** — strongest, but requires client-cert provisioning and is undone by the corporate-MITM scenario env-ctl explicitly tolerates. **Reserve for a future hardened mode.**
3. **Do not revive Token Binding (RFC 8471).** It is dead: Chrome removed it in 2024 and the ecosystem never coalesced.
4. **The bearer is still a password.** Sender-constraint reduces replay but does not eliminate the "owner-session malware steals the live key/proof" case. Keep the existing blast-radius controls (24h max, allowlist, quota, durable audit, USB-pull grace drain). For the Telegram cloud-agent path, never co-locate the relay bearer and the Telegram bot token in one process — both are passwords.

---

## Key facts (with inline sources)

### The bearer-token replay problem
- A bearer token is, by definition, usable by *anyone who holds it* — there is no proof the presenter is the legitimate party. Sender-constrained (a.k.a. proof-of-possession) tokens fix this by binding the token to a key the client must demonstrate it controls. ([WorkOS: sender-constrained OAuth](https://workos.com/blog/mtls-dpop-token-binding-sender-constrained-oauth))
- Stateless replay detection is impossible: with a plain bearer the only defenses are short TTL + nonce/jti tracking + signature verification, all of which require either a constraint mechanism or server-side state. ([Obsidian: token replay attacks](https://www.obsidiansecurity.com/blog/token-replay-attacks-detection-prevention))
- The replay window equals the token lifetime. A 24h bearer is "long-lived" by OAuth norms (best practice for short-lived access tokens is 5–15 min). ([Obsidian: refresh token best practices](https://www.obsidiansecurity.com/blog/refresh-token-security-best-practices))
- Theft is not theoretical: infostealer malware exfiltrated ~**17 billion browser cookies in 2024**, and a stolen session/bearer token bypasses passwords *and* MFA. ([Obsidian: what is token theft](https://www.obsidiansecurity.com/blog/what-is-token-theft-oauth-session-api-token-attacks-explained))
- In autonomous-agent systems the risk compounds: credential replay happens at machine speed, multiplying exposure. ([Strata: agent credential replay](https://www.strata.io/blog/agentic-identity/agent-credential-replay/))

### DPoP — RFC 9449 (the app-layer option)
- **RFC 9449, "OAuth 2.0 Demonstrating Proof of Possession (DPoP)", published September 2023.** ([RFC 9449](https://www.rfc-editor.org/rfc/rfc9449.html))
- Mechanism: client sends a `DPoP` header per request containing a JWT with `typ: "dpop+jwt"`, the public `jwk`, and claims `jti` (unique id), `htm` (HTTP method), `htu` (HTTP URI), `iat`. The token is bound to the key via `cnf.jkt` = the SHA-256 JWK thumbprint (RFC 7638). ([RFC 9449](https://www.rfc-editor.org/rfc/rfc9449.html); [Auth0: DPoP](https://auth0.com/docs/secure/sender-constraining/demonstrating-proof-of-possession-dpop))
- The proof binds the *request context* (method + URI), so a captured proof can't be replayed against a different endpoint. Replay against the *same* endpoint is blocked by `jti` tracking and an optional server-issued `nonce` (which also absorbs clock drift). ([Auth0: protect access tokens with DPoP](https://auth0.com/blog/protect-your-access-tokens-with-dpop/))
- Clock tolerance: RFC 9449 permits `iat` "reasonably near" the present; sub-minute drift is typical and the server `nonce` handles larger skew. ([RFC 9449](https://www.rfc-editor.org/rfc/rfc9449.html))
- Latency: app-layer signing adds roughly **2–10 ms/request** depending on backend; ⚠ this range is implementation-specific (some sources report ~1.3 ms client / ~0.5 ms server). For env-ctl, negligible vs network RTT. ([Carrier Integrations: DPoP in production](https://www.carrierintegrations.com/sender-constrained-tokens-how-dpop-solves-the-bearer-token-security-crisis-in-production-carrier-api-integrations/))

### mTLS-bound tokens — RFC 8705 (the PKI option)
- **RFC 8705, "OAuth 2.0 Mutual-TLS Client Authentication and Certificate-Bound Access Tokens."** Binds a token to the client cert via the `x5t#S256` confirmation (cert SHA-256 thumbprint). ([RFC 8705](https://datatracker.ietf.org/doc/html/rfc8705))
- Strongest constraint (binding happens at the TLS layer) but requires provisioning and managing client certificates.
- ⚠ Latency claims of "5–15 ms/connection" appear in vendor blogs but lack a precise authoritative cite; handshake cost is dominated by implementation (rustls vs native-tls) and connection reuse.

### Token Binding — RFC 8471 (do NOT use)
- **RFC 8471, "The Token Binding Protocol Version 1.0"** — bound tokens to the TLS connection. ([RFC 8471](https://www.rfc-editor.org/rfc/rfc8471))
- **Deprecated in practice: Chrome removed Token Binding support in version 130 (2024); the ecosystem never coordinated adoption.** ([WorkOS: sender-constrained OAuth](https://workos.com/blog/mtls-dpop-token-binding-sender-constrained-oauth))

### Standards posture
- **FAPI 2.0 Security Profile (Final)** mandates that access tokens be sender-constrained via **mTLS *or* DPoP** — bearer-only is not compliant. ([FAPI 2.0 Security Profile](https://openid.net/specs/fapi-security-profile-2_0-final.html))
- **OAuth 2.1** recommends sender-constrained tokens, especially for public clients. ([WorkOS: DPoP / RFC 9449 explained](https://workos.com/blog/dpop-rfc-9449-explained))
- ⚠ A claim that the Model Context Protocol (MCP) spec explicitly requires "sender-constrained tokens" could **not** be verified in accessible MCP docs as of 2026-06-02 — treat as unconfirmed.

### Why `SO_PEERCRED` doesn't reach remote callers (and its own residual)
- `SO_PEERCRED` returns the peer's `{pid, uid, gid}` at connect time and is **AF_UNIX only** — it does not exist on `AF_INET`, so it cannot constrain a remote HTTPS caller. ([man7: unix(7)](https://man7.org/linux/man-pages/man7/unix.7.html))
- Even on the local plane, the PID it returns can be **reused** after the original process exits, creating a TOCTOU/identity-confusion race. ([CVE-2020-25653 (spice-vdagent)](https://bugzilla.redhat.com/show_bug.cgi?id=1886372))
- gRPC over UDS can rely on local peer authentication instead of TLS for exactly this reason; the security model changes the moment the socket is no longer local. ([gRPC auth guide](https://grpc.io/docs/guides/auth/))

### Certificate pinning vs corporate MITM
- Pinning defeats generic MITM, but **corporate TLS-inspection appliances present a re-signed cert with a *different* public key**, which breaks static pinning — so pinning cannot be relied on where corporate MITM is in scope. ([OWASP: certificate & public key pinning](https://owasp.org/www-community/controls/Certificate_and_Public_Pinning); [Indusface: SSL pinning](https://www.indusface.com/learning/what-is-ssl-pinning-a-quick-walk-through/))

---

## Current versions / APIs (Rust, verified 2026-06-02)

| Component | Status | Source |
|---|---|---|
| `jsonwebtoken` | v10.4.0; supports RS256/PS256, ES256, EdDSA (Ed25519) — suitable for DPoP proof signing/verification | [docs.rs/crate/jsonwebtoken](https://docs.rs/crate/jsonwebtoken/latest); [github.com/Keats/jsonwebtoken](https://github.com/Keats/jsonwebtoken) |
| `rustls` | **0.24+ requires explicitly selecting a `CryptoProvider`** (`aws-lc-rs` *or* `ring`) — no implicit default. env-ctl must pin one at startup. | [docs.rs rustls CryptoProvider](https://docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html); [Google Cloud: switch rustls crypto provider](https://docs.cloud.google.com/rust/switch-rustls-crypto-provider) |
| `aws-sigv4` | available — canonical AWS SigV4 request-signing impl, if a SigV4-style HMAC scheme is preferred over DPoP for some upstreams | [crates.io/crates/aws-sigv4](https://crates.io/crates/aws-sigv4); [docs.rs/aws-sigv4](https://docs.rs/aws-sigv4/latest/aws_sigv4/) |
| `subtle` | already used in env-ctl `token.rs` for constant-time MAC comparison | [token.rs](file:///home/drdave/Desktop/env-ctl/crates/secrets-engine/src/broker/token.rs) |

DPoP key recommendation: **Ed25519 (`EdDSA`)** — small keys, fast verify, supported by `jsonwebtoken` 10.x.

---

## Security tradeoffs

| Mechanism | Binds to | Strength | Cost / friction | MITM-relay compatible? | Fit for env-ctl |
|---|---|---|---|---|---|
| `SO_PEERCRED` uid/pid | local process | Strong locally; **zero remote reach**; PID-reuse residual | ~0 | N/A (local only) | **Primary, local plane** |
| HMAC request signing (keyed-MAC, current `token.rs`) | a shared secret + request context | Good if secret stays secret; still a "password" | very low (~µs verify) | Yes | **Secondary, all planes** |
| DPoP (RFC 9449) | a client-held key (`jkt`) + per-request method/URI | Strong PoP; needs `jti`/nonce state for same-endpoint replay | ~1–5 ms/req CPU; needs replay store | **Yes** (app-layer, survives the local CA) | **Recommended for remote** |
| mTLS-bound (RFC 8705) | client TLS cert (`x5t#S256`) | Strongest | client-cert PKI provisioning | **No** (broken by corporate MITM env-ctl tolerates) | Future hardened mode only |
| Token Binding (RFC 8471) | TLS connection | (was strong) | — | — | **Do not use — deprecated** |

Cross-cutting truths:
- **None of these defeat owner-session malware** that reads the live key/proof material out of the agent's own memory. They shrink the *replay* and *exfiltration-reuse* windows; the threat model already states A2/A10 are *bounded, not prevented*.
- **Replay defense = TTL + nonce/`jti` + signature**, and at least one of those needs server-side state. env-ctl already keeps a store, so `jti` dedup is feasible. ([Obsidian: token replay](https://www.obsidiansecurity.com/blog/token-replay-attacks-detection-prevention))
- **24h is a deliberate compromise**: short enough to bound a compromised client (A7), long enough to avoid constant USB-gated relay re-mints. Treat it as a ceiling, not a default.

---

## Concrete guidance for the env-ctl implementation

**Layer the constraints; do not rely on DPoP (or anything) alone.**

1. **Local plane (UDS control + loopback relay): keep `SO_PEERCRED`.**
   - Already deployed. Continue using `{uid, pid}` capture at swap time per ARCHITECTURE §6 ("peer-bound at swap time").
   - Mitigate PID reuse the way the threat model already specifies: monotonic issuance floor + `last_seen_ms` high-water mark + `CLOCK_BOOTTIME` (THREAT-MODEL §5). Effective vs accidental skew; **not** vs owner-session malware — keep documenting it honestly.

2. **Remote plane (HTTPS data plane): add DPoP-style proof-of-possession.**
   - At bearer mint, capture the client's **Ed25519 public-key thumbprint** into the token's confirmation claim (`cnf.jkt`, RFC 7638 SHA-256).
   - Require a `DPoP` proof JWT per request: verify `typ=dpop+jwt`, the embedded `jwk` matches `cnf.jkt`, `htm`/`htu` match the actual request, and `iat` is within tolerance.
   - **`jti` replay store**: dedup per-`jti` for the proof's validity window. env-ctl already has a libSQL store; a small TTL-indexed table (or in-RAM map keyed by `jti`, evicted past the window) suffices. Issue a server `nonce` to cap clock drift.
   - Sign/verify with `jsonwebtoken` 10.x (`EdDSA`). Verify in constant time where comparing secrets; use `subtle` (already in `token.rs`).

3. **Keep HMAC request signing as the universal floor.**
   - The existing keyed-MAC in `token.rs` (constant-time via `subtle`) should bind host + method + URI + timestamp, so even non-DPoP callers get request-context binding. Replay window = token TTL (≤24h, per-relay policy).

4. **Single TTL choke point — unchanged.**
   - Continue funneling every mint through `clamp_ttl(now, policy_ttl_secs, requested_ttl_secs) = now + requested.clamp(1, policy_ttl).min(MAX_BEARER_TTL_SECS)` with `MAX_BEARER_TTL_SECS = 86400`, backed by the storage `CHECK` constraint (ARCHITECTURE §7). Do not add a second TTL path for DPoP/remote bearers.

5. **USB-gated mint — unchanged.**
   - Keep proving keyfile *possession* cryptographically; use the partition UUID only as a fast pre-filter, never as the proof (ARCHITECTURE §7).

6. **Upstream verification — unchanged.**
   - Real-key upstream calls must keep refusing the local CA and OS trust store and use only the **frozen webpki-roots / Mozilla bundle** (ARCHITECTURE §6, FS-S7). DPoP/mTLS choices here are the *upstream provider's* requirement, not env-ctl's relay-internal scheme.

7. **mTLS-bound tokens (RFC 8705): defer.**
   - Cleaner than DPoP only if env-ctl ships client-cert provisioning *and* the corporate-MITM scenario is out of scope. Since env-ctl deliberately tolerates corporate MITM (local CA, in-RAM, ≤24h, relay-scoped per ARCHITECTURE §8 / FS-S6), mTLS binding would be silently defeated. Park it as a "hardened mode" for MITM-free deployments.

8. **Telegram cloud-agent path: hard isolation.**
   - The relay bearer and the Telegram bot token are **both passwords**; a leaked bot token allows full impersonation, and Telegram is an active malware C2 channel (Agent Tesla). ([GitGuardian: Telegram bot token](https://www.gitguardian.com/remediation/telegram-bot-token); [Cofense: weaponizing Telegram bots](https://cofense.com/blog/weaponizing-telegram-bots-how-threat-actors-exfiltrate-credentials))
   - Never pass a relay bearer and a Telegram token into the same process. Keep them in separate vaults with independent rotation; if the agent has no stable IP, DPoP (key bound at mint, proof per request) is the right remote constraint here since IP/peer-cred binding is unavailable.

**Residual risks to keep documented (per threat model):**
- **A2 (owner-session malware):** bounded by 24h + allowlist + quota + durable audit + USB-pull grace drain. *Not prevented.*
- **A10 (plain-HTTP bearer replay / same-uid replay):** bounded by allowlist + quota + TTL; bearer secrecy alone is insufficient. DPoP/HMAC binding shrinks this further but does not close owner-session theft.
- **Clock rollback:** mitigated by monotonic floor + high-water mark; ineffective vs owner-session malware.

---

## Open questions

1. **DPoP `jti` store sizing & eviction.** What proof validity window (and thus `jti` retention) balances replay protection against store growth at agent/machine speed? Needs a load model.
2. **DPoP vs HMAC-SigV4 for remote.** Both give request-context binding; DPoP adds asymmetric PoP (key never leaves client) at higher CPU. Is the asymmetric guarantee worth it for env-ctl's threat model, or is keyed-MAC sufficient given the 24h ceiling? Decision pending.
3. **MCP sender-constraint requirement.** ⚠ Unverified — does the current MCP auth spec actually mandate sender-constrained tokens? Confirm before claiming compliance.
4. **mTLS latency budget.** ⚠ The "5–15 ms/connection" figure is unsourced; if a hardened mTLS mode is ever pursued, benchmark rustls (with chosen `CryptoProvider`) under connection reuse rather than trusting vendor numbers.
5. **Server `nonce` rollout.** Is the extra round-trip (server issues `DPoP-Nonce`, client retries) acceptable for the relay's latency profile, or should env-ctl rely solely on a tight `iat` window + `jti` dedup?
6. **Key custody for DPoP.** Where does the client's DPoP private key live for a headless cloud agent, and does that custody reintroduce the very exfiltration risk DPoP is meant to mitigate? (If the key sits next to the bearer, the constraint adds little.)

---

### Sources
- [RFC 9449 — DPoP](https://www.rfc-editor.org/rfc/rfc9449.html)
- [RFC 8705 — mTLS / certificate-bound tokens](https://datatracker.ietf.org/doc/html/rfc8705)
- [RFC 8471 — Token Binding](https://www.rfc-editor.org/rfc/rfc8471)
- [FAPI 2.0 Security Profile (Final)](https://openid.net/specs/fapi-security-profile-2_0-final.html)
- [WorkOS — sender-constrained OAuth (Token Binding vs DPoP vs mTLS)](https://workos.com/blog/mtls-dpop-token-binding-sender-constrained-oauth)
- [WorkOS — DPoP / RFC 9449 explained](https://workos.com/blog/dpop-rfc-9449-explained)
- [Auth0 — DPoP docs](https://auth0.com/docs/secure/sender-constraining/demonstrating-proof-of-possession-dpop)
- [Auth0 — protect access tokens with DPoP](https://auth0.com/blog/protect-your-access-tokens-with-dpop/)
- [Carrier Integrations — DPoP in production (latency)](https://www.carrierintegrations.com/sender-constrained-tokens-how-dpop-solves-the-bearer-token-security-crisis-in-production-carrier-api-integrations/)
- [rustls — CryptoProvider](https://docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html)
- [Google Cloud — switch rustls crypto provider](https://docs.cloud.google.com/rust/switch-rustls-crypto-provider)
- [jsonwebtoken — docs.rs](https://docs.rs/crate/jsonwebtoken/latest) · [GitHub Keats/jsonwebtoken](https://github.com/Keats/jsonwebtoken)
- [aws-sigv4 — crates.io](https://crates.io/crates/aws-sigv4) · [docs.rs](https://docs.rs/aws-sigv4/latest/aws_sigv4/)
- [man7 — unix(7) / SO_PEERCRED](https://man7.org/linux/man-pages/man7/unix.7.html)
- [CVE-2020-25653 — SO_PEERCRED PID reuse race](https://bugzilla.redhat.com/show_bug.cgi?id=1886372)
- [Obsidian — token replay attacks](https://www.obsidiansecurity.com/blog/token-replay-attacks-detection-prevention)
- [Obsidian — refresh token best practices](https://www.obsidiansecurity.com/blog/refresh-token-security-best-practices)
- [Obsidian — what is token theft (2024 infostealer stats)](https://www.obsidiansecurity.com/blog/what-is-token-theft-oauth-session-api-token-attacks-explained)
- [Strata — agent credential replay](https://www.strata.io/blog/agentic-identity/agent-credential-replay/)
- [gRPC — authentication guide](https://grpc.io/docs/guides/auth/)
- [OWASP — certificate & public key pinning](https://owasp.org/www-community/controls/Certificate_and_Public_Pinning)
- [Indusface — SSL pinning](https://www.indusface.com/learning/what-is-ssl-pinning-a-quick-walk-through/)
- [GitGuardian — Telegram bot token remediation](https://www.gitguardian.com/remediation/telegram-bot-token)
- [Cofense — weaponizing Telegram bots (Agent Tesla C2)](https://cofense.com/blog/weaponizing-telegram-bots-how-threat-actors-exfiltrate-credentials)
- env-ctl design docs: [THREAT-MODEL.md](/home/drdave/Desktop/env-ctl/docs/THREAT-MODEL.md), [ARCHITECTURE.md](/home/drdave/Desktop/env-ctl/docs/ARCHITECTURE.md), [token.rs](/home/drdave/Desktop/env-ctl/crates/secrets-engine/src/broker/token.rs)
