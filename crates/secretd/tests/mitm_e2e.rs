//! PR-3b HTTPS_PROXY/CONNECT MITM acceptance test.
//!
//! End-to-end over the REAL `serve_proxy` listener: a client issues `CONNECT testhost:443`, the proxy
//! mints a per-host leaf from the engine's local CA, terminates the client's TLS with it, reads the
//! plaintext request, and runs it through the SAME `relay_swap` as the plain ingress. The recording
//! `Upstream` asserts the REAL key (`SENTINEL`) reaches it and returns a known body that the proxy
//! streams back over the terminated TLS.
//!
//! Load-bearing assertions:
//!   * the TLS handshake SUCCEEDS against a client trusting ONLY the engine's CA (the minted leaf
//!     validates) — proving the MITM leaf chains to our CA and matches `testhost`;
//!   * the response body is the upstream's known body;
//!   * the child sent ONLY the bearer; the REAL key (`SENTINEL`) NEVER appears in the drained events;
//!   * a SECOND request on the same tunnel works (keep-alive);
//!   * a `CONNECT` for an UNCOVERED host is refused (bare 502) with NO MITM (no leaf, no TLS).
#![cfg(feature = "mitm-ca")]

use std::sync::{Arc, Mutex};

use envctl_secrets::seam::{NoMint, SystemClock, UpstreamError, UsbProbe};
use envctl_secrets::vault::{InMemStore, Store};
use envctl_secrets::{
    EgressReq, EgressResp, Engine, EventSink, Method, Provider, RelayKind, RelayPolicy,
    SecretEvent, SecretMeta, SwapMode, Unlock, Upstream,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;

const SENTINEL: &[u8] = b"REAL-KEY-SENTINEL";
const USB_UUID: &str = "MITM-E2E-USB";
const TEST_HOST: &str = "api.anthropic.com";
const UPSTREAM_BODY: &[u8] = b"{\"ok\":true,\"from\":\"upstream\"}";

// ---- fakes -----------------------------------------------------------------------------------

struct PresentUsb(Zeroizing<Vec<u8>>);
impl UsbProbe for PresentUsb {
    fn keyfile_for(&self, uuid: &str) -> Option<Zeroizing<Vec<u8>>> {
        (uuid == USB_UUID).then(|| self.0.clone())
    }
}

/// A recording upstream: asserts it received the REAL key (`SENTINEL`) and that the bearer/token_id
/// never reached it, then pumps a known body back through the proxy's per-request sink (the
/// `__test_pump_response_body` hook runs in the same task as the `EGRESS_CTX` scope).
#[derive(Clone)]
struct RecordingUpstream {
    seen_key: Arc<Mutex<Option<Vec<u8>>>>,
    seen_headers: Arc<Mutex<Vec<(String, String)>>>,
}
#[async_trait::async_trait]
impl Upstream for RecordingUpstream {
    async fn send(
        &self,
        req: EgressReq,
        real_key: &Zeroizing<Vec<u8>>,
    ) -> Result<EgressResp, UpstreamError> {
        *self.seen_key.lock().unwrap() = Some(real_key.to_vec());
        *self.seen_headers.lock().unwrap() = req.headers.clone();
        // Stream the known body back to the proxy exactly as a real upstream's pump would.
        envctl_secretd::proxy::__test_pump_response_body(hyper::body::Bytes::from_static(
            UPSTREAM_BODY,
        ))
        .await;
        Ok(EgressResp {
            status: 200,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            allowed: true,
        })
    }
}

// ---- helpers ----------------------------------------------------------------------------------

fn fast_params() -> envctl_secrets::keyslot::Argon2Params {
    envctl_secrets::keyslot::Argon2Params {
        m_kib: envctl_secrets::keyslot::ARGON2_M_KIB_FLOOR,
        t_cost: envctl_secrets::keyslot::ARGON2_T_COST_FLOOR,
        p_lanes: 1,
    }
}

fn covering_policy() -> RelayPolicy {
    RelayPolicy {
        relay_id: "mitm-e2e".to_string(),
        kind: RelayKind::Named,
        provider: Provider::Anthropic,
        secret_name: "anthropic_key".to_string(),
        swap: SwapMode::ProxyMitm,
        host_allow: vec![TEST_HOST.to_string()],
        path_allow: vec!["/".to_string()],
        method_allow: vec![Method::Post, Method::Get],
        policy_ttl_secs: 86_400,
        rate_per_min: None,
        quota_total_requests: None,
        quota_total_bytes: None,
        enabled: true,
        revoked: false,
    }
}

/// Build a rustls `ClientConfig` trusting ONLY the engine's CA PEM (no webpki/native roots) — so a
/// successful handshake PROVES the minted leaf chains to our CA. ring-only, no aws-lc-rs.
fn client_config_trusting_only(ca_pem: &std::path::Path) -> rustls::ClientConfig {
    let pem = std::fs::read(ca_pem).expect("read CA pem");
    let mut rd = std::io::BufReader::new(&pem[..]);
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut rd) {
        roots
            .add(cert.expect("parse CA cert"))
            .expect("add CA root");
    }
    assert!(!roots.is_empty(), "the CA pem must yield at least one root");
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring safe protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// Send `CONNECT host:443` over a fresh TCP stream and read the status line. Returns the raw TCP
/// stream (positioned just after the CONNECT response head) plus the parsed status code.
async fn connect_tunnel(addr: std::net::SocketAddr, host: &str) -> (tokio::net::TcpStream, u16) {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("tcp connect");
    let req = format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .await
        .expect("write CONNECT");

    // Read until the end of the CONNECT response head (\r\n\r\n).
    let mut buf = Vec::new();
    let mut tmp = [0u8; 256];
    loop {
        let n = stream.read(&mut tmp).await.expect("read CONNECT resp");
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    (stream, status)
}

/// Over an already-established TLS tunnel, send one plaintext HTTP/1.1 POST carrying ONLY the bearer
/// in the provider auth header, and return the full response (head+body) bytes.
async fn send_plaintext_request<S>(tls: &mut S, host: &str, bearer: &str) -> Vec<u8>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let body = b"{\"q\":1}";
    let req = format!(
        "POST /v1/messages HTTP/1.1\r\nHost: {host}\r\nx-api-key: {bearer}\r\n\
         content-type: application/json\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n",
        body.len()
    );
    tls.write_all(req.as_bytes()).await.expect("write req head");
    tls.write_all(body).await.expect("write req body");
    tls.flush().await.expect("flush");

    // Read the response until we have the full known body (content-length is small + fixed).
    let mut out = Vec::new();
    let mut tmp = [0u8; 512];
    loop {
        let n = tls.read(&mut tmp).await.expect("read resp");
        if n == 0 {
            break;
        }
        out.extend_from_slice(&tmp[..n]);
        if out.windows(UPSTREAM_BODY.len()).any(|w| w == UPSTREAM_BODY) {
            break;
        }
    }
    out
}

// ---- the test ---------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mitm_terminates_tls_and_swaps_through_relay() {
    use tokio_rustls::TlsConnector;

    let root = std::env::temp_dir().join(format!("envctl-mitm-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = envctl_secrets::paths::Paths::under(root.clone());
    std::fs::create_dir_all(&paths.runtime).unwrap();

    let rec = RecordingUpstream {
        seen_key: Arc::new(Mutex::new(None)),
        seen_headers: Arc::new(Mutex::new(Vec::new())),
    };
    let keyfile = Zeroizing::new(vec![0x5Au8; 64]);
    let engine = Engine::with_seams(
        paths.clone(),
        Box::new(InMemStore::new()) as Box<dyn Store>,
        Box::new(SystemClock),
        Box::new(PresentUsb(keyfile.clone())),
        Box::new(NoMint),
        Box::new(rec.clone()),
    )
    .expect("with_seams");

    let (sink, rx) = EventSink::channel();
    engine
        .init_vault(
            Zeroizing::new("correct horse battery staple".to_string()),
            Some(USB_UUID.to_string()),
            Some(keyfile.clone()),
            fast_params(),
            &sink,
        )
        .expect("init_vault");
    engine
        .unlock(
            Unlock::Passphrase(Zeroizing::new("correct horse battery staple".to_string())),
            &sink,
        )
        .expect("unlock");
    // The CA backs the MITM leaves; without it issuance refuses (fail-closed).
    engine.ca_init(true, &sink).expect("ca_init apply");
    // The REAL key (SENTINEL) lives broker_only in the vault; the swap injects it into the upstream.
    engine
        .secret_put(
            SecretMeta {
                name: "anthropic_key".to_string(),
                provider: Provider::Anthropic,
                note: String::new(),
                broker_only: true,
            },
            Zeroizing::new(SENTINEL.to_vec()),
            &sink,
        )
        .expect("secret_put");
    // Mint a ProxyMitm bearer under the covering policy (persists the policy → host coverage).
    let owner_uid = rustix::process::getuid().as_raw();
    let bearer = engine
        .relay_mint(covering_policy(), 3600, Some(owner_uid), None, &sink)
        .expect("relay_mint");
    let raw_bearer = bearer.raw.to_string();

    // Materialize the CA pem the client must trust, BEFORE moving the engine into serve_proxy.
    let ca_pem = engine.ca_pem_path().expect("ca_pem_path");
    let client_cfg = Arc::new(client_config_trusting_only(&ca_pem));
    let connector = TlsConnector::from(client_cfg);

    // Serve the REAL proxy listener.
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (addr, server_handle) = envctl_secretd::proxy::serve_proxy(engine, owner_uid, async move {
        let _ = shutdown_rx.await;
    })
    .await
    .expect("serve_proxy binds loopback");

    // 1. CONNECT testhost:443 → 200 Connection Established.
    let (tcp, status) = connect_tunnel(addr, TEST_HOST).await;
    assert_eq!(status, 200, "CONNECT to a covered host must be 200");

    // 2. TLS handshake against a client trusting ONLY our CA — proves the minted leaf chains to it.
    let server_name = rustls::pki_types::ServerName::try_from(TEST_HOST).unwrap();
    let mut tls = connector
        .connect(server_name, tcp)
        .await
        .expect("TLS handshake against the MITM leaf must validate via our CA");

    // 3. Send a plaintext request carrying ONLY the bearer; assert the upstream's body comes back.
    let resp = send_plaintext_request(&mut tls, TEST_HOST, &raw_bearer).await;
    assert!(
        resp.windows(UPSTREAM_BODY.len())
            .any(|w| w == UPSTREAM_BODY),
        "the response body must be the upstream's known body"
    );
    assert!(
        String::from_utf8_lossy(&resp).contains("200"),
        "the response status must be 200"
    );

    // 4. The recording upstream received the REAL key (the swap happened): the key handed to
    //    `Upstream::send` is the SENTINEL, NOT the bearer the child presented. (Stripping the
    //    inbound auth header from the FORWARDED request is `DaemonUpstream::send`'s job, exercised by
    //    the plain-ingress swap test; the recording upstream here only inspects the swap's key.)
    assert_eq!(
        rec.seen_key.lock().unwrap().as_deref(),
        Some(SENTINEL),
        "the upstream must receive the REAL key (the swap happened)"
    );
    assert_ne!(
        rec.seen_key.lock().unwrap().as_deref(),
        Some(raw_bearer.as_bytes()),
        "the upstream must NOT receive the bearer as the key"
    );
    // The child carried ONLY the bearer in its auth header — confirm the ingress snapshot saw exactly
    // that bearer (and the engine swapped it for the real key before send).
    let headers = rec.seen_headers.lock().unwrap().clone();
    let header_blob = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        header_blob.contains(&raw_bearer),
        "the child's request must have carried the bearer through the MITM ingress"
    );

    // 5. A SECOND request on the SAME tunnel works (keep-alive).
    let resp2 = send_plaintext_request(&mut tls, TEST_HOST, &raw_bearer).await;
    assert!(
        resp2
            .windows(UPSTREAM_BODY.len())
            .any(|w| w == UPSTREAM_BODY),
        "a second request on the same tunnel must also succeed (keep-alive)"
    );

    // 5b. The REAL key (SENTINEL) NEVER appears in the bytes written back to the child over the
    //     terminated TLS — the one MITM-specific leak channel. The proxy only ever writes the
    //     upstream's `EgressResp` head + streamed body back, so a leak is structurally impossible;
    //     this asserts it affirmatively (both responses on the tunnel).
    for (label, bytes) in [("first", &resp), ("second", &resp2)] {
        assert!(
            !bytes.windows(SENTINEL.len()).any(|w| w == SENTINEL),
            "the REAL key must NEVER appear in the {label} response written back to the child"
        );
    }

    // 6. The REAL key (SENTINEL) NEVER appears in the emitted events. (The proxy drives `relay_swap`
    //    with an internal NULL sink — the swap's own durable audit lives in the engine's hash-chained
    //    log, which by construction records host/method/token_id but never key material; the engine's
    //    own tests prove that. Here we drain the setup sink, which carries init/unlock/ca/mint events,
    //    and assert the SENTINEL never leaked into any of them — the mint must not echo the real key.)
    let sentinel_str = String::from_utf8_lossy(SENTINEL);
    let events: Vec<SecretEvent> = rx.try_iter().collect();
    let event_blob = events
        .iter()
        .map(|e| format!("{e:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !event_blob.contains(sentinel_str.as_ref()),
        "the REAL key must never appear in any emitted event"
    );
    assert!(
        !raw_bearer.contains(sentinel_str.as_ref()),
        "the bearer must not be the real key"
    );

    // 7. A CONNECT for an UNCOVERED host is refused (bare 502) with NO MITM (no leaf, no TLS).
    let (_tcp2, status_uncovered) = connect_tunnel(addr, "evil.example.com").await;
    assert_eq!(
        status_uncovered, 502,
        "an uncovered host must be refused with a bare 502 (no leaf, no MITM)"
    );

    // teardown
    let _ = shutdown_tx.send(());
    server_handle.abort();
    let _ = std::fs::remove_dir_all(&root);
}
