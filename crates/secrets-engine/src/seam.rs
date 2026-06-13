//! The behavioral seams (the envctl `HookRunner` family) — all `Send + Sync` so the `Engine`
//! stays `Send + Sync`. Real impls live here; fakes for tests are injected via `Engine::with_seams`.
use zeroize::Zeroizing;

/// Wall + monotonic clock. `boottime_ms` is a `CLOCK_BOOTTIME` cross-check for clock-rollback
/// defense on the 24h relay window (OI-6).
pub trait Clock: Send + Sync {
    fn now(&self) -> chrono::DateTime<chrono::Utc>;
    fn boottime_ms(&self) -> i64;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
    /// `CLOCK_BOOTTIME` in milliseconds: a monotonic counter since boot that INCLUDES suspend time
    /// and CANNOT be stepped backward by the operator, NTP, or a settimeofday() rollback — exactly
    /// the property the OI-6 relay rollback fence needs. Read via `rustix::time::clock_gettime`
    /// (pure-Rust linux_raw syscall on Linux; no C). Saturating ms conversion; never panics.
    fn boottime_ms(&self) -> i64 {
        let ts = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
        ts.tv_sec
            .saturating_mul(1000)
            .saturating_add(ts.tv_nsec / 1_000_000)
    }
}

/// USB key probe. Resolves the GPT PARTUUID as a pre-filter, then returns the keyfile bytes so
/// the engine can PROVE possession (by unwrapping the USB keyslot). `None` => USB absent or
/// possession unproven (fail-closed). UUID match alone is NOT presence (CF-4/OI-5).
pub trait UsbProbe: Send + Sync {
    fn keyfile_for(&self, partition_uuid: &str) -> Option<Zeroizing<Vec<u8>>>;
}

/// Production USB possession probe.
///
/// **Default build** (no `seed-factor`): no hardware backend is compiled in, so this returns
/// `None` — "USB absent", the correct fail-closed default (callers gate on `Some`; this is *not*
/// a panic).
///
/// **Under `seed-factor`**: possession is proven by the **Cognitum Seed** hardware root of trust.
/// The Seed's Ed25519 device key (private key never leaves the device) deterministically signs a
/// fixed, PARTUUID-bound domain-separated message via `POST /api/v1/custody/sign`. Ed25519 signing
/// is deterministic (verified by spike 2026-06-13, stable across a device restart), so the 64-byte
/// signature is reproducible key material that ONLY a holder of the Seed can produce — exactly the
/// IKM that [`crate::keyslot::kek_from_usb`] expects. The signature is fetched over the documented
/// SSH access path via `std::process` (the system `ssh`), so **no linked dependency is added and
/// the no-C trust-boundary gate stays green**. Any failure → `None` (fail-closed).
pub struct RealUsbProbe;

impl UsbProbe for RealUsbProbe {
    #[cfg(not(feature = "seed-factor"))]
    fn keyfile_for(&self, _uuid: &str) -> Option<Zeroizing<Vec<u8>>> {
        None
    }

    #[cfg(feature = "seed-factor")]
    fn keyfile_for(&self, partition_uuid: &str) -> Option<Zeroizing<Vec<u8>>> {
        seed_factor::keyfile_for(partition_uuid)
    }
}

/// Cognitum Seed possession backend for [`RealUsbProbe`]. Isolated so the default build compiles
/// none of it. See `PLAN-cognitum-seed-envctl-vault-factor.md` (meta root) for the design + spike
/// evidence.
#[cfg(feature = "seed-factor")]
pub(crate) mod seed_factor {
    use zeroize::Zeroizing;

    /// SSH target for the Seed; overridable for mDNS / WiFi addressing. Default is the USB
    /// link-local address from the device docs.
    fn ssh_target() -> String {
        std::env::var("ENVCTL_SEED_SSH").unwrap_or_else(|_| "genesis@169.254.42.1".to_string())
    }

    /// Domain-separated, PARTUUID-bound context the Seed signs. Binding the slot UUID into the
    /// message means a different slot derives a different KEK from the same device key.
    fn kek_context(partition_uuid: &str) -> String {
        std::env::var("ENVCTL_SEED_KEK_CONTEXT")
            .unwrap_or_else(|_| format!("envctl/usb-kek/v1/{partition_uuid}"))
    }

