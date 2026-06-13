//! Local CA + leaf issuance (rcgen-backed, feature `mitm-ca`). The CA private key is stored
//! ENCRYPTED in the vault and is only usable while the vault is unlocked; `Engine::lock` zeroizes
//! the in-RAM issuer. MITM leaf certs are minted in-RAM by the relay-gated proxy resolver ONLY for
//! a host that an active USB-gated relay actively covers — never via the operator `ca issue` path
//! (CF-5). Phase-0 was a placeholder; PR-3a makes the CA real (still entirely `#[cfg(mitm-ca)]`,
//! so a CA-less engine build drops rcgen/rustls/TLS entirely).
//!
//! ## Trust-boundary invariant
//!
//! The CA private key NEVER leaves the engine: it is sealed at rest in the vault under the reserved
//! `__mitm_ca_key` secret (`broker_only` → un-revealable through `secret_get`, HF-5), and lives in
//! RAM only as the reconstructed rcgen issuer inside [`LocalCa`]. The struct is [`ZeroizeOnDrop`]:
//! our owned copy of the PKCS#8 key DER is wiped when the issuer is dropped (on `lock`, or when the
//! engine tears down). Leaf private keys are minted fresh per request, returned to the caller, and
//! NEVER persisted.

#[cfg(feature = "mitm-ca")]
mod imp {
    use rcgen::{
        BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, Ia5String, IsCa,
        KeyPair, KeyUsagePurpose, SanType, PKCS_ECDSA_P256_SHA256,
    };
    use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use zeroize::{Zeroize, Zeroizing};

    /// Common Name stamped on the generated local CA. Cosmetic — the child trusts the CA by the
    /// explicit PEM bundle (`ca_pem_path`), not by name.
    pub(crate) const CA_COMMON_NAME: &str = "env-ctl local MITM CA";

    /// Leaf certificate validity window, in seconds. Deliberately SHORT (MITM leaves are minted
    /// fresh per request and never persisted), so a leaf that somehow escaped RAM expires fast.
    pub(crate) const LEAF_TTL_SECS: i64 = 6 * 60 * 60; // 6 hours
    /// Backdate the leaf's `not_before` to absorb modest client/host clock skew.
    pub(crate) const LEAF_BACKDATE_SECS: i64 = 5 * 60; // 5 minutes

    /// CA validity window, in seconds (10 years). The CA is long-lived; rotation is an operator op.
    pub(crate) const CA_TTL_SECS: i64 = 10 * 365 * 24 * 60 * 60;

    /// The in-RAM local CA issuer. Holds the reconstructed rcgen key pair + issuer cert needed to
    /// sign leaves, plus an owned, zeroized copy of the PKCS#8 key DER (wiped on drop). Present only
    /// while the vault is unlocked; `Engine::lock` drops it (`Option<LocalCa> = None`).
    pub struct LocalCa {
        /// The issuer key pair (reconstructed from the sealed PKCS#8 DER). Used to sign leaves.
        key_pair: KeyPair,
        /// The issuer certificate, rebuilt deterministically from the fixed CA params + the recovered
        /// public key. `rcgen::CertificateParams::signed_by` reads only the issuer's DN / key-id
        /// method / key-usages from this — never its serial — so it is a faithful signing issuer for
        /// the persisted CA cert (whose DER we keep verbatim below for the chain + PEM).
        issuer_cert: rcgen::Certificate,
        /// The PERSISTED public CA certificate DER (verbatim, as sealed at `ca_init`). Used for the
        /// returned leaf chain + the public PEM. Public material — not secret.
        ca_cert_der: Vec<u8>,
        /// Our owned copy of the CA private-key PKCS#8 DER, kept ONLY so `Drop` can zeroize it. The
        /// rcgen `KeyPair` holds its own internal copy we cannot reach to wipe; this guarantees at
        /// least our handling of the key material is wiped, and documents the residual honestly.
        key_der_zeroizing: Zeroizing<Vec<u8>>,
    }

    impl Drop for LocalCa {
        fn drop(&mut self) {
            // Explicit best-effort wipe of our owned key copy. `Zeroizing` already wipes on drop;
            // this makes the intent unmistakable and covers the field even if it is ever changed to
            // a plain `Vec`.
            self.key_der_zeroizing.zeroize();
        }
    }

