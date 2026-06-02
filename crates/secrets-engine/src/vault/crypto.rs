//! Per-record AEAD seal/open (XChaCha20-Poly1305, 24-byte random nonce, AAD-bound). Pure-Rust;
//! lands in Phase 1.
//!
//! Construction & rationale are grounded in `docs/research/01-aead-at-rest.md`:
//!   - XChaCha20-Poly1305 is ChaCha20-Poly1305 with an extended 192-bit (24-byte) nonce
//!     (CFRG `draft-irtf-cfrg-xchacha-03`). The long nonce makes per-record *random* nonces safe
//!     (the birthday wall is ~2^96 messages under one key), which suits a crash-prone local
//!     daemon that must not depend on durable counter state.
//!   - Nonce policy (env-ctl OI-16): every nonce is freshly minted from the OS CSPRNG via
//!     `XChaCha20Poly1305::generate_nonce(&mut OsRng)`. Seeded RNG is permitted only under
//!     `#[cfg(test)]` (the KAT vector). The seal path here never reuses or seeds a nonce.
//!   - AAD is *authenticated, not encrypted* (RFC 8439 §2.8): it binds record identity so a
//!     ciphertext cannot be replayed into another row. Callers pass canonical fixed-width AAD.
//!   - Full 128-bit Poly1305 tag, never truncated. Tag verification is the sole correctness
//!     oracle on `open` — a bad key, tampered ciphertext, or wrong AAD all surface as `None`.
//!   - RAM hygiene (R6 / OI-7, research doc point 6): recovered plaintext is returned inside a
//!     `Zeroizing<Vec<u8>>` so the heap copy of the decrypted secret body is wiped when the caller
//!     drops it, rather than lingering in freed heap. This mirrors `keyslot::unwrap_dek`, which
//!     explicitly zeroizes its recovered plaintext.
//!
//! Dependency note: the seal path uses `chacha20poly1305::aead::OsRng`, which is re-exported ONLY
//! under the crate's `getrandom` feature, and `generate_nonce`, which needs the `rand_core`
//! feature. The workspace therefore pins `features = ["alloc", "getrandom", "rand_core"]`
//! (`getrandom` already implies `rand_core`, so `["alloc", "getrandom"]` is the minimal correct
//! set). Reducing this to `["alloc", "rand_core"]` would drop the `OsRng` re-export and break the
//! build — do not do so.
use crate::keyslot::Dek;
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
    Key, XChaCha20Poly1305, XNonce,
};
use zeroize::Zeroizing;

/// XChaCha20-Poly1305 nonce width, in bytes (192-bit extended nonce).
const NONCE_LEN: usize = 24;

/// Build the cipher from the raw DEK bytes. The `Key` is a thin `[u8; 32]` view (no copy of key
/// material beyond what `KeyInit` requires internally); the borrow keeps the DEK owned by `Dek`,
/// which zeroizes on drop.
fn cipher_for(dek: &Dek) -> XChaCha20Poly1305 {
    // `Dek.0` is exactly 32 bytes, matching the XChaCha20-Poly1305 key size; `from_slice` cannot
    // panic here.
    XChaCha20Poly1305::new(Key::from_slice(&dek.0))
}

/// Seal plaintext under the DEK with the given canonical AAD. Returns `(nonce24, ct||tag)`.
///
/// The 24-byte nonce is drawn fresh from `OsRng` on every call (env-ctl OI-16) — never seeded,
/// never reused. `aad` is authenticated but not encrypted. The returned ciphertext has the 16-byte
/// Poly1305 tag appended (the `aead::Aead::encrypt` convention).
pub fn seal(dek: &Dek, aad: &[u8], plaintext: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let cipher = cipher_for(dek);
    // MANDATE OsRng (OI-16): a fresh 24-byte CSPRNG nonce per seal.
    let nonce: XNonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ct_tag = cipher
        .encrypt(&nonce, Payload { msg: plaintext, aad })
        // Encryption of an in-memory buffer under a valid 32-byte key cannot fail for this AEAD;
        // a failure here would be an unrecoverable internal invariant violation.
        .expect("XChaCha20-Poly1305 seal must not fail for an in-memory plaintext");
    (nonce.to_vec(), ct_tag)
}

