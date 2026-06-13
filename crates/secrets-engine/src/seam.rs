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
/// IKM that [`crate::keyslot::kek_from_usb`] expects. The signature is fetched by a **direct,
/// pure-Rust HTTPS call** (ring-only `rustls`, already in the resolved graph) to the Seed over the
/// USB link-local interface, validating the Seed's TLS against the **pinned Cognitum CA** — no
/// `ssh`, no `known_hosts`, no agent, no `$HOME` access, so the daemon's Seed path works unchanged
/// under the `env-ctl.service` systemd sandbox AND the no-C trust-boundary gate stays green. Any
/// failure → `None` (fail-closed).
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
///
/// # Transport (systemd-sandbox-safe)
/// The Seed is reached by a **direct, blocking, pure-Rust HTTPS client** (`rustls`, ring-only —
/// already in the resolved graph, so the no-C gate stays green). The server's TLS is validated
/// against the **pinned Cognitum CA only** (loaded from `ENVCTL_SEED_CA`; frozen-roots discipline
/// per FS-S7 — never the OS trust store). This replaces the former `ssh genesis@…` + on-device
/// `curl` tunnel, which broke under `env-ctl.service` (`ProtectHome=read-only` ⇒ no writable
/// `known_hosts`, no agent). No `ssh`, no `$HOME` access, no subprocess.
///
/// # Auth (bearer token, possession-floored)
/// `custody/sign` requires a bearer token minted by the **USB-only** pair window. The token is
/// device-bound and revocable (not a master secret); it is resolved from `ENVCTL_SEED_TOKEN`, else
/// the token file (`ENVCTL_SEED_TOKEN_FILE`, default `$XDG_DATA_HOME/env-ctl/seed-token`, which is
/// inside the unit's `ReadWritePaths`). If absent or rejected, the daemon **re-mints on demand** by
/// re-opening the USB-only pair window (possession of the USB is the floor of trust — ADR-057), so
/// a lost/expired token is self-healing as long as the Seed is present. Every device call is bound
/// by `IO_TIMEOUT` so a wedged device can never hang the synchronous unlock path.
#[cfg(feature = "seed-factor")]
pub(crate) mod seed_factor {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::sync::Arc;
    use std::time::Duration;
    use zeroize::Zeroizing;

    /// Base URL of the Seed REST API. Default = the USB link-local address from the device docs;
    /// overridable for mDNS (`.local`) / WiFi addressing.
    fn api_base() -> String {
        std::env::var("ENVCTL_SEED_API").unwrap_or_else(|_| "https://169.254.42.1:8443".to_string())
    }

    /// Pinned Cognitum CA (PEM). The CA is name-constrained to `169.254.x.x` + `.local` and is
    /// installed system-wide; we pin THIS root explicitly (FS-S7 frozen-roots) rather than trusting
    /// the OS store. Readable under `ProtectSystem=strict` (it lives under `/usr`).
    fn ca_path() -> String {
        std::env::var("ENVCTL_SEED_CA")
            .unwrap_or_else(|_| "/usr/local/share/ca-certificates/cognitum-ca.crt".to_string())
    }

    /// Stable pairing-client name for the daemon. Re-pairing under the same name replaces the
    /// previous token (no per-unlock client leak).
    const CLIENT_NAME: &str = "envctl-daemon";

    /// I/O ceiling for any single device call (connect, read, write). A wedged device drops within
    /// this bound so the synchronous unlock path can never block indefinitely.
    const IO_TIMEOUT: Duration = Duration::from_secs(15);