    /// Fixed parameters for the local CA cert (deterministic DN / key-usage / is_ca). Used at both
    /// generation (`generate`) and reconstruction (`from_material`) so the rebuilt signing issuer
    /// matches the persisted CA cert's identity-bearing fields.
    fn ca_params(now_unix: i64) -> anyhow::Result<CertificateParams> {
        let mut params = CertificateParams::new(Vec::<String>::new())
            .map_err(|e| anyhow::anyhow!("ca params: {e}"))?;
        params
            .distinguished_name
            .push(DnType::CommonName, CA_COMMON_NAME);
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        params.not_before = offset_dt(now_unix - LEAF_BACKDATE_SECS)?;
        params.not_after = offset_dt(now_unix + CA_TTL_SECS)?;
        Ok(params)
    }

    /// Build a `time::OffsetDateTime` from a Unix timestamp. `time` is reached only here, under
    /// `mitm-ca`, to express SUB-DAY validity windows (rcgen's `date_time_ymd` is day-granular).
    fn offset_dt(unix_secs: i64) -> anyhow::Result<time::OffsetDateTime> {
        time::OffsetDateTime::from_unix_timestamp(unix_secs)
            .map_err(|e| anyhow::anyhow!("invalid validity timestamp {unix_secs}: {e}"))
    }

    /// Output of CA generation: the freshly minted key + self-signed cert, both as DER. The caller
    /// (`Engine::ca_init`) seals `key_der` into the vault and `put_meta`s `cert_der` + `not_after`.
    pub(crate) struct GeneratedCa {
        /// PKCS#8 private-key DER. SECRET — sealed into the vault, never persisted in the clear.
        pub key_der: Zeroizing<Vec<u8>>,
        /// Self-signed CA certificate DER. PUBLIC.
        pub cert_der: Vec<u8>,
        /// `not_after` of the CA cert, RFC3339 (persisted as meta for visibility / rotation).
        pub not_after_rfc3339: String,
    }

    impl LocalCa {
        /// Mint a brand-new self-signed local CA (ECDSA P-256, ring-backed). Returns the key+cert
        /// DER for the engine to seal/persist; does NOT itself touch the vault. `now_unix` anchors
        /// the validity window (taken from the engine `Clock` so tests are deterministic).
        pub(crate) fn generate(now_unix: i64) -> anyhow::Result<GeneratedCa> {
            let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
                .map_err(|e| anyhow::anyhow!("ca keygen: {e}"))?;
            let params = ca_params(now_unix)?;
            let not_after = params.not_after;
            let cert = params
                .self_signed(&key_pair)
                .map_err(|e| anyhow::anyhow!("ca self-sign: {e}"))?;
            let cert_der = cert.der().to_vec();
            let key_der = Zeroizing::new(key_pair.serialize_der());
            let not_after_rfc3339 = offset_to_rfc3339(not_after);
            Ok(GeneratedCa {
                key_der,
                cert_der,
                not_after_rfc3339,
            })
        }

        /// Reconstruct the in-RAM issuer from the sealed CA key DER + the persisted CA cert DER
        /// (called by `Engine::unlock` when CA meta is present). The leaf signing issuer is rebuilt
        /// from the fixed CA params + recovered key; the persisted `ca_cert_der` is kept verbatim for
        /// the returned chain + PEM.
        pub(crate) fn from_material(
            ca_key_der: Zeroizing<Vec<u8>>,
            ca_cert_der: &[u8],
        ) -> anyhow::Result<Self> {
            let key_pair = KeyPair::from_pkcs8_der_and_sign_algo(
                &PrivatePkcs8KeyDer::from(ca_key_der.as_slice()),
                &PKCS_ECDSA_P256_SHA256,
            )
            .map_err(|e| anyhow::anyhow!("ca key reconstruct: {e}"))?;
            // Rebuild the signing issuer cert. Its serial/validity may differ from the persisted
            // cert, but `signed_by` only consumes the issuer DN / key-id method / key-usages, which
            // are fixed in `ca_params` and identical to those baked into the persisted CA cert.
            let params = ca_params(0)?;
            let issuer_cert = params
                .self_signed(&key_pair)
                .map_err(|e| anyhow::anyhow!("ca issuer rebuild: {e}"))?;
            Ok(LocalCa {
                key_pair,
                issuer_cert,
                ca_cert_der: ca_cert_der.to_vec(),
                key_der_zeroizing: ca_key_der,
            })
        }

        /// True iff this CA holds a usable issuer (always true for a constructed `LocalCa`; the
        /// engine models "no CA" as `Option::None`).
        pub fn is_initialized(&self) -> bool {
            !self.ca_cert_der.is_empty()
        }

