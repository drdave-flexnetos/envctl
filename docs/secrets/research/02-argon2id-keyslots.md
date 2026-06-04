# env-ctl research — argon2id params + LUKS-style keyslots

> Scope: KDF parameter selection and the LUKS-style dual-KEK keyslot design for env-ctl's
> at-rest envelope. Verified against live primary sources (RFC 9106, RFC 5869, OWASP, crates.io,
> docs.rs) and the in-repo design (`crates/secrets-engine/src/keyslot.rs`, `docs/ARCHITECTURE.md`,
> `docs/DESIGN-NOTES.md`, `docs/THREAT-MODEL.md`).
> Last verified: **2026-06-02**. Items that could not be verified are flagged **[UNVERIFIED]**.

---

## TL;DR — recommendation for env-ctl

- **Keep the current passphrase params: Argon2id `m=1 GiB (1_048_576 KiB)`, `t=4`, `p=4`.**
  This *exceeds* the RFC 9106 first-recommended profile (`m=2 GiB, t=1, p=4`) on time cost and is
  far above the OWASP minimum, which is appropriate for a single-operator local vault where a
  multi-hundred-ms unlock is acceptable. No change needed.
- **Keep the `ARGON2_M_KIB_FLOOR = 262_144` (256 MiB) downgrade guard** (`keyslot.rs:46`, FS-S13).
  It prevents an attacker who controls the header from substituting weak params. Do not lower it.
- **Construct the hasher explicitly** as `Argon2::new(Algorithm::Argon2id, Version::V0x13, params)`.
  `Argon2::default()` already yields Argon2id/V0x13 today, so this is defense-in-depth against a
  future default change, not a current API requirement. The code already commits to V0x13
  (`keyslot.rs:81`).
- **USB slot stays HKDF-SHA256, not Argon2** — correct, because the keyfile is already 64 bytes of
  CSPRNG output, so a memory-hard KDF buys nothing (DESIGN-NOTES.md HF-3 / line 75).
- **Pin crates to current stable**: `argon2 = "0.5"` (0.5.3), `chacha20poly1305 = "0.10"` (0.10.1),
  `hkdf = "0.13"`. Avoid the 0.6.0-rc / 0.11.0-rc lines until they stabilize (see Versions below).
- **The vault is 1-of-2, not 2FA** by default (CF-3). The `RequireBoth` factor already exists in the
  enum (`keyslot.rs:31`) — finish wiring it for true dual-factor scenarios.

---

## Key facts (with inline source URLs)

### Argon2id KDF parameters

- **RFC 9106 is the normative Argon2 spec** (IRTF CFRG, Sept 2021). It defines Argon2d, Argon2i, and
  the hybrid **Argon2id**, and recommends Argon2id as the default choice.
  <https://datatracker.ietf.org/doc/html/rfc9106>
- **RFC 9106 §4 / §7.4 — two recommended profiles**, both with 128-bit salt and 256-bit tag:
  - **FIRST (primary, "uniformly safe default for all environments"):** `m = 2^21 KiB (2 GiB)`,
    `t = 1`, `p = 4`.
  - **SECOND (memory-constrained):** `m = 2^16 KiB (64 MiB)`, `t = 3`, `p = 4`.
    <https://datatracker.ietf.org/doc/html/rfc9106#section-4>
- **Argon2id is a hybrid**: a first (data-independent, Argon2i-like) pass for side-channel
  resistance, then data-dependent (Argon2d-like) passes for GPU/TMTO resistance — RFC 9106 §3.4 /
  §9. <https://datatracker.ietf.org/doc/html/rfc9106#section-9>
- **OWASP Password Storage Cheat Sheet (current, verified 2026-06-02)** recommends — note these are
  *minimums* for online password verification, not the env-ctl target:
  - Primary: **`m = 19456 KiB (19 MiB)`, `t = 2`, `p = 1`**
  - Equivalent alternatives (CPU/RAM tradeoff, same defense level):
    `m=47104 (46 MiB) t=1 p=1`, `m=12288 (12 MiB) t=3 p=1`, `m=9216 (9 MiB) t=4 p=1`,
    `m=7168 (7 MiB) t=5 p=1`.
  <https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html>
  > **Correction to prior research:** earlier notes cited OWASP "m=64 MiB, t=3, p=1". The live
  > cheat sheet does **not** list that; current primary is **m=19 MiB, t=2, p=1**. env-ctl's
  > `m=1 GiB` sits far above either figure, so the discrepancy does not affect the design.

