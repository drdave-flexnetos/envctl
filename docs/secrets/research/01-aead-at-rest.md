# env-ctl research — XChaCha20-Poly1305 at-rest envelope in Rust

> Scope: at-rest, app-layer record encryption for env-ctl's secrets vault (libSQL/redb-pending store, per-record envelope under a DEK). Verified against live sources as of **June 2026**. Assistant knowledge cutoff was Jan 2026; every version/API claim below was re-checked on the web and dated.

---

## TL;DR — recommendation for env-ctl

**Keep XChaCha20-Poly1305 as the per-record at-rest AEAD. It is the correct choice.** It is pure-Rust, RFC-aligned, audited, constant-time, hardware-independent, and its 192-bit nonce makes per-record random nonces safe without a counter — which is exactly what a multi-process, crash-prone local daemon wants.

Concrete asks for the implementation:

1. **Pin `chacha20poly1305 = "0.10"`** (current stable is **0.10.1**). Do **not** adopt `0.11.0-rc.x` in production yet — it is a release candidate and raises MSRV to 1.85. See [versions](#current-versions--apis).
2. **Mandate `OsRng` for nonce generation** in the seal path (env-ctl OI-16). Use `XChaCha20Poly1305::generate_nonce(&mut OsRng)`; forbid seeded RNG outside `#[cfg(test)]`.
3. **Bind canonical, fixed-width AAD** per record (env-ctl HF-2): `domain || u8(table_tag) || u64be(secret_id) || u64be(version) || u64be(dek_generation)`. AAD is authenticated, not encrypted — never put plaintext secret material in it.
4. **Rotate the DEK well before the per-key message ceiling.** COSE caps a single ChaCha20-Poly1305 key at 2^64 messages; pick a conservative rolling rotation window (env-ctl HF-1 atomic re-encrypt) — a 2^48–2^60 budget is comfortable, and 2^96 is the nonce-collision wall, not your operating point.
5. **Hold key material as `Zeroizing<[u8;32]>`** (env-ctl R6) and keep secrets in `secrecy` wrappers in transit through RAM.
6. **Decide explicitly whether you need a *committing* AEAD.** XChaCha20-Poly1305 is **not** key-/message-committing. If env-ctl's threat model needs "this ciphertext provably decrypts to exactly one plaintext under one key" (multi-recipient / non-repudiation / partitioning-oracle resistance), bolt on a commitment or switch that record class to a committing scheme. For a single-DEK local vault this is almost certainly **not** required.

You do **not** need a nonce-misuse-resistant AEAD (AES-GCM-SIV) given random 192-bit nonces from `OsRng`. Keep SIV in your back pocket only if you ever cannot guarantee CSPRNG nonces.

---

## Key facts (with inline source URLs)

- **Construction.** XChaCha20-Poly1305 is ChaCha20-Poly1305 with an extended **192-bit (24-byte) nonce**. It derives a per-message subkey via **HChaCha20** from the key + first 16 bytes of the nonce, then runs standard ChaCha20-Poly1305 (RFC 8439) with the remaining 8 nonce bytes. Specified in the CFRG draft. ([datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03), [doc.libsodium.org — xchacha20-poly1305 construction](https://doc.libsodium.org/secret-key_cryptography/aead/chacha20-poly1305/xchacha20-poly1305_construction))

- **Why the long nonce matters for env-ctl.** A 192-bit nonce makes **random** nonce selection safe: the birthday bound puts a ~50% collision probability only after ~**2^96** messages under one key. Standard 96-bit-nonce ChaCha20-Poly1305 cannot do this safely. ([draft-irtf-cfrg-xchacha-03 §3.1, "Security of XChaCha20-Poly1305"](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03))

- **RFC 8439 nonce rule (the base scheme).** For plain ChaCha20-Poly1305, the 96-bit nonce **MUST NOT be reused** with the same key, and the RFC explicitly warns against random generation at that size — counter/deterministic nonces are the prescribed pattern. XChaCha20-Poly1305 lifts this constraint precisely because the nonce is large enough for random selection. ([rfc-editor.org/rfc/rfc8439 §4 "Security Considerations"](https://www.rfc-editor.org/rfc/rfc8439.html#section-4))

- **Per-key message ceiling.** COSE (RFC 9053) guidance: do not exceed **2^64** messages encrypted under a single ChaCha20-Poly1305 key; refresh/rotate before that. This is the operative rotation driver, distinct from the 2^96 nonce wall. ([datatracker.ietf.org/doc/rfc9053](https://datatracker.ietf.org/doc/rfc9053/))

- **Audit.** The RustCrypto AES-GCM and ChaCha20+Poly1305 implementations were reviewed by **NCC Group** (engagement sponsored by MobileCoin, **December 2019**, ~5 person-days). The public report found the implementations sound and constant-time, with no reported vulnerabilities in the AEAD constructions. ([nccgroup.com — public report: RustCrypto AES-GCM and ChaCha20+Poly1305 implementation review](https://www.nccgroup.com/us/research-blog/public-report-rustcrypto-aes-gcm-and-chacha20poly1305-implementation-review/))

- **Pure Rust, no C/FFI.** `chacha20poly1305` is implemented in Rust over the RustCrypto `chacha20`, `poly1305`, and `aead` crates — no C, no OpenSSL/aws-lc. This satisfies env-ctl's no-C tenet for the *crypto* layer (note: the *store* backend C-tension is separate — env-ctl OI-1). ([github.com/RustCrypto/AEADs/tree/master/chacha20poly1305](https://github.com/RustCrypto/AEADs/tree/master/chacha20poly1305))

- **AAD semantics.** Associated data is **authenticated but not encrypted**. It is the right place to bind record identity (table, id, version, dek_generation) so a ciphertext cannot be replayed into a different row. ([rfc-editor.org/rfc/rfc8439 §2.8 "AEAD Construction"](https://www.rfc-editor.org/rfc/rfc8439.html#section-2.8))

- **No commitment.** ChaCha20-Poly1305 (and AES-GCM) are **not** key- or message-committing: an attacker who controls keys can craft one ciphertext that decrypts validly under two different keys. This enables partitioning-oracle attacks in multi-key settings. Modern committing-AEAD constructions exist (e.g., CTX, generic transforms) but add overhead. ([eprint.iacr.org/2024/1813 (committing AEAD survey/analysis)](https://eprint.iacr.org/2024/1813.pdf), [usenix.org Sec'21 "Partitioning Oracle Attacks" — Len, Grubbs, Ristenpart](https://www.usenix.org/conference/usenixsecurity21/presentation/len))

- **Performance (single-platform data point, treat as indicative not canonical).** A 2025 benchmark reports XChaCha20-Poly1305 at ~**4.2 GB/s** in pure software on an Apple M3 Pro; software-only AES-GCM ~1.8 GB/s, AES-NI-accelerated AES-GCM ~6.4 GB/s. Throughput is platform-, compiler-, and message-size-dependent; for a local daemon doing per-record secrets, crypto is not the bottleneck. ([blog.vitalvas.com/post/2025/06/01/xchacha20-poly1305-vs-aes](https://blog.vitalvas.com/post/2025/06/01/xchacha20-poly1305-vs-aes/))

- **Nonce-misuse-resistant alternative.** AES-GCM-SIV (RFC 8452) tolerates nonce reuse (reuse leaks only equality of plaintexts, not the key). Cost: encryption runs at roughly **~70% of AES-GCM throughput** (~30% slower) in the measured x86-64 software case; decryption is comparable. It is **not** needed if you guarantee CSPRNG nonces. ([rfc-editor.org/rfc/rfc8452](https://www.rfc-editor.org/rfc/rfc8452.html), [imperialviolet.org/2017/05/14/aesgcmsiv.html (Langley)](https://www.imperialviolet.org/2017/05/14/aesgcmsiv.html))

---

## Current versions / APIs

| Crate | Latest stable | Pre-release | env-ctl pin | Notes |
|---|---|---|---|---|
| `chacha20poly1305` | **0.10.1** (2022-08-10) | **0.11.0-rc.3** (2026-02-02) | `"0.10"` ✅ | RC raises MSRV to **1.85** / edition 2024; stay on 0.10 for prod. ([crates.io/crates/chacha20poly1305](https://crates.io/crates/chacha20poly1305)) |
| `secrecy` | **0.10.3** | — | (use in RAM path) | `SecretBox`/`SecretString`/`SecretVec` aliases over `Secret<T>`; zeroize-on-drop. ([crates.io/crates/secrecy](https://crates.io/crates/secrecy), [docs.rs/secrecy](https://docs.rs/secrecy)) |
| `zeroize` | (current) | — | via `Zeroizing<>` | Backs env-ctl R6 `Dek/Kek(Zeroizing<[u8;32]>)`. ([docs.rs/zeroize](https://docs.rs/zeroize)) |
| `subtle` | (current) | — | bearer MAC compare | Constant-time `ct_eq` for env-ctl R5. ([docs.rs/subtle](https://docs.rs/subtle)) |

> **Verification note:** I confirmed `chacha20poly1305` advanced past my Jan-2026 cutoff to `0.11.0-rc.3` (published 2026-02-02). **Stable is still 0.10.1.** The 0.11 line is RC-only and bumps MSRV to 1.85 — incompatible with env-ctl's stated MSRV floor (DESIGN-NOTES references a 1.80 CI gate, OI re: MSRV). Do not bump until 0.11 ships stable *and* env-ctl's MSRV is raised deliberately. (Source: [crates.io/api/v1/crates/chacha20poly1305](https://crates.io/crates/chacha20poly1305))

### 0.10.1 API surface you will use

Types: `XChaCha20Poly1305`, `Key` (32 B), `XNonce` (24 B). Traits: `KeyInit`, `Aead`, `AeadInPlace`, `AeadCore`. Nonce generation via `generate_nonce(&mut OsRng)` requires the crate's `rand_core` feature. ([docs.rs/chacha20poly1305/0.10.1](https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/))

```rust
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
    XChaCha20Poly1305, XNonce, Key,
};

// dek: &Zeroizing<[u8; 32]>  (env-ctl R6)
let cipher = XChaCha20Poly1305::new(Key::from_slice(&dek[..]));

// MANDATE OsRng (env-ctl OI-16) — 24-byte random nonce is safe at this size.
let nonce: XNonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);

// Canonical, fixed-width AAD (env-ctl HF-2). NOT encrypted — identity binding only.
let aad: Vec<u8> = canonical_aad(domain, table_tag, secret_id, version, dek_generation);

let ct = cipher
    .encrypt(&nonce, Payload { msg: plaintext, aad: &aad })
    .map_err(|_| SealError)?;
// Persist: (nonce, ct, dek_generation). Tag is appended inside `ct`.
```

For zero-copy / tighter RAM hygiene use `AeadInPlace::encrypt_in_place_detached` (returns the 16-byte tag separately) over a buffer you can `Zeroizing`-wrap. ([docs.rs/chacha20poly1305/0.10.1 — `AeadInPlace`](https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/trait.AeadInPlace.html))

---

## Security tradeoffs

| Property | XChaCha20-Poly1305 (recommended) | AES-256-GCM | AES-256-GCM-SIV |
|---|---|---|---|
| Pure-Rust, no AES-NI dependency | ✅ constant-time everywhere | ⚠️ slow/risky without AES-NI | ⚠️ same caveat |
| Random per-record nonce safe? | ✅ 24-byte nonce (2^96 wall) | ❌ 12-byte; needs counter | ✅ misuse-resistant |
| Nonce **reuse** consequence | catastrophic (key/auth break) | catastrophic | graceful (leaks plaintext equality only) |
| Per-key message budget | refresh ≪ 2^64 (COSE) | ~2^32 records (NIST GCM) | larger, but rotate anyway |
| Throughput, software-only | high (~4 GB/s class) | low w/o AES-NI | ~30% slower than GCM |
| Committing? | ❌ no | ❌ no | ❌ no (SIV ≠ committing) |
| Audited RustCrypto impl | ✅ NCC 2019 | ✅ NCC 2019 | partial |

Key tradeoff judgments for env-ctl:

- **Random-nonce safety wins over SIV.** A daemon that can crash/restart mid-batch could in theory re-issue a counter; the 192-bit random nonce sidesteps counter-state durability entirely. This is the single strongest reason to prefer XChaCha20 here over AES-GCM. ([draft-irtf-cfrg-xchacha-03](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03))
- **No AES-NI dependency** means uniform, side-channel-resistant performance across whatever CPU the vault runs on (constant-time software ChaCha is the norm; constant-time software AES is hard). ([github.com/RustCrypto/AEADs](https://github.com/RustCrypto/AEADs))
- **Non-committing is acceptable for a single-DEK vault** but becomes relevant the moment multiple keys can validly decrypt the same blob (key-rotation overlap windows, multi-recipient, external attacker supplying keys). Flag it; don't silently assume it away. ([usenix Sec'21 partitioning oracle](https://www.usenix.org/conference/usenixsecurity21/presentation/len))
- **Tag truncation / 16-byte tag.** Keep the full 128-bit Poly1305 tag; do not truncate.

---

## Concrete guidance for the env-ctl implementation

1. **Dependency hygiene.** `chacha20poly1305 = { version = "0.10", default-features = false, features = ["alloc", "rand_core"] }`. Confirm the CI no-C / single-rustls gate still passes; this crate adds no C. ([crates.io/crates/chacha20poly1305](https://crates.io/crates/chacha20poly1305))

2. **Nonce policy (OI-16).** Centralize nonce minting in one function that takes `&mut OsRng`. Add a `#[cfg(not(test))] compile_error!`-style guard or a feature gate so no seeded RNG can reach the seal path. Add a debug-assert that `(dek_generation, nonce)` is unique within a rotation batch (already in OI-16). Never persist or reuse a nonce.

3. **Canonical AAD (HF-2).** Encode exactly `domain || u8(table_tag) || u64be(secret_id) || u64be(version) || u64be(dek_generation)` with **fixed widths** (no length-prefixed/var-int ambiguity — that ambiguity is the CF-class bug HF-2 fixes). Unit-test the byte layout with golden vectors. Decryption must reconstruct the *same* AAD from the row it is loaded from, so a ciphertext copied to another row fails authentication.

4. **Record layout.** Store `(dek_generation, nonce[24], ciphertext_with_tag)`. `dek_generation` selects the unwrapping key and is also bound into AAD — both, deliberately.

5. **DEK rotation (HF-1).** Treat COSE's 2^64 as a hard ceiling and operate far below it. Implement rotation as the atomic, resumable, O(all-secrets) re-encrypt HF-1 already specifies; bump `dek_generation`; re-wrap (not re-derive) the `hmac_key` row so live bearers survive rotation (env-ctl OI-9 / R5). ([datatracker.ietf.org/doc/rfc9053](https://datatracker.ietf.org/doc/rfc9053/))

6. **Key material in RAM (R6 / OI-7).** `Dek/Kek(Zeroizing<[u8;32]>)`; consume `Kek` by value in `unwrap_dek` so it dies promptly; pass keyfile/passphrase as `&Zeroizing`; use `secrecy::SecretBox`/`SecretVec` for variable-length plaintext bodies crossing the engine boundary; ensure no `Serialize` on key types. Watch for heap reallocation during unwrap (any `Vec` growth re-copies pre-zeroization). ([docs.rs/secrecy](https://docs.rs/secrecy), [docs.rs/zeroize](https://docs.rs/zeroize))

7. **In-place path for large bodies.** Prefer `encrypt_in_place_detached` into a `Zeroizing`-wrapped buffer for real secret bodies to minimize plaintext copies. ([docs.rs/chacha20poly1305/0.10.1](https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/trait.AeadInPlace.html))

8. **Test vectors.** Add a known-answer test from the CFRG draft's XChaCha20-Poly1305 vectors to lock the wiring (key/nonce/AAD/ct/tag). ([draft-irtf-cfrg-xchacha-03 — appendix test vectors](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03))

9. **Do not conflate the crypto with the store.** This AEAD layer is store-agnostic — it operates on `(plaintext, aad) → blob`. Keep it behind the `Store` trait so it works identically whether OI-1 resolves to redb (pure-Rust) or a waived libSQL. ([env-ctl DESIGN-NOTES.md R9 / OI-1] — local design doc)

---

## Open questions (flagged / could-not-fully-verify)

- **OI-1 (BLOCKING, env-ctl-internal).** The *store* backend C-dependency (libSQL bundles C SQLite, VERIFIED in `libsql-ffi`) is unresolved. The AEAD layer is unaffected, but "pure-Rust at-rest" is only fully true once OI-1 lands on redb (or the no-C tenet is explicitly waived). Resolve before production. *(Local design doc.)*

- **OI-16 code compliance — UNVERIFIED.** I confirmed OsRng is the correct/required API and that DESIGN-NOTES *mandates* it, but I did **not** inspect the actual `secrets-engine` source to confirm the seal path uses `&mut OsRng` and forbids seeded RNG. Verify in code + add the static guard.

- **Committing-AEAD decision — UNRESOLVED.** Whether env-ctl needs a committing AEAD depends on its key-multiplicity threat model (does any flow allow >1 valid key for one ciphertext?). Not answerable from the docs read. If yes, evaluate a generic committing transform / CTX-style construction; budget the overhead. ([eprint.iacr.org/2024/1813](https://eprint.iacr.org/2024/1813.pdf))

- **0.11 migration timing.** `0.11.0-rc.3` exists (2026-02) but is RC-only and needs Rust 1.85. Track for the eventual stable; do not adopt until env-ctl raises MSRV deliberately. Re-check at next dependency review. ([crates.io/crates/chacha20poly1305](https://crates.io/crates/chacha20poly1305))

- **Performance number is a single data point.** The ~4.2 GB/s M3-Pro figure is from one blog benchmark on one device/message size — directionally useful, not authoritative. If throughput ever matters, benchmark on the actual Ubuntu 26.04 / RTX-5090 target. ([blog.vitalvas.com](https://blog.vitalvas.com/post/2025/06/01/xchacha20-poly1305-vs-aes/))

- **CFRG draft is still a draft.** XChaCha20-Poly1305 is specified by `draft-irtf-cfrg-xchacha` (currently -03), not a finalized RFC, though it is the de-facto standard and matches libsodium's long-deployed `crypto_aead_xchacha20poly1305`. Cite the draft + libsodium, and treat libsodium interop as the stability anchor. ([datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03), [doc.libsodium.org](https://doc.libsodium.org/secret-key_cryptography/aead/chacha20-poly1305/xchacha20-poly1305_construction))