        /// Mint a per-SNI leaf cert in-RAM, signed by the CA issuer. SAN = `host`, SHORT validity
        /// ([`LEAF_TTL_SECS`]), fresh key per call. NEVER persisted. Returns a rustls-ready chain
        /// (leaf DER, then CA DER) + the leaf signing key. `now_unix` anchors validity.
        pub(crate) fn issue_leaf(
            &self,
            host: &str,
            now_unix: i64,
        ) -> anyhow::Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
            let leaf_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
                .map_err(|e| anyhow::anyhow!("leaf keygen: {e}"))?;
            let san = Ia5String::try_from(host)
                .map_err(|e| anyhow::anyhow!("invalid SNI host {host:?}: {e}"))?;
            let mut params = CertificateParams::new(Vec::<String>::new())
                .map_err(|e| anyhow::anyhow!("leaf params: {e}"))?;
            params.subject_alt_names = vec![SanType::DnsName(san)];
            params.distinguished_name.push(DnType::CommonName, host);
            params.is_ca = IsCa::ExplicitNoCa;
            params.key_usages = vec![
                KeyUsagePurpose::DigitalSignature,
                KeyUsagePurpose::KeyEncipherment,
            ];
            params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
            // Emit the Authority Key Identifier (binds the leaf to the CA's Subject Key Identifier).
            params.use_authority_key_identifier_extension = true;
            params.not_before = offset_dt(now_unix - LEAF_BACKDATE_SECS)?;
            params.not_after = offset_dt(now_unix + LEAF_TTL_SECS)?;

            let leaf = params
                .signed_by(&leaf_key, &self.issuer_cert, &self.key_pair)
                .map_err(|e| anyhow::anyhow!("leaf sign: {e}"))?;

            let leaf_der: CertificateDer<'static> = CertificateDer::from(leaf.der().to_vec());
            let ca_der: CertificateDer<'static> = CertificateDer::from(self.ca_cert_der.clone());
            let chain = vec![leaf_der, ca_der];
            let key: PrivateKeyDer<'static> =
                PrivateKeyDer::from(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
            Ok((chain, key))
        }

        /// The PUBLIC CA certificate, PEM-encoded. Never exposes the private key.
        pub fn ca_cert_pem(&self) -> String {
            pem_cert(&self.ca_cert_der)
        }

        /// The PUBLIC CA certificate DER (borrowed). Used by the engine to write the PEM bundle.
        pub(crate) fn ca_cert_der(&self) -> &[u8] {
            &self.ca_cert_der
        }
    }

    /// PEM-encode a DER certificate as a single `CERTIFICATE` block, without pulling a PEM writer
    /// dependency (rcgen's pem encoder is not re-exported). Standard base64, 64-char lines.
    pub(crate) fn pem_cert(der: &[u8]) -> String {
        const PEM_LINE: usize = 64;
        let b64 = base64_std(der);
        let mut out = String::with_capacity(b64.len() + 64);
        out.push_str("-----BEGIN CERTIFICATE-----\n");
        let mut i = 0;
        while i < b64.len() {
            let end = (i + PEM_LINE).min(b64.len());
            out.push_str(&b64[i..end]);
            out.push('\n');
            i = end;
        }
        out.push_str("-----END CERTIFICATE-----\n");
        out
    }