### Parameter bounds (RustCrypto `argon2`)

- `m_cost`: `8 * p` … `2^32 − 1` KiB · `t_cost`: `1` … `2^32 − 1` · `p_cost`: `1` … `2^24 − 1` ·
  `output_len`: `4` … `2^32 − 1` bytes. <https://docs.rs/argon2/0.5.3/argon2/struct.Params.html>
- `Argon2::default()` returns **Argon2id, Version::V0x13** with default params; the algorithm and
  version are not required to be passed explicitly today.
  <https://docs.rs/argon2/0.5.3/argon2/struct.Argon2.html>

### LUKS-style keyslots (prior art)

- **LUKS2 supports up to 32 independent keyslots**, each independently wrapping the same volume key,
  any of which can unlock — the model env-ctl mirrors for its DEK.
  <https://gitlab.com/cryptsetup/cryptsetup/-/wikis/LUKS-standard/on-disk-format.pdf>
- LUKS2 keyslots use PBKDF (Argon2i/Argon2id or PBKDF2) per keyslot; per-keyslot KDF params are
  stored in the keyslot's JSON area. (Exact per-slot min/max KDF bounds are library/format
  defaults, not separately pinned by env-ctl.) **[UNVERIFIED]** for precise LUKS2 numeric bounds.
  <https://man7.org/linux/man-pages/man8/cryptsetup.8.html>

### HKDF (USB keyfile slot)

- **RFC 5869** defines HKDF as `Extract(salt, IKM) → PRK` then `Expand(PRK, info, L) → OKM`; used in
  IKEv2, PANA, EAP-AKA. <https://www.rfc-editor.org/rfc/rfc5869>
- RustCrypto `hkdf`: `Hkdf::<Sha256>::new(Some(salt), ikm)` does extract+expand;
  `Hkdf::from_prk(prk)` is expand-only. <https://docs.rs/hkdf/latest/hkdf/struct.Hkdf.html>

### AEAD (envelope)

- **XChaCha20-Poly1305**: 256-bit (32-byte) key, **192-bit (24-byte) nonce**, 128-bit (16-byte)
  Poly1305 tag. The 24-byte nonce permits random nonces without a birthday-bound collision concern.
  <https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/struct.XChaCha20Poly1305.html>
- The RustCrypto `chacha20poly1305` crate has had an NCC Group security audit.
  <https://github.com/RustCrypto/AEADs>

---

## Current versions / APIs (verified on crates.io, 2026-06-02)

| Crate | Latest stable | Newest pre-release | env-ctl pin | Notes |
|---|---|---|---|---|
| `argon2` | **0.5.3** (2024-01-20) | 0.6.0-rc.8 (2026-03-22) | `"0.5"` | Stay on 0.5.x until 0.6 ships final. <https://crates.io/crates/argon2> |
| `chacha20poly1305` | **0.10.1** (2022-08-10) | 0.11.0-rc.3 (2026-02-02) | `"0.10"` | Stay on 0.10.x. <https://crates.io/crates/chacha20poly1305> |
| `hkdf` | **0.13.0** (2026-03-30) | — | `"0.13"` recommended | Was 0.12 in prior notes; 0.13.0 is now stable, MSRV 1.85. <https://crates.io/crates/hkdf> |

> **Version correction:** prior research listed `chacha20poly1305 0.11.0-rc.2 (Nov 2025)` as the
> newest pre-release; the live newest is **0.11.0-rc.3 (Feb 2026)**, and these RC lines remain
> pre-1.0 unstable — do not adopt for a vault. `hkdf` has advanced to a **0.13.0 stable** since the
> earlier "0.12" note; prefer 0.13.

Representative current API (RustCrypto):