    /// Device-bound bearer token at rest. Default lives in the unit's `ReadWritePaths`
    /// (`%h/.local/share/env-ctl`) so the daemon can both read and refresh it under the sandbox.
    fn token_file() -> std::path::PathBuf {
        if let Ok(p) = std::env::var("ENVCTL_SEED_TOKEN_FILE") {
            return std::path::PathBuf::from(p);
        }
        let base = std::env::var("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join(".local/share")
            });
        base.join("env-ctl").join("seed-token")
    }

    /// Resolve a bearer token: explicit env override first, then the token file. Trimmed; empty
    /// ⇒ `None`.
    fn resolve_token() -> Option<Zeroizing<String>> {
        if let Ok(t) = std::env::var("ENVCTL_SEED_TOKEN") {
            let t = t.trim().to_string();
            if !t.is_empty() {
                return Some(Zeroizing::new(t));
            }
        }
        let raw = std::fs::read_to_string(token_file()).ok()?;
        let t = raw.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(Zeroizing::new(t))
        }
    }

    /// Persist a freshly minted token at `0600` (best-effort; failure just means we re-pair next
    /// time).
    fn store_token(token: &str) {
        use std::os::unix::fs::OpenOptionsExt;
        let path = token_file();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
        {
            let _ = f.write_all(token.as_bytes());
        }
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

    /// Build the pinned-CA, ring-only rustls client config. Loads ONLY the Cognitum CA as the trust
    /// root (frozen-roots; NOT the OS store). `None` if the CA is missing / unreadable / empty.
    fn tls_config() -> Option<Arc<rustls::ClientConfig>> {
        let pem = std::fs::read(ca_path()).ok()?;
        let mut roots = rustls::RootCertStore::empty();
        let mut rd = std::io::BufReader::new(&pem[..]);
        for cert in rustls_pemfile::certs(&mut rd) {
            roots.add(cert.ok()?).ok()?;
        }
        if roots.is_empty() {
            return None;
        }
        let cfg = rustls::ClientConfig::builder_with_provider(
            rustls::crypto::ring::default_provider().into(),
        )
        .with_safe_default_protocol_versions()
        .ok()?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Some(Arc::new(cfg))
    }

    /// Split an `https://host:port` base into `(host, port)` (port defaults to 443).
    fn host_port(base: &str) -> Option<(String, u16)> {
        let rest = base.strip_prefix("https://")?;
        let rest = rest.split('/').next().unwrap_or(rest);
        match rest.rsplit_once(':') {
            Some((h, p)) => Some((h.to_string(), p.parse().ok()?)),
            None => Some((rest.to_string(), 443)),
        }
    }

    /// Parse the numeric status code from an HTTP/1.1 status line (`HTTP/1.1 200 OK`).
    fn parse_status(resp: &str) -> Option<u16> {
        resp.lines().next()?.split_whitespace().nth(1)?.parse().ok()
    }

    /// Extract a JSON string field value (`"name":"value"`) by scanning the raw response — robust
    /// against chunked transfer framing (chunk-size lines never contain the field name). The value
    /// is read up to the next quote (the Seed's signature/token values contain no escaped quotes).
    fn extract_field(resp: &str, name: &str) -> Option<String> {
        let key = format!("\"{name}\"");
        let after = &resp[resp.find(&key)? + key.len()..];
        let after = after.trim_start().strip_prefix(':')?.trim_start();
        let after = after.strip_prefix('"')?;
        let end = after.find('"')?;
        let val = &after[..end];
        if val.is_empty() {
            None
        } else {
            Some(val.to_string())
        }
    }

    /// One blocking HTTPS request to the Seed; returns `(status, raw_response_text)`.
    /// `Connection: close` makes the body close-delimited, so we read to EOF (rustls may surface a
    /// non-graceful TCP close as an error *after* the body is already buffered — tolerated). `None`
    /// on transport failure.
    fn https(
        cfg: &Arc<rustls::ClientConfig>,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Option<(u16, String)> {
        let (host, port) = host_port(&api_base())?;
        let server_name = rustls::pki_types::ServerName::try_from(host.clone()).ok()?;
        let mut conn = rustls::ClientConnection::new(Arc::clone(cfg), server_name).ok()?;
        let addr = (host.as_str(), port).to_socket_addrs().ok()?.next()?;
        let mut sock = TcpStream::connect_timeout(&addr, IO_TIMEOUT).ok()?;
        sock.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
        sock.set_write_timeout(Some(IO_TIMEOUT)).ok()?;
        let mut tls = rustls::Stream::new(&mut conn, &mut sock);

        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (k, v) in headers {
            req.push_str(k);
            req.push_str(": ");
            req.push_str(v);
            req.push_str("\r\n");
        }
        req.push_str("\r\n");
        req.push_str(body);
        tls.write_all(req.as_bytes()).ok()?;
        tls.flush().ok()?;

        let mut buf = Vec::new();
        let _ = tls.read_to_end(&mut buf); // non-graceful close after full body ⇒ tolerated
        let text = String::from_utf8_lossy(&buf).into_owned();
        let status = parse_status(&text)?;
        Some((status, text))
    }

    /// `POST /api/v1/custody/sign` with a bearer token. Returns `(status, signature?)`.
    fn custody_sign(
        cfg: &Arc<rustls::ClientConfig>,
        token: &str,
        data: &str,
    ) -> Option<(u16, Option<String>)> {
        let body = format!("{{\"data\":\"{data}\"}}");
        let auth = format!("Bearer {token}");
        let (status, resp) = https(
            cfg,
            "POST",
            "/api/v1/custody/sign",
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
            ],
            &body,
        )?;
        Some((status, extract_field(&resp, "signature")))
    }

    /// `(status, signature)` → the signature only when the call was `2xx` and non-empty.
    fn ok_sig(res: Option<(u16, Option<String>)>) -> Option<String> {
        let (status, sig) = res?;
        if !(200..300).contains(&status) {
            return None;
        }
        sig.filter(|s| !s.is_empty())
    }

    /// Re-mint a device-bound bearer token via the **USB-only** pair window (possession floor) and
    /// persist it. `None` if the window/pair is unavailable (e.g. Seed absent).
    fn pair_and_store(cfg: &Arc<rustls::ClientConfig>) -> Option<Zeroizing<String>> {
        let (w, _) = https(cfg, "POST", "/api/v1/pair/window", &[], "")?;
        if !(200..300).contains(&w) {
            return None;
        }
        let body = format!("{{\"client_name\":\"{CLIENT_NAME}\"}}");
        let (p, resp) = https(
            cfg,
            "POST",
            "/api/v1/pair",
            &[("Content-Type", "application/json")],
            &body,
        )?;
        if !(200..300).contains(&p) {
            return None;
        }
        let token = extract_field(&resp, "token")?;
        store_token(&token);
        Some(Zeroizing::new(token))
    }

    /// Sign arbitrary `data` with the Seed's Ed25519 device key over the REST custody API and return
    /// the 128-char hex signature. `None` on any failure (Seed unreachable / unpaired / empty).
    /// Single implementation shared by the KEK probe and the presence gate (Profile S).
    ///
    /// Flow: validate TLS against the pinned Cognitum CA → try the stored/env bearer token → on a
    /// missing or rejected token, re-mint once via the USB-only pair window (possession floor) and
    /// retry. Every device call is bounded by `IO_TIMEOUT` so a wedged device cannot hang the
    /// synchronous unlock path.
    pub(crate) fn sign_hex(data: &str) -> Option<String> {
        let cfg = tls_config()?;

        // 1. Try an already-provisioned token (env override or token file).
        if let Some(token) = resolve_token() {
            if let Some(sig) = ok_sig(custody_sign(&cfg, &token, data)) {
                return Some(sig);
            }
            // else: token revoked / expired / forbidden — fall through to re-mint.
        }

        // 2. Re-mint on demand (USB possession is the trust floor) and retry once.
        let token = pair_and_store(&cfg)?;
        ok_sig(custody_sign(&cfg, &token, data))
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
        use super::{extract_field, host_port, parse_sig_hex, parse_status};

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

        #[test]
        fn host_port_splits_base_url() {
            assert_eq!(
                host_port("https://169.254.42.1:8443"),
                Some(("169.254.42.1".to_string(), 8443))
            );
            assert_eq!(
                host_port("https://seed.local:8443/api/v1"),
                Some(("seed.local".to_string(), 8443))
            );
            // No explicit port ⇒ HTTPS default.
            assert_eq!(
                host_port("https://seed.local"),
                Some(("seed.local".to_string(), 443))
            );
            assert_eq!(host_port("http://nope"), None, "https only");
        }

        #[test]
        fn parse_status_reads_code() {
            assert_eq!(parse_status("HTTP/1.1 200 OK\r\n\r\n{}"), Some(200));
            assert_eq!(parse_status("HTTP/1.1 401 Unauthorized\r\n"), Some(401));
            assert_eq!(parse_status("garbage"), None);
        }

        #[test]
        fn extract_field_scans_json_value() {
            // A 2xx custody/sign body — note the value is the (public) hex signature.
            let body = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"signature\": \"90017fcc0e\", \"client\":\"envctl-daemon\"}";
            assert_eq!(
                extract_field(body, "signature"),
                Some("90017fcc0e".to_string())
            );
            assert_eq!(
                extract_field(body, "client"),
                Some("envctl-daemon".to_string())
            );
            // Tolerates chunked framing: a chunk-size line between header and body.
            let chunked = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1f\r\n{\"token\":\"abc.def.ghi\"}\r\n0\r\n\r\n";
            assert_eq!(
                extract_field(chunked, "token"),
                Some("abc.def.ghi".to_string())
            );
            assert_eq!(extract_field(body, "missing"), None);
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
