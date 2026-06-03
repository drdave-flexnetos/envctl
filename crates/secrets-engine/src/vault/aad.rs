//! Fixed-width canonical AAD, recomputed at decrypt time and NEVER stored (HF-2). Binding the
//! record's table, row id, version, and DEK generation into the AEAD AAD makes a ciphertext
//! un-relocatable to another row/version/generation.
//!
//! Layout ( research/01-aead-at-rest.md §"Concrete guidance" point 3, HF-2):
//!
//! ```text
//!   b"env-ctl/v1/aad"   (14-byte domain-separation prefix, constant)
//!   u8   table_tag      (1 byte)
//!   i64be row_id        (8 bytes, big-endian)
//!   i64be version       (8 bytes, big-endian)
//!   i64be dek_generation(8 bytes, big-endian)
//!   ----------------------------------------
//!   total = 14 + 1 + 8 + 8 + 8 = 39 bytes, ALWAYS.
//! ```
//!
//! Every field is fixed-width — there are no length prefixes or var-ints, so two distinct
//! tuples can never produce the same byte string (no parsing ambiguity). The AAD is
//! authenticated-but-not-encrypted by XChaCha20-Poly1305; it carries record *identity* only,
//! never plaintext secret material. Decryption reconstructs this exact AAD from the row being
//! loaded, so a ciphertext copied to a different (tag, row_id, version, dek_generation) fails
//! the Poly1305 tag check.
//!
//! Integer width (schema consistency): the record-identity fields are `i64`, matching the live
//! schema (`Keyslot.id: i64`, `Keyslot.dek_generation: i64`, `Store::append_audit -> i64`) and the
//! sibling `keyslot::keyslot_aad`, which also serializes its identity fields as `i64::to_be_bytes`.
//! Encoding `i64` here (rather than `u64`) removes the cross-encoder disagreement and avoids a
//! lossy `as u64` cast at every Phase-1 call site. The research doc writes the binding as
//! `u64be(...)`; `i64::to_be_bytes()` is bit-identical to `u64::to_be_bytes()` for any non-negative
//! id/version/generation (which is the only legitimate domain), so this is layout-compatible with
//! the doc while staying type-honest with the schema.

/// Domain-separation prefix. Versioned (`/v1/`) so a future layout change is unambiguous and
/// can never authenticate against a v1 record.
const AAD_DOMAIN: &[u8] = b"env-ctl/v1/aad";

/// Exact, fixed serialized width of `record_aad`'s output. Asserted by the golden test below.
const AAD_LEN: usize = AAD_DOMAIN.len() + 1 + 8 + 8 + 8; // 14 + 1 + 24 = 39

// Compile-time guarantee of the wire width, independent of any runtime build profile: the doc
// comment promises "39 bytes, ALWAYS", and a release build compiles out `debug_assert!`. This
// const assertion makes the invariant a hard compile error if the layout ever drifts.
const _: () = assert!(AAD_LEN == 39, "record AAD must be exactly 39 bytes wide");

#[repr(u8)]
pub enum TableTag {
    SecretVersion = 1,
    CaKey = 2,
    Cert = 3,
    HmacKey = 4,
}

