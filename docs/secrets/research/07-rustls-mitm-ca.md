# env-ctl research — rustls intercepting proxy + on-the-fly leaf certs

> Scope: how to build a local TLS-intercepting (MITM) data plane in pure Rust for
> env-ctl's credential-broker relay, using a private CA and per-SNI leaf certs
> minted on the fly, while keeping upstream egress verification cryptographically
> intact.
>
> Currency: versions/APIs verified against the live web as of **2026-06-02**.
> Assistant knowledge cutoff was Jan 2026; everything dated after that was
> re-checked. Unverifiable claims are explicitly flagged **[UNVERIFIED]**.

---

## TL;DR — recommendation for env-ctl

1. **Stack**: `rustls 0.23.x` (server side, for the local intercept listener) +
   `rcgen 0.14.x` (mint CA + leaf certs) + `hyper 1.x` + a `moka` cert cache.
   This is exactly the proven shape used by `hudsucker` and `http-mitm-proxy`.
   See [hudsucker `RcgenAuthority`](https://docs.rs/hudsucker/latest/hudsucker/certificate_authority/struct.RcgenAuthority.html)
   and [http-mitm-proxy](https://docs.rs/http-mitm-proxy).

2. **Per-SNI leaf minting**: implement `rustls::server::ResolvesServerCert` and
   read the SNI from `ClientHello` to pick/mint the leaf at handshake time, before
   any application data is decrypted. Trait signature is
   `resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>>`
   ([docs.rs](https://docs.rs/rustls/latest/rustls/server/trait.ResolvesServerCert.html)).
   If you need async I/O (e.g. fetch the *upstream* cert to copy SANs) use the
   [`Acceptor`](https://docs.rs/rustls/latest/rustls/server/struct.Acceptor.html)
   `read_tls()` + `accept()` polling pattern instead.

3. **CRITICAL upstream-trust safeguard (FS-S7)**: env-ctl's relay must verify the
   *real* upstream server with `webpki-roots` **only** — never the OS trust store.
   `rustls-platform-verifier` and reqwest's `rustls-tls-native-roots` feature
   silently trust the operating-system CA store, which defeats env-ctl's threat
   model (a poisoned OS store would let the relay accept a forged upstream).
   - Use `reqwest` with feature `rustls-tls` (webpki-roots), **not**
     `rustls-tls-native-roots`.
   - Add a `cargo deny` CI gate that rejects `rustls-platform-verifier` and any
     `*-native-roots` feature in the dependency tree.
   - Source: [rustls-platform-verifier](https://github.com/rustls/rustls-platform-verifier)
     ("uses the operating system's verifier").

4. **Pin** these (current as of 2026-06-02):
   `rustls 0.23.40`, `rcgen 0.14.7`, `webpki-roots 1.0.7`, `hyper 1.x`.

---

## Key facts (with inline sources)

### TLS interception architecture
- A TLS MITM proxy terminates the client TLS with a leaf cert it mints itself
  (signed by a CA the client has been told to trust), then opens a *separate*
  outbound TLS connection to the real upstream. The leaf is minted **per
  destination**, keyed on SNI. mitmproxy documents the canonical flow: it pauses
  after reading the client SNI, connects upstream to fetch the real cert, then
  mints a matching leaf — so the SANs line up.
  [mitmproxy: how it works](https://docs.mitmproxy.org/stable/concepts/how-mitmproxy-works/).
- The leaf cert is selected/minted **during the ClientHello**, i.e. *before* the
  application-layer HTTP request (and its `Host` header) is ever decrypted. This
  means SNI is your only routing signal at cert-selection time — a fact with
  direct security consequences (see SNI/Host confusion below).

### Private CA requirements
- A CA cert that can sign leaves needs X.509v3 `basicConstraints` with `CA:TRUE`
  and `keyUsage` including `keyCertSign`. Standard X.509 config.
  [OpenSSL x509v3_config](https://docs.openssl.org/1.0.2/man1/x509v3_config/).
- In rcgen this is the `is_ca` field (`IsCa` enum, `Ca(...)` vs `SelfSignedOnly`)
  on `CertificateParams`.
  [rcgen `CertificateParams`](https://docs.rs/rcgen/latest/rcgen/struct.CertificateParams.html).
- Ensure the CA cert carries a Subject-Key-Identifier and that issued leaves
  carry the matching Authority-Key-Identifier. rcgen historically mishandled this
  when signing with an *externally supplied* CA: see
  [rcgen issue #261](https://github.com/rustls/rcgen/issues/261) (AKI mismatch),
  fixed in the 0.14 line — **verify your generated leaves chain correctly** with
  `openssl verify` during testing.

### Upstream verification (the part that must stay honest)
- For the outbound leg, seed a `RootCertStore` from
  [`webpki-roots`](https://github.com/rustls/webpki-roots) `TLS_SERVER_ROOTS` and
  build a normal `ClientConfig`. This keeps real CA verification intact and
  independent of any local CA or OS store.
- `webpki-roots` uses the "semver trick": the old `0.26.x` line re-exports the
  `1.x` crate, so a `0.26` pin auto-upgrades to `1.x`.
  [webpki-roots releases](https://github.com/rustls/webpki-roots/releases).
- Root-store churn matters for a long-lived relay: DigiCert "G1" root was
  distrusted on the Mozilla/Google schedule and is **removed as of
  2026-04-15** in current root programs.
  [DigiCert G1 removal timeline](https://securityboulevard.com/2026/04/digicert-g1-root-removal-2026-what-it-means-risks-action-plan-for-your-tls-infrastructure/).
  Keep `webpki-roots` updated or you will spuriously reject or wrongly accept
  upstreams as programs evolve.

### Revocation
- rustls does **not** do OCSP/CRL checking out of the box; you must implement it
  separately or rely on a verifier that does.
  [rustls issue #1541](https://github.com/rustls/rustls/issues/1541).
- `rustls-platform-verifier` *does* provide OCSP/CRL — but it gets that by
  delegating to the OS, which (per FS-S7) env-ctl must not trust for upstream.
  Net: if env-ctl wants revocation on the upstream leg, it must wire it in itself
  (stapled OCSP and/or CRL distribution-point fetch), on top of webpki-roots.

---

## Current versions / APIs (verified 2026-06-02)

| Crate | Version | Notes / source |
|-------|---------|----------------|
| `rustls` | **0.23.40** | Latest 0.23.x; released 2026-04-28. [releases](https://github.com/rustls/rustls/releases) |
| `rcgen` | **0.14.7** | Current 0.14.x (June 2026); 0.15 in planning. [features/versions](https://docs.rs/crate/rcgen/latest/features) |
| `webpki-roots` | **1.0.7** | Post-DigiCert-G1-removal (2026-04). [releases](https://github.com/rustls/webpki-roots/releases) |
| `hyper` | **1.x** | Use 1.x (>= 1.5) with `hyper-util`. |
| `moka` | latest | In-memory cert cache (used by http-mitm-proxy). [http-mitm-proxy](https://docs.rs/http-mitm-proxy) |
| `hudsucker` | latest | Reference MITM proxy crate. [RcgenAuthority](https://docs.rs/hudsucker/latest/hudsucker/certificate_authority/struct.RcgenAuthority.html) |

### rustls API surface you will touch (all verified on docs.rs/latest)
- **`ResolvesServerCert`** — `resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>>`.
  Synchronous; runs during the handshake. Use for the common "mint/cache leaf
  keyed on SNI" path.
  [trait docs](https://docs.rs/rustls/latest/rustls/server/trait.ResolvesServerCert.html).
- **`Acceptor`** — `read_tls()` then `accept()` returns a pending handshake you
  can inspect (SNI/ALPN) and complete after doing async I/O. Use when you must
  contact the upstream to copy its SANs *before* choosing the leaf.
  [Acceptor docs](https://docs.rs/rustls/latest/rustls/server/struct.Acceptor.html).
- **`CertifiedKey::from_der(...)`** — constructor verifies the private key matches
  the cert's public key. Good defense against accidentally pairing the wrong key.
  [CertifiedKey docs](https://docs.rs/rustls/latest/rustls/sign/struct.CertifiedKey.html).
- **`ServerConnection::server_name()`** — returns the SNI; `None` until SNI is
  processed, available *before* auth completes.
  [ServerConnection docs](https://docs.rs/rustls/latest/rustls/server/struct.ServerConnection.html).
- **`CryptoProvider`** — 0.23 defaults to `aws-lc-rs`; `ring` is available behind
  a feature flag. Pick one explicitly and pin it.
  [CryptoProvider docs](https://docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html).

### rcgen 0.13+ API notes (verified)
- 0.13 removed implicit key generation: you must call `KeyPair::generate()` (or
  `generate_for(...)` / `generate_rsa_for(...)`) explicitly and pass it in.
  [0.12→0.13 migration](https://github.com/rustls/rcgen/blob/main/rcgen/docs/0.12-to-0.13.md),
  [KeyPair docs](https://docs.rs/rcgen/latest/rcgen/struct.KeyPair.html).
- Backend caveat: the **ring** backend only ingests PKCS#8 DER keys; **aws-lc-rs**
  also accepts PKCS#1 / SEC1. Match this to env-ctl's chosen CA key format.
  [KeyPair docs](https://docs.rs/rcgen/latest/rcgen/struct.KeyPair.html).

---

## Security tradeoffs

| Tradeoff | Detail |
|----------|--------|
| **A CA private key is now a crown jewel** | Anyone with the MITM CA key can forge certs for *any* host the client trusts. In env-ctl, store the CA key inside the same XChaCha20-Poly1305 / argon2id keyslot vault as other secrets; never write it plaintext to disk; consider USB-partition-UUID gating on its unlock, same as relay bearers. |
| **OS-store vs webpki-roots on the upstream leg (FS-S7)** | Trusting the OS store on egress means a single poisoned OS CA silently breaks env-ctl's end-to-end guarantee. webpki-roots-only keeps trust auditable and reproducible. This is the single most important config choice in this doc. [rustls-platform-verifier](https://github.com/rustls/rustls-platform-verifier). |
| **SNI/Host confusion (A9)** | Leaf is chosen on SNI during ClientHello, but the actual HTTP `Host` is only known after decryption. A client (or attacker) can send SNI=`a.com` then `Host: b.com`. Mitigation: in your per-request `decide()` path, re-verify that the decrypted `Host` matches the SNI used to mint the leaf (or matches the upstream you actually dialed), and reject mismatches. This also blunts request-smuggling via `Host` injection. |
| **Cert cache keyed on SNI alone** | A cache that maps SNI→leaf without coupling to the relay/authorization context can be reused across requests that should not share trust. Couple cache eviction to relay revocation (env-ctl's <=24h USB-gated bearer lifetime), so a revoked relay can't keep serving a cached leaf. (General principle, consistent with mitmproxy's per-destination cert model.) |
| **No built-in revocation** | rustls won't OCSP/CRL-check upstreams for you ([#1541](https://github.com/rustls/rustls/issues/1541)). Decide explicitly whether env-ctl needs it; if yes, implement stapled-OCSP/CRL on the egress `ClientConfig`. |
| **Downgrade / cipher surface** | Pin TLS versions and the crypto provider; don't enable legacy ciphers "for compatibility" on the intercept listener. |

---

## Concrete guidance for the env-ctl implementation

**Cargo.toml (pin and constrain features):**
```toml
# Intercept listener (server side) — pick ONE crypto provider and pin it.
rustls       = { version = "0.23.40", default-features = false, features = ["ring", "logging", "std", "tls12"] }
rcgen        = "0.14.7"
webpki-roots = "1.0.7"
hyper        = "1"
hyper-util   = "0.1"
moka         = { version = "0.12", features = ["future"] }

# Upstream egress — webpki-roots ONLY. Do NOT add rustls-tls-native-roots.
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
```
> Note: I picked the `ring` provider above to match env-ctl's stated preference in
> the fact-check; if you instead want the rustls default, drop `ring` and use the
> implicit `aws-lc-rs`. Either is fine — just pick one and be explicit.
> Confirm `ring` supports your CA key's signature algo (P-256/P-384/RSA-2048) —
> see open questions.

**cargo-deny gate (CI must fail the build if these appear):**
```toml
# deny.toml (sketch)
[bans]
deny = [
  { name = "rustls-platform-verifier" },
]
# Also assert no enabled feature named "*native-roots*" in the resolved graph.
# (Enforce via a feature-audit step / cargo-tree grep in CI.)
```

**Leaf-minting flow (per SNI):**
1. On ClientHello, read SNI.
2. Look up `moka` cache: `sni -> Arc<CertifiedKey>`. Hit → return it.
3. Miss → mint: `KeyPair::generate()`, build `CertificateParams` with
   `subject_alt_names = [DnsName(sni)]`, sign with the in-vault CA
   (`CertifiedKey::from_der` to pair), insert into cache, return.
4. If you must mirror the upstream's real SANs, switch to the `Acceptor` pattern:
   pause, dial upstream with the webpki-roots `ClientConfig`, read its leaf,
   copy SANs, then mint and complete the handshake.

**Per-request defense (`decide()`):**
- After decryption, assert `Host` (or `:authority`) matches the SNI that selected
  the leaf and matches the upstream you dialed. Reject on mismatch (A9).
- Tie the request's authorization to the live relay bearer; on bearer revocation,
  evict the corresponding cache entries.

**CA generation checklist:**
- `is_ca = IsCa::Ca(BasicConstraints::Unconstrained)` (or path-len-limited).
- `key_usage` includes `KeyCertSign` (+ `CrlSign` if you issue CRLs).
- Subject-Key-Identifier present; verify issued leaves' Authority-Key-Identifier
  matches (regression-test against [#261](https://github.com/rustls/rcgen/issues/261)).
- Validate a freshly issued leaf chains to the CA with `openssl verify -CAfile`.

---

## Open questions / things to verify before shipping

1. **Ring vs aws-lc-rs signature support for the CA key (OI-?)** — confirm the
   chosen provider signs with env-ctl's CA key algorithm (P-256 / P-384 /
   RSA-2048). If env-ctl picks an algo `ring` doesn't support, either change the
   algo or use `aws-lc-rs`. *Not yet verified against env-ctl's actual key
   choice.*
2. **24h relay-window clock source (OI-6)** — is `Clock::boottime_ms()` adequate
   and monotonic across suspend for the <=24h USB-gated bearer expiry? *Verify on
   the live Ubuntu 26.04 box.*
3. **Revocation on egress** — does env-ctl's threat model actually require
   OCSP/CRL on the upstream leg, given short bearer lifetimes? If yes, design the
   stapled-OCSP/CRL path now (rustls gives you nothing for free, [#1541](https://github.com/rustls/rustls/issues/1541)).
4. **Host-header smuggling post-decryption** — confirm the HTTP parser in the
   data plane normalizes `Host`/`:authority` before the `decide()` comparison;
   sloppy parsing reopens A9.
5. **rcgen #261 fix PR specifics** — issue is confirmed and addressed in the 0.14
   line, but the exact fixing PR (referenced as #262) was **[UNVERIFIED]** in
   sourcing. Don't rely on a PR number; rely on your own `openssl verify` test.

### Flagged / could-not-verify claims (carried from fact-check, do NOT cite as fact)
- **[UNVERIFIED]** "Cisco IronPort WSA SNI-cache MITM substitution attack" — no
  published CVE/PoC found in open literature. Treat only as a generic
  *cache-keying* cautionary tale, not a real exploit you can cite.
- **[UNVERIFIED]** "62% of middlebox traffic has reduced security" — the
  Cloudflare "Monsters in the Middleboxes"
  ([blog](https://blog.cloudflare.com/monsters-in-the-middleboxes/)) post is about
  MITM *detection*; the specific 62% figure was not located. Don't quote it.
- **[PARTIALLY VERIFIED]** webpki-roots 1.x removed DigiCert G1 (confirmed,
  2026-04-15) but the additional "COMODO / QuoVadis removed" claim was **not**
  independently confirmed — verify against the actual `webpki-roots` changelog
  before asserting.