```rust
use argon2::{Algorithm, Argon2, Params, Version};
let params = Params::new(m_kib, t_cost, p_lanes, Some(32)).unwrap();
let a2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params); // explicit = defense-in-depth
a2.hash_password_into(passphrase, salt, &mut kek_bytes)?;

use hkdf::Hkdf; use sha2::Sha256;
let hk = Hkdf::<Sha256>::new(Some(salt), keyfile);  // extract
hk.expand(b"env-ctl/v1/kek/usb", &mut kek_bytes)?;  // expand
```

---

## Security tradeoffs

- **Memory cost (`m`) is the strongest lever** against GPU/ASIC cracking; raising `m` raises an
  attacker's per-guess silicon cost more than raising `t`. env-ctl's `m=1 GiB` is deliberately high.
- **`t` vs `m`**: RFC 9106's primary profile trades almost all cost into memory (`t=1, m=2 GiB`).
  env-ctl chooses `t=4, m=1 GiB` — more passes over less memory. Both are defensible; the high `t`
  hedges if a future deployment must drop `m` on a constrained host (the 256 MiB floor still holds).
- **Parallelism (`p`) does not add cracking resistance per se** — it scales wall-clock down on the
  defender's many cores while an attacker parallelizes anyway. `p=4` matches RFC 9106.
- **1-of-2 keyslots = OR semantics (CF-3).** Vault strength is `max(weakness of USB factor, weakness
  of passphrase factor)` — i.e. the *weaker* factor sets the floor. A weak passphrase is an
  attacker-forceable downgrade path even with a strong USB keyfile. The honest framing in
  DESIGN-NOTES CF-3 (line 33) is correct: this is *availability* via either factor, not 2FA.
- **`RequireBoth` (true 2FA)** flips to AND semantics: an attacker needs both the partition and the
  passphrase. Tradeoff: lose the "unlock with either" convenience and add a single-point-of-failure
  if the USB is lost. Already modeled as a `Factor` variant (`keyslot.rs:31`).
- **HKDF for the USB slot is correct, not a weakness.** The keyfile is 64 bytes of CSPRNG entropy
  (DESIGN-NOTES.md line 75); Argon2 adds cost only against *low-entropy* inputs, so HKDF is the
  right (fast, deterministic) KDF here. Using Argon2 on auto-unlock would be wrong.
- **AEAD tag as the correctness oracle.** `unwrap_dek` returns `None` on Poly1305 tag failure
  (`keyslot.rs:74`) — there is no separate verifier to leak timing or enable a confirmation oracle.
- **Header MAC over the keyslot set** (`header_mac`, `keyslot.rs:88`, OI-8) detects tampering /
  keyslot addition-removal and rollback below the issuance floor; recomputed on unlock, refuse on
  drift.
- **DEK rotation is O(all-secrets)**; passphrase/keyslot rotation is O(1) keyslot rewrite
  (DESIGN-NOTES HF-1, line 42). Rotate the cheap thing often; rotate the DEK only on suspected
  compromise.

---

## Concrete guidance for the env-ctl implementation

1. **Passphrase KEK** (`kek_from_passphrase`, `keyslot.rs:84`): use
   `Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(m_kib, t_cost, p_lanes, Some(32)))`
   with the slot's stored `Argon2Params`. Default `{m:1_048_576, t:4, p:4}` (`keyslot.rs:38-44`).
   **Enforce `m_kib >= ARGON2_M_KIB_FLOOR` before unwrap** and hard-fail otherwise (FS-S13).
2. **USB KEK** (`kek_from_usb`, `keyslot.rs:81`): `Hkdf::<Sha256>::new(Some(salt32), keyfile)` then
   `expand(b"env-ctl/v1/kek/usb", &mut out32)`. Assert OKM length == 32. The domain-separated `info`
   string prevents cross-slot key confusion — keep it versioned (`/v1/`).
3. **`keyslot_aad`** (`keyslot.rs:65`, HF-3): fixed-width canonical encoding binding
   `factor · kdf-id · m · t · p · salt · usb_partition_uuid · dek_generation · slot-id`. Use
   fixed-width big-endian integers and length-prefixed byte fields so no two distinct slot states
   serialize to the same AAD. Cover with a round-trip unit test (HF-3 calls for this).
