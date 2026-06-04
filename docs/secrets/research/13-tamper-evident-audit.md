# env-ctl research — Tamper-evident hash-chained audit logs

> Scope: how env-ctl should build a tamper-evident, hash-chained audit log for a single-operator, local-+server, pure-Rust secrets vault + credential broker on Ubuntu 26.04. Sourced against the live web (assistant cutoff Jan 2026). Claims that could not be independently verified are flagged **[UNVERIFIED]** or **[SPECULATIVE]**.

---

## TL;DR — recommendation for env-ctl

- **Keep the existing linear hash-chain design.** env-ctl already defines `AuditRecord { seq, ts, actor_uid, event_type, subject, detail, outcome, prev_hash, row_hash }` (`/home/drdave/Desktop/env-ctl/crates/secrets-engine/src/event.rs`) and a `Store` trait with `append_audit()` / `verify_audit_chain()` stubs (`/home/drdave/Desktop/env-ctl/crates/secrets-engine/src/vault/store.rs`). This is the right primitive. For a single-operator vault, **O(n) full-chain verification on unlock is acceptable** — a Merkle tree (O(log n) proofs) is not needed at this scale.
- **Canonicalize before hashing.** Serialize each row with a deterministic encoding (RFC 8785 JSON Canonicalization Scheme, or a fixed binary layout) so `row_hash = H(prev_hash ‖ canonical(row))` is reproducible across machines and library versions. See [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785.html).
- **Hash choice: BLAKE3 for the chain is fine; SHA-256 if you want maximal external interoperability.** env-ctl already depends on both (`sha2 = "0.10"`, `blake3 = "1.5"` in `/home/drdave/Desktop/env-ctl/Cargo.toml`). BLAKE3 is faster and tree-structured; SHA-256 is the lingua franca for regulators/auditors and RFC 3161.
- **Detect truncation/deletion with gap-free sequence numbers**, optionally backed by periodic heartbeat entries for time-gap detection.
- **Commit audit rows synchronously and durably before the RPC returns** for security-relevant events — this is already env-ctl's HF-14 invariant (`/home/drdave/Desktop/env-ctl/docs/DESIGN-NOTES.md`). Cosmetic events stay best-effort.
- **Sign the chain head, don't just hash it.** A bare `row_hash` only proves internal consistency; it does not stop an attacker who can rewrite the *whole* log. Add an asymmetric signature over the chain head/Merkle root with **Ed25519** (RFC 8032) so a holder of the public key can verify independently, and/or anchor it externally (RFC 3161 / OpenTimestamps). For Phase 0, internal chaining + optional Ed25519 head-signing is sufficient.
- **Resolve OI-1 first.** The store backend is still open because libSQL bundles C SQLite (`libsql-ffi-0.9.30/bundled/src/sqlite3.c`), which collides with the pure-Rust tenet (`/home/drdave/Desktop/env-ctl/docs/DESIGN-NOTES.md`). Whatever backend wins must enforce append-only semantics (no UPDATE/DELETE on the audit table) at the schema/trigger level. Candidate pure-Rust stores: `redb`, `sled`.

---

## Key facts (with inline source URLs)