    /// Decode a 128-char hex Ed25519 signature into 64 bytes. `None` on any malformed input
    /// (wrong length / non-hex) — fail-closed.
    pub(crate) fn parse_sig_hex(s: &str) -> Option<[u8; 64]> {
        let s = s.trim();
        if s.len() != 128 {
            return None;
        }
        let mut out = [0u8; 64];
        for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
            let hi = (chunk[0] as char).to_digit(16)?;
            let lo = (chunk[1] as char).to_digit(16)?;
            out[i] = ((hi << 4) | lo) as u8;
        }
        Some(out)
    }

    /// Device-side script: open the localhost-only pairing window, pair a transient client, sign
    /// the context message, print ONLY the hex signature, then unpair. The bearer token never
    /// leaves the device; only the (public) signature returns on stdout.
    fn device_script(context: &str) -> String {
        format!(
            r#"set -e
B=https://localhost:8443/api/v1
curl -sk -X POST $B/pair/window >/dev/null
P=$(curl -sk -X POST $B/pair -H 'Content-Type: application/json' -d '{{"client_name":"envctl-kek"}}')
T=$(printf '%s' "$P" | grep -oE '"token"[[:space:]]*:[[:space:]]*"[^"]+"' | head -1 | sed -E 's/.*"([^"]+)".*/\1/')
S=$(curl -sk -X POST $B/custody/sign -H "Authorization: Bearer $T" -H 'Content-Type: application/json' -d '{{"data":"{context}"}}')
curl -sk -X DELETE $B/pair/envctl-kek -H "Authorization: Bearer $T" >/dev/null 2>&1 || true
printf '%s' "$S" | grep -oE '"signature"[[:space:]]*:[[:space:]]*"[0-9a-fA-F]+"' | sed -E 's/.*"([0-9a-fA-F]+)".*/\1/'
"#
        )
    }

    /// Sign arbitrary `data` with the Seed's Ed25519 device key over the documented SSH path and
    /// return the 128-char hex signature. `None` on any failure (Seed unreachable / unpaired /
    /// empty). Single SSH+sign implementation shared by the KEK probe and the presence gate
    /// (Profile S).
    pub(crate) fn sign_hex(data: &str) -> Option<String> {
        use std::io::Write;
        let target = ssh_target();
        let script = device_script(data);
        let mut child = std::process::Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                &target,
                "bash -s",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(script.as_bytes()).ok()?;
        let out = child.wait_with_output().ok()?;
        if !out.status.success() {
            return None;
        }
        let hex = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if hex.is_empty() {
            None
        } else {
            Some(hex)
        }
    }

    /// Resolve the USB keyslot keyfile from the Seed: the deterministic signature over the
    /// PARTUUID-bound KEK context, as 64 raw bytes. `partition_uuid` binds the derived KEK to the
    /// specific slot. Returns `None` on any failure so the engine fails closed.
    ///
    // HARDENING (follow-up): verify the returned signature against the Seed's pinned Ed25519 device
    // public key before use, for a clean possession error instead of a downstream KEK mismatch.
    pub(super) fn keyfile_for(partition_uuid: &str) -> Option<Zeroizing<Vec<u8>>> {
        let sig = parse_sig_hex(&sign_hex(&kek_context(partition_uuid))?)?;
        Some(Zeroizing::new(sig.to_vec()))
    }

    #[cfg(test)]
    mod tests {
        use super::parse_sig_hex;

        #[test]
        fn parse_sig_hex_roundtrips_64_bytes() {
            // The spike signature (2026-06-13) — a real 128-hex Ed25519 signature.
            let hex = "90017fccf53948ce509c216d1cf64c6cdd75d50a9f28e63cef27d6706a7b4c765de7a2849dc8c1d6b19f5ee6e3211b8142b669ca8b6c1fb16a6dc989dc5fa60e";
            let b = parse_sig_hex(hex).expect("valid 128-hex parses");
            assert_eq!(b.len(), 64);
            assert_eq!(b[0], 0x90);
            assert_eq!(b[63], 0x0e);
        }

        #[test]
        fn parse_sig_hex_rejects_malformed() {
            assert!(parse_sig_hex("dead").is_none(), "too short");
            assert!(parse_sig_hex(&"zz".repeat(64)).is_none(), "non-hex");
            assert!(
                parse_sig_hex(&"00".repeat(63)).is_none(),
                "126 hex = wrong length"
            );
        }
    }
}

pub struct MintRequest {
    pub provider: crate::broker::Provider,
    pub repos: Vec<String>,
    pub perms: Vec<String>,
    pub ttl_secs: i64,
}
pub struct ScopedToken {
    pub token: Zeroizing<Vec<u8>>,
    pub expires_at: i64,
}
#[derive(Debug, thiserror::Error)]
pub enum MintError {
    #[error("provider does not support native sub-tokens")]
    Unsupported,
    #[error("{0}")]
    Other(String),
}
/// Optional native scoped sub-token minting (GitHub fine-grained PAT / App token, OpenAI project
/// key). Defaults to `Unsupported` so the proxy-swap path is the universal fallback.
pub trait ProviderMint: Send + Sync {
    fn mint_scoped(&self, _p: &MintRequest) -> Result<ScopedToken, MintError> {
        Err(MintError::Unsupported)
    }
}
pub struct NoMint;
impl ProviderMint for NoMint {}

#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    #[error("upstream io: {0}")]
    Io(String),
    #[error("upstream host not allowlisted: {0}")]
    HostNotAllowed(String),
}
/// The egress sender. The daemon impl MUST verify TLS against the FROZEN webpki-roots store —
/// never the local CA or the OS store (FS-S7) — and only after the engine has confirmed the
/// upstream host is in the provider's canonical allowlist (HF-11).
#[async_trait::async_trait]
pub trait Upstream: Send + Sync {
    async fn send(
        &self,
        req: crate::EgressReq,
        real_key: &Zeroizing<Vec<u8>>,
    ) -> Result<crate::EgressResp, UpstreamError>;
}