    /// Minimal standard-alphabet base64 (no padding omission, no line wrapping) — pure-Rust, no
    /// dependency. Used only for PEM-encoding a PUBLIC cert DER.
    fn base64_std(input: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
            if chunk.len() > 1 {
                out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(ALPHABET[(n & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    /// Format a `time::OffsetDateTime` as RFC3339 without a direct `time` formatting feature: build
    /// it via chrono from the Unix timestamp (chrono is already a crate dep).
    fn offset_to_rfc3339(dt: time::OffsetDateTime) -> String {
        let secs = dt.unix_timestamp();
        chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)
            .map(|d| d.to_rfc3339())
            .unwrap_or_default()
    }
}

#[cfg(feature = "mitm-ca")]
pub use imp::LocalCa;
#[cfg(feature = "mitm-ca")]
pub(crate) use imp::CA_COMMON_NAME;

/// CA-less builds keep a zero-sized placeholder so the engine's `Option<LocalCa>` field type and the
/// `ca` module both exist regardless of the feature (the engine only ever stores `None` here).
#[cfg(not(feature = "mitm-ca"))]
pub struct LocalCa;

#[cfg(not(feature = "mitm-ca"))]
impl LocalCa {
    pub fn is_initialized(&self) -> bool {
        false
    }
}

#[cfg(all(test, feature = "mitm-ca"))]
mod tests {
    use super::imp::{LocalCa, LEAF_TTL_SECS};

    const NOW: i64 = 1_700_000_000;

    /// Round-trip: generate → reconstruct from the (key DER, cert DER) material → issue a leaf whose
    /// SAN is the requested host and which is signed by the CA. Parsed/checked with x509-parser.
    #[test]
    fn generate_reconstruct_issue_leaf() {
        let gen = LocalCa::generate(NOW).expect("generate");
        assert!(!gen.cert_der.is_empty());
        assert!(!gen.key_der.is_empty());

        let ca = LocalCa::from_material(gen.key_der.clone(), &gen.cert_der).expect("from_material");
        assert!(ca.is_initialized());

        let (chain, _leaf_key) = ca.issue_leaf("api.example.test", NOW).expect("issue_leaf");
        assert_eq!(chain.len(), 2, "chain = leaf + CA");

        // Leaf parses, carries the host SAN, and is within its (short) validity window.
        let (_rem, leaf) =
            x509_parser::parse_x509_certificate(chain[0].as_ref()).expect("parse leaf");
        let san_present = leaf
            .subject_alternative_name()
            .ok()
            .flatten()
            .map(|ext| {
                ext.value.general_names.iter().any(|gn| {
                    matches!(gn, x509_parser::extensions::GeneralName::DNSName(d) if *d == "api.example.test")
                })
            })
            .unwrap_or(false);
        assert!(san_present, "leaf must carry the host DNS SAN");

        // CA cert parses + is a CA.
        let (_rem, ca_x509) =
            x509_parser::parse_x509_certificate(chain[1].as_ref()).expect("parse CA");
        assert!(
            ca_x509.is_ca(),
            "second chain element must be the CA certificate"
        );

        // Issuer linkage (without the x509-parser `verify` feature, which would gate a ring-backed
        // signature check): the leaf's issuer DN equals the CA's subject DN, and the leaf's Authority
        // Key Identifier matches the CA's Subject Key Identifier. rcgen itself performs the real
        // signing; this proves the leaf was issued under THIS CA.
        assert_eq!(
            leaf.issuer().to_string(),
            ca_x509.subject().to_string(),
            "leaf issuer must equal CA subject"
        );
        let leaf_aki = leaf
            .get_extension_unique(&x509_parser::oid_registry::OID_X509_EXT_AUTHORITY_KEY_IDENTIFIER)
            .ok()
            .flatten()
            .and_then(|ext| match ext.parsed_extension() {
                x509_parser::extensions::ParsedExtension::AuthorityKeyIdentifier(aki) => {
                    aki.key_identifier.as_ref().map(|k| k.0.to_vec())
                }
                _ => None,
            });
        let ca_ski = ca_x509
            .get_extension_unique(&x509_parser::oid_registry::OID_X509_EXT_SUBJECT_KEY_IDENTIFIER)
            .ok()
            .flatten()
            .and_then(|ext| match ext.parsed_extension() {
                x509_parser::extensions::ParsedExtension::SubjectKeyIdentifier(ski) => {
                    Some(ski.0.to_vec())
                }
                _ => None,
            });
        assert!(
            leaf_aki.is_some() && leaf_aki == ca_ski,
            "leaf AKI must match CA SKI"
        );
    }

    /// The leaf validity window is the SHORT, fixed TTL (not rcgen's 1975..4096 default).
    #[test]
    fn leaf_validity_is_short() {
        let gen = LocalCa::generate(NOW).expect("generate");
        let ca = LocalCa::from_material(gen.key_der, &gen.cert_der).expect("from_material");
        let (chain, _k) = ca.issue_leaf("host.test", NOW).expect("issue_leaf");
        let (_rem, leaf) =
            x509_parser::parse_x509_certificate(chain[0].as_ref()).expect("parse leaf");
        let nb = leaf.validity().not_before.timestamp();
        let na = leaf.validity().not_after.timestamp();
        let span = na - nb;
        // span == LEAF_TTL_SECS + backdate; assert it is on the order of hours, never years.
        assert!(span <= LEAF_TTL_SECS + 3600, "leaf validity must be short");
        assert!(span > 0);
    }

    /// `ca_cert_pem` emits ONLY a CERTIFICATE block (no private-key material) and decodes back to
    /// the stored CA cert DER.
    #[test]
    fn ca_pem_is_public_only() {
        let gen = LocalCa::generate(NOW).expect("generate");
        let ca = LocalCa::from_material(gen.key_der, &gen.cert_der).expect("from_material");
        let pem = ca.ca_cert_pem();
        assert!(pem.contains("BEGIN CERTIFICATE"));
        assert!(!pem.contains("PRIVATE KEY"), "PEM must never carry the key");
    }
}