### Hash chaining and Merkle trees
- A tamper-evident log links each entry to its predecessor: `row_hash = H(prev_hash ‖ row_data)`. Modifying any historical entry changes its hash and breaks every downstream link, so a single recomputation pass detects tampering. [designgurus.io](https://www.designgurus.io/answers/detail/how-do-you-design-tamperevident-audit-logs-merkle-trees-hashing)
- A **linear chain costs O(n)** to verify (you must replay the whole log); a **Merkle tree gives O(log n)** inclusion/consistency proofs, which matters for large or distributed logs but not for a single-operator vault. [martinuke0.github.io/posts/merkle-tree](https://martinuke0.github.io/posts/merkle-tree/)
- **A root hash (or chain head) alone does not establish trust.** It must be signed, externally anchored, or compared against a trusted reference; otherwise an attacker who controls the store can present a self-consistent forged chain. [transparency.dev/verifiable-data-structures](https://transparency.dev/verifiable-data-structures/)

### Canonicalization
- **RFC 8785 (JSON Canonicalization Scheme, JCS)** is the current standard for deterministic JSON: lexicographic key sort by UTF-16 code-unit order, whitespace removal, and number normalization per ECMAScript. Use it (or an equivalent fixed encoding) so hashes are stable across serializers. [rfc-editor.org/rfc/rfc8785](https://www.rfc-editor.org/rfc/rfc8785.html)

### Hash-function security margins
- For an 80-bit security target, **preimage and second-preimage resistance need ≥80 bits; collision resistance needs ≥160 bits** of output (birthday bound). Both SHA-256 (256-bit) and BLAKE3 (256-bit default) clear this with large margin. [freemanlaw.com/preimage-resistance-...](https://freemanlaw.com/preimage-resistance-second-preimage-resistance-and-collision-resistance/)
- **BLAKE3** splits input into 1024-byte chunks, hashes each independently, and combines them via a binary Merkle tree, making it parallelizable and SIMD-friendly. Its tree structure also enables verified streaming. [github.com/C2SP/C2SP/blob/main/BLAKE3.md](https://github.com/C2SP/C2SP/blob/main/BLAKE3.md)

### Authentication vs. signatures
- **HMAC (e.g., HMAC-SHA256) requires the secret key for *both* signing and verification.** This means a verifier must hold the secret, so HMAC alone cannot give third-party / public verifiability — anyone who can verify can also forge. [en.wikipedia.org/wiki/HMAC](https://en.wikipedia.org/wiki/HMAC)
- **Ed25519 (EdDSA over Curve25519 with SHA-512), per RFC 8032,** is asymmetric: only the private key can sign, but anyone with the public key can verify. This is the right tool when an external auditor must verify the log without the ability to forge it. [datatracker.ietf.org/doc/html/rfc8032](https://datatracker.ietf.org/doc/html/rfc8032)

### Truncation, rollback, and forward integrity
- **Sequence-number gaps are a reliable deletion/truncation indicator**; enforcing gap-free, monotonic sequence numbers exposes removed rows. [docs.keyfactor.com/ejbca/.../integrity-protected-security-audit-log](https://docs.keyfactor.com/ejbca/latest/integrity-protected-security-audit-log)
- **Forward-secure MACs/signatures provide backward integrity:** if the signing key is compromised at time *t*, entries created *before* *t* remain unforgeable; entries after *t* are not protected. Combined with reference-monitor/epoch closure techniques this defends against truncation. *(Precise framing: forward security protects historical entries against future key compromise — it does not make post-compromise entries trustworthy.)* [eprint.iacr.org/2017/949.pdf](https://eprint.iacr.org/2017/949.pdf)

### External timestamping / anchoring
- **RFC 3161 (Time-Stamp Protocol, 2001)** remains the current standard for Time-Stamp Authorities; TSA tokens are widely accepted in regulatory and legal contexts but require a trusted (often paid) third party. [rfc-editor.org/rfc/rfc3161](https://www.rfc-editor.org/rfc/rfc3161.html)
- **OpenTimestamps** offers free, decentralized anchoring to the Bitcoin blockchain (no per-stamp cost, but weaker formal/regulatory acceptance and higher confirmation latency). [opentimestamps.org](https://opentimestamps.org/)

### Regulatory recognition of cryptographic audit trails
- The **2022 amendments to SEC Rule 17a-4** explicitly recognize cryptographic methods — hash chains, digital signatures, and Merkle trees — as an acceptable alternative to traditional WORM (write-once-read-many) storage for audit-trail integrity. This validates a hash-chain-first design for compliance-sensitive deployments. [archive360.com/blog/sec-rule-17a-4-amended-...](https://www.archive360.com/blog/sec-rule-17a-4-amended-taking-the-worm-requirement-out-of-our-misery)

---

## Current versions / APIs (verified against crates.io / source)

| Component | env-ctl pin | Latest stable (web-checked) | Notes |
|---|---|---|---|
| `sha2` | `0.10` (`Cargo.toml:51`) | 0.10.9 (2025-04-30); **0.11.x** released ~Apr 2026 | 0.11 is a breaking release; staying on 0.10.x is fine. [crates.io/crates/sha2](https://crates.io/crates/sha2) |
| `blake3` | `1.5` (`Cargo.toml:52`) | 1.5.x verified; **1.8.5** present in registry (2026) **[date UNVERIFIED]** | Consider bumping to current 1.x. [crates.io/crates/blake3](https://crates.io/crates/blake3) |
| `ed25519-dalek` | not yet pinned | check latest 2.x at sign time | For head/root signing (RFC 8032). |
| `sled` | not pinned | embedded KV store, lock-free append-only log, atomic batches | Pure-Rust backend candidate for OI-1. [docs.rs/sled](https://docs.rs/sled) |
| `redb` | not pinned | pure-Rust embedded store | Alternative OI-1 backend; MVCC, single-file. |
| libSQL (current store) | `libsql-ffi-0.9.30` | bundles C SQLite (`bundled/src/sqlite3.c`) | Violates no-C tenet → OI-1 reopened. |

**BLAKE3 "hazmat" subtree API [UNVERIFIED]:** Some material claims recent `blake3` exposes a hazmat API for computing/combining subtree chaining values without retaining every per-entry hash. This capability is clearly present in the **Bao** verified-streaming project, but I could **not** confirm a stable, advertised `hazmat` module in the core `blake3` crate docs as of this writing. Treat as not-yet-available unless you verify it against the current `blake3` release notes / `docs.rs/blake3` before relying on it.

---

## Security tradeoffs

| Decision | Option A | Option B | env-ctl lean |
|---|---|---|---|
| Chain structure | Linear chain — O(n) verify, trivial to implement | Merkle tree — O(log n) proofs, supports partial/consistency proofs | **Linear** (single operator, modest log size) |
| Hash function | SHA-256 — slower, max interop, RFC 3161-native | BLAKE3 — fast, parallel, tree-native | **BLAKE3** for chain; keep SHA-256 available for external anchoring |
| Integrity proof | `row_hash` only — detects in-place edits | Signed head (Ed25519) and/or external anchor — detects whole-log rewrite | **Hash now; sign head Phase 1+** |
| Verifier model | HMAC — symmetric, verifier can forge | Ed25519 — asymmetric, public verify only | **Ed25519** if any third party must verify; HMAC only if verifier == signer |
| Anchoring | None (internal trust) | RFC 3161 TSA (trusted, paid) / OpenTimestamps (free, slow, weaker acceptance) / blockchain | **None for Phase 0**, RFC 3161 if regulated |
| Commit timing | Async/best-effort — fast, can lose tail | Sync+durable before RPC return | **Sync+durable for security events (HF-14)**, async for cosmetic |
| Key lifecycle | Long-lived signing key | Forward-secure / rotate-on-unlock | **Rotate signing key on USB unlock** for backward integrity |

Key caveats:
- **Internal-only chains protect against *partial* tampering, not a privileged total rewrite.** Anyone who can both edit the audit table and recompute hashes can produce a consistent forgery. The defense is an external/asymmetric trust anchor (signed head, TSA, or off-box copy of the head).
- **HMAC is the wrong choice if you ever want an auditor to verify without trusting them** — they would necessarily hold a forging key. Use Ed25519 for that role.
- **Forward security only buys backward integrity.** After a key compromise, future entries are not trustworthy until the key is rotated; rotate on every USB unlock.

---

## Concrete guidance for the env-ctl implementation

### 1. Finalize the row hash
- Define a single canonical serialization for `AuditRecord` (RFC 8785 JCS over the JSON form, **or** a fixed-width binary layout) and compute `row_hash = H(prev_hash ‖ canonical(seq, ts, actor_uid, event_type, subject, detail, outcome))`.
- Reuse env-ctl's existing fixed-width AAD discipline: HF-2 already specifies a canonical AAD (`domain ‖ table_tag ‖ secret_id ‖ version ‖ dek_generation`) in the crypto layer (`/home/drdave/Desktop/env-ctl/docs/DESIGN-NOTES.md`). Apply the same fixed-field, fixed-width philosophy to the audit canonicalization so the hash input is unambiguous. **[VERIFY: confirm the audit canonicalization byte-layout is documented and matches what `verify_audit_chain()` recomputes.]**
- Genesis row: define `prev_hash` for `seq == 1` explicitly (e.g., all-zero or a domain-separated constant) so verification has a fixed anchor.

### 2. Implement `verify_audit_chain()`
- Walk from `seq == 1` to head: recompute `row_hash`, assert it equals the stored value, assert `prev_hash == previous row's row_hash`, and assert sequence numbers are gap-free and monotonic.
- Run this on unlock (and optionally as a background sweep). For a single-operator vault, full O(n) replay at unlock is acceptable.

### 3. Durability / commit semantics
- Enforce HF-14: for security-relevant events, the audit row must be committed **durably and synchronously in the same transaction** as the operation before the RPC returns. Cosmetic events stay best-effort. (`/home/drdave/Desktop/env-ctl/docs/DESIGN-NOTES.md`)

### 4. Store backend (resolves OI-1)
- Choose a pure-Rust embedded store (`redb` or `sled`) to honor the no-C tenet; libSQL's bundled C SQLite is the blocker.
- Enforce **append-only** on the audit table at the schema/trigger level: reject UPDATE/DELETE. Add a sequence-number CHECK/assertion (DB-level if available, else application-level).
- For compaction/rotation, use an atomic batch guarded by a `rotation_in_progress` flag — the same crash-recovery pattern env-ctl uses for DEK rotation.

### 5. Trust anchor (Phase 1+)
- Add Ed25519 (RFC 8032) signing over the chain head (or a periodic Merkle root). Rotate the signing key on USB unlock for forward security. Publish the public key so the head can be verified off-box.
- If regulatory requirements emerge, anchor the signed head to an RFC 3161 TSA (court-accepted) or OpenTimestamps (free). Defer this decision to the operator.

### 6. Per-bearer forensic traceability (OI-11)
- For the broker/relay path, include a `token_id` in the `RelaySwapped` audit event so each <=24h USB-gated relay bearer is linkable in the chain (`/home/drdave/Desktop/env-ctl/docs/DESIGN-NOTES.md`).

---

## Open questions (unresolved in current design docs / code)

1. **Key rotation on unlock.** How should the audit *signing* key (if Ed25519 head-signing is adopted) interact with USB-partition-UUID unlock? Rotate per unlock, per epoch, or per DEK generation? *No guidance in design docs or code.*
2. **Stateless vs. background verification.** Full chain replay on every unlock, or incremental/background verification with a checkpointed last-good seq? *Not specified.*
3. **Merkle-root batch signing cadence.** If/when a Merkle layer is added, how often is the root signed (per-op, per-24h, per-rotation)? Rate-limit implications? *Not addressed.*
4. **Heartbeat entries vs. sequence gaps.** Should the schema include periodic heartbeat rows to detect *time*-based truncation (not just count gaps)? *Schema not yet finalized (Phase 1 pending).*
5. **External anchoring choice.** RFC 3161 vs. OpenTimestamps vs. none. Design docs mark anchoring "optional" without a decision. *Operator decision pending.*
6. **Archival & compaction strategy.** How are old audit segments archived/compacted while preserving chain continuity and the signed head? *Not addressed in Phase 0 docs.*

---

## Flagged / unverified claims

- **BLAKE3 core-crate `hazmat` subtree API** — [UNVERIFIED] in the `blake3` crate; confirmed concept only in Bao. Verify against current `docs.rs/blake3` before use.
- **`blake3` 1.8.5 exact release date** — [SPECULATIVE]; version appears in the registry but the precise release date was not confirmed.
- **`sha2` 0.11.0 release timing** — [APPROXIMATE]; reported ~Apr 2026, breaking vs. 0.10.x.
- **Rollback/replay audit-defense paper (arXiv 2511.13641)** — [LOW-RELEVANCE / UNVERIFIED] specifics not independently checked.
- **DESIGN-NOTES.md line numbers** (HF-2, HF-14, OI-1, OI-11) — content verified to exist; exact line numbers are fragile and may shift on reorganization.