/// Open a sealed record. `None` on tag failure (tamper / wrong key / wrong AAD) or a malformed
/// nonce length.
///
/// `decrypt` verifies the Poly1305 tag over `ct_tag` bound to `nonce` and `aad` before returning
/// any plaintext; any mismatch yields `None` (constant-time tag comparison is handled inside the
/// audited RustCrypto `poly1305`/`aead` layer).
///
/// The recovered plaintext is wrapped in `Zeroizing<Vec<u8>>` (R6 / OI-7): the caller owns it and
/// the heap buffer is wiped on drop, so decrypted secret material does not linger in freed heap.
/// Record bodies are variable-length, so — unlike `keyslot::unwrap_dek`'s fixed 32-byte DEK — no
/// post-decrypt length validation is performed here; the AEAD tag is the sole correctness oracle.
pub fn open(dek: &Dek, aad: &[u8], nonce: &[u8], ct_tag: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    // Reject a wrong nonce width up front: `XNonce::from_slice` would otherwise panic on a
    // mis-sized slice, and a malformed record must be a clean `None`, not a crash.
    if nonce.len() != NONCE_LEN {
        return None;
    }
    let cipher = cipher_for(dek);
    let nonce = XNonce::from_slice(nonce);
    cipher
        .decrypt(nonce, Payload { msg: ct_tag, aad })
        .ok()
        .map(Zeroizing::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a compact hex string (no separators) into bytes. Test-only helper.
    fn hex(s: &str) -> Vec<u8> {
        assert!(s.len() % 2 == 0, "odd-length hex");
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    /// Known-answer test from CFRG `draft-irtf-cfrg-xchacha-03`, Appendix A.3.1
    /// ("Example and Test Vector for AEAD_XCHACHA20_POLY1305"). This pins the full wiring
    /// (key / 24-byte nonce / AAD / ciphertext / tag) so any regression in the construction or
    /// in our seal/open framing is caught.
    ///
    /// The vector matches libsodium's long-deployed `crypto_aead_xchacha20poly1305`, our stability
    /// anchor per the research doc.
    #[test]
    fn kat_cfrg_draft_xchacha20poly1305() {
        // 114-byte plaintext from the draft vector. Built from canonical ASCII so the bytes are
        // unambiguous: "Ladies and Gentlemen of the class of '99: If I could offer you only one
        // tip for the future, sunscreen would be it."
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only \
one tip for the future, sunscreen would be it."
            .to_vec();

        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let key = hex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let nonce = hex("404142434445464748494a4b4c4d4e4f5051525354555657");

        // Expected ciphertext (with appended 16-byte tag) from the draft vector.
        let expected_ct = hex(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb731c7f1b0b4aa6440b\
             f3a82f4eda7e39ae64c6708c54c216cb96b72e1213b4522f8c9ba40db5d945b11b69b982c1bb9e3f3fa\
             c2bc369488f76b2383565d3fff921f9664c97637da9768812f615c68b13b52e",
        );
        let expected_tag = hex("c0875924c1c7987947deafd8780acf49");

        let dek = Dek(key.as_slice().try_into().expect("32-byte key"));
        let cipher = cipher_for(&dek);
        let xnonce = XNonce::from_slice(&nonce);

        // Encrypt and compare against the pinned ct||tag.
        let got = cipher
            .encrypt(xnonce, Payload { msg: &plaintext, aad: &aad })
            .expect("encrypt");
        let mut expected = expected_ct.clone();
        expected.extend_from_slice(&expected_tag);
        assert_eq!(got, expected, "ciphertext+tag must match CFRG draft vector");

        // And our `open` must recover the plaintext from that same (nonce, ct||tag, aad).
        // `open` returns a `Zeroizing<Vec<u8>>`; compare via the slice deref.
        let recovered = open(&dek, &aad, &nonce, &got).expect("open must succeed on the KAT");
        assert_eq!(recovered.as_slice(), plaintext.as_slice());
    }

    /// Round-trip + tamper/AAD detection on the live seal/open path (random OsRng nonce).
    #[test]
    fn roundtrip_and_tamper_detection() {
        let dek = Dek([0x42; 32]);
        let aad = b"env-ctl/v1/record-identity";
        let plaintext = b"super secret value: hunter2";

        // Honest round-trip.
        let (nonce, ct_tag) = seal(&dek, aad, plaintext);
        assert_eq!(nonce.len(), NONCE_LEN, "nonce must be 24 bytes");
        let opened = open(&dek, aad, &nonce, &ct_tag).expect("honest open must succeed");
        // `opened` is a `Zeroizing<Vec<u8>>`; compare its bytes against the original plaintext.
        assert_eq!(opened.as_slice(), &plaintext[..]);

        // Two seals of the same plaintext must use distinct (random) nonces.
        let (nonce2, ct_tag2) = seal(&dek, aad, plaintext);
        assert_ne!(nonce, nonce2, "OsRng nonces must differ across seals");
        assert_ne!(ct_tag, ct_tag2, "fresh nonce must yield distinct ciphertext");

        // Tamper: flip one ciphertext byte => tag check fails => None.
        let mut tampered = ct_tag.clone();
        tampered[0] ^= 0x01;
        assert!(
            open(&dek, aad, &nonce, &tampered).is_none(),
            "flipped ciphertext byte must fail authentication"
        );

        // Tamper: flip one tag byte (last byte) => None.
        let mut tampered_tag = ct_tag.clone();
        let last = tampered_tag.len() - 1;
        tampered_tag[last] ^= 0x80;
        assert!(
            open(&dek, aad, &nonce, &tampered_tag).is_none(),
            "flipped tag byte must fail authentication"
        );

        // Wrong AAD => None (the identity binding is enforced).
        assert!(
            open(&dek, b"env-ctl/v1/other-identity", &nonce, &ct_tag).is_none(),
            "wrong AAD must fail authentication"
        );

        // Wrong key => None.
        let other = Dek([0x17; 32]);
        assert!(
            open(&other, aad, &nonce, &ct_tag).is_none(),
            "wrong DEK must fail authentication"
        );

        // Malformed nonce length => None, not a panic.
        assert!(
            open(&dek, aad, &nonce[..NONCE_LEN - 1], &ct_tag).is_none(),
            "short nonce must yield None"
        );
        assert!(
            open(&dek, aad, &[], &ct_tag).is_none(),
            "empty nonce must yield None"
        );
    }
}