/// Canonical AAD bytes for one record. Fixed-width, big-endian, tag-prefixed.
///
/// `row_id` / `version` / `dek_generation` are `i64` to match the live schema and `keyslot_aad`.
/// They are emitted as `i64::to_be_bytes()`, which is bit-identical to `u64::to_be_bytes()` for the
/// only legitimate (non-negative) domain — see the module doc.
pub fn record_aad(tag: TableTag, row_id: i64, version: i64, dek_generation: i64) -> Vec<u8> {
    // Pre-size to the exact width so the `Vec` never reallocates while we extend.
    let mut out = Vec::with_capacity(AAD_LEN);
    out.extend_from_slice(AAD_DOMAIN);
    // `#[repr(u8)]` guarantees a 1-byte discriminant; `as u8` reads it directly.
    out.push(tag as u8);
    out.extend_from_slice(&row_id.to_be_bytes());
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&dek_generation.to_be_bytes());
    debug_assert_eq!(out.len(), AAD_LEN, "AAD must be fixed-width ({AAD_LEN} bytes)");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Known-answer / golden-bytes vector: pin the EXACT on-the-wire layout so any accidental
    /// reordering, endianness flip, or prefix change is caught. If this test changes, the AAD
    /// format changed and every at-rest record's authentication is affected — treat as breaking.
    #[test]
    fn golden_bytes_exact_layout() {
        // tag = HmacKey (4), row_id = 1, version = 0x0102, dek_generation = 0xFF.
        let aad = record_aad(TableTag::HmacKey, 1, 0x0102, 0xFF);

        let expected: Vec<u8> = {
            let mut v = Vec::new();
            v.extend_from_slice(b"env-ctl/v1/aad"); // 14-byte domain prefix
            v.push(0x04); //                            u8(tag=HmacKey)
            v.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x01]); // u64be row_id = 1
            v.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x01, 0x02]); // u64be version = 0x0102
            v.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0xFF]); // u64be dek_generation = 0xFF
            v
        };

        assert_eq!(aad, expected, "AAD byte layout drifted from the canonical spec");
        assert_eq!(aad.len(), 39, "AAD must be exactly 39 bytes wide");
        assert_eq!(aad.len(), AAD_LEN);
    }

    /// The all-zero tuple under each tag: the prefix is always present and the discriminant byte
    /// sits exactly after the 14-byte prefix.
    #[test]
    fn prefix_and_tag_position() {
        for (tag, disc) in [
            (TableTag::SecretVersion, 1u8),
            (TableTag::CaKey, 2),
            (TableTag::Cert, 3),
            (TableTag::HmacKey, 4),
        ] {
            let aad = record_aad(tag, 0, 0, 0);
            assert_eq!(&aad[..AAD_DOMAIN.len()], AAD_DOMAIN, "domain prefix missing");
            assert_eq!(aad[AAD_DOMAIN.len()], disc, "tag discriminant misplaced/wrong");
            assert_eq!(aad.len(), AAD_LEN);
        }
    }

    /// Mint a fresh `TableTag` from an index. `TableTag` is intentionally not `Copy` (it models a
    /// schema choice, not a value to pass around freely), so the collision sweep re-mints it per
    /// iteration rather than cloning.
    fn tag_for(idx: usize) -> TableTag {
        match idx {
            0 => TableTag::SecretVersion,
            1 => TableTag::CaKey,
            2 => TableTag::Cert,
            _ => TableTag::HmacKey,
        }
    }

    /// Injectivity: across a cartesian product of distinct tags, ids, versions and generations,
    /// no two distinct tuples ever produce the same AAD bytes (fixed-width => collision-free).
    /// This is the property that makes a ciphertext un-relocatable to another row/version/gen.
    #[test]
    fn distinct_tuples_never_collide() {
        // Values chosen to expose endianness / field-boundary bugs: a value that lands as
        // "row=0x0100" must not collide with "version=0x0001" etc. Includes adjacent and
        // max-ish values (now `i64`, matching the schema) to stress the high/low byte ordering.
        let scalars: [i64; 7] = [0, 1, 2, 0x0100, 0x0001_0000, i64::MAX - 1, i64::MAX];

        let mut seen: HashSet<Vec<u8>> = HashSet::new();
        let mut count = 0usize;

        for ti in 0..4usize {
            for &row in &scalars {
                for &ver in &scalars {
                    for &gen in &scalars {
                        let aad = record_aad(tag_for(ti), row, ver, gen);
                        assert!(
                            seen.insert(aad),
                            "collision for (tag_idx={ti}, row={row:#x}, ver={ver:#x}, gen={gen:#x})"
                        );
                        count += 1;
                    }
                }
            }
        }

        // 4 tags * 7 * 7 * 7 = 1372 distinct tuples, all unique.
        assert_eq!(count, 4 * 7 * 7 * 7);
        assert_eq!(seen.len(), count, "every tuple must map to a unique AAD");
    }

    /// Direct pairwise check that the field that *differs* is the only thing that changes the
    /// bytes — i.e. swapping which field holds a value yields different AAD (no aliasing between
    /// row_id / version / dek_generation).
    #[test]
    fn fields_are_not_aliased() {
        let row_set = record_aad(TableTag::SecretVersion, 5, 0, 0);
        let ver_set = record_aad(TableTag::SecretVersion, 0, 5, 0);
        let gen_set = record_aad(TableTag::SecretVersion, 0, 0, 5);
        assert_ne!(row_set, ver_set);
        assert_ne!(row_set, gen_set);
        assert_ne!(ver_set, gen_set);

        // Changing only the tag also changes the bytes.
        let as_cert = record_aad(TableTag::Cert, 5, 0, 0);
        assert_ne!(row_set, as_cert);
    }
}