4. **Wrap/unwrap** (`wrap_dek`/`unwrap_dek`): XChaCha20-Poly1305 with a fresh 24-byte CSPRNG nonce
   per wrap; store `(nonce24, ct||tag)`. Pass `keyslot_aad(slot)` as associated data so the envelope
   is bound to its slot metadata. Zeroize the KEK on consume (OI-7) — `Kek` is `ZeroizeOnDrop`.
5. **Scratch zeroization (OI-7):** Argon2's memory arena and HKDF intermediates must be wiped.
   RustCrypto `argon2` zeroizes its internal blocks; still zeroize any caller-side buffers and the
   derived `kek_bytes` (wrap them in `Zeroizing`).
6. **Header MAC (OI-8):** compute `header_mac(dek, slots, issuance_floor_ms)` after any keyslot
   mutation; verify on every unlock; refuse on mismatch or floor regression.
7. **USB identity (OI-5):** pin the **GPT PARTUUID** (`Keyslot.usb_partition_uuid`,
   `keyslot.rs:58`), obtained via `blkid -o value -s PARTUUID`, not the filesystem UUID or device
   path.
8. **Finish `RequireBoth` (CF-3):** derive a combined KEK from both factors (e.g.
   `KEK = HKDF-SHA256(info, Argon2id(pp) || HKDF(usb))`) for the opt-in true-2FA slot. Concatenate
   inside a single KDF rather than XOR-ing two KEKs, to avoid related-key footguns.

---

## Open questions

- **OWASP vs RFC posture.** OWASP's *low* numbers target sub-second online verification; env-ctl's
  `m=1 GiB` targets an offline-crack-resistant local vault. Confirm the chosen unlock latency on the
  dual-RTX-5090 box is acceptable (likely well under 1 s) and document the rationale so the high `m`
  isn't "corrected" downward later. **[OPEN]**
- **`argon2 0.6` / `chacha20poly1305 0.11` migration.** Both are in RC as of 2026-06; track for API
  changes (e.g. `Params`/`Version` surface) before bumping. **[OPEN]**
- **Per-hardware tuning.** Earlier research suggested `p=1–2, m=4–8 GiB` for 256 GB+ RAM hosts to
  hit a target latency. This is plausible first-principles advice but **[UNVERIFIED]** against any
  normative source; env-ctl currently uses a single fixed profile and does not need per-host tuning.
- **LUKS2 numeric KDF bounds.** The exact min/max Argon2 params enforced inside the LUKS2 *format*
  (vs. the underlying library) were not verified from a primary spec page. **[UNVERIFIED]** — does
  not affect env-ctl, which defines its own floor.
- **`RequireBoth` KDF composition.** The combined-KEK construction above is a recommendation, not
  yet ratified in DESIGN-NOTES; confirm the exact composition + AAD binding before implementation.
  **[OPEN]**

---

### Sources

- RFC 9106 (Argon2): <https://datatracker.ietf.org/doc/html/rfc9106>
- RFC 5869 (HKDF): <https://www.rfc-editor.org/rfc/rfc5869>
- OWASP Password Storage Cheat Sheet: <https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html>
- `argon2` crate: <https://docs.rs/argon2/0.5.3/argon2/> · <https://crates.io/crates/argon2>
- `chacha20poly1305` crate: <https://docs.rs/chacha20poly1305/0.10.1/> · <https://crates.io/crates/chacha20poly1305>
- `hkdf` crate: <https://docs.rs/hkdf/latest/hkdf/> · <https://crates.io/crates/hkdf>
- RustCrypto AEADs (audit): <https://github.com/RustCrypto/AEADs>
- LUKS2 on-disk format: <https://gitlab.com/cryptsetup/cryptsetup/-/wikis/LUKS-standard/on-disk-format.pdf>
- cryptsetup(8): <https://man7.org/linux/man-pages/man8/cryptsetup.8.html>
- In-repo: `crates/secrets-engine/src/keyslot.rs`, `docs/ARCHITECTURE.md`, `docs/DESIGN-NOTES.md` (CF-3, HF-1, HF-3, OI-5/7/8), `docs/THREAT-MODEL.md` (FS-S13)
