//! PR-2a relay-proxy swap-boundary acceptance tests.
//!
//! The load-bearing test mirrors `crates/secrets-engine/tests/relay.rs`'s `CapturingUpstream`: it
//! builds an engine with a TEST `Upstream` (via `with_seams`) that captures exactly what it is
//! handed, mints a peer-bound bearer over a vault holding a known REAL_KEY, and drives a request
//! through the SAME `relay_swap` entry point the proxy's `handle` calls — asserting:
//!   * the captured upstream saw the REAL_KEY (the swap happened),
//!   * the BEARER never reached the upstream (only the real key did),
//!   * the REAL_KEY never appears in any emitted event.
//!
//! A second test drives the REAL proxy listener (`serve_proxy`) over loopback with a bogus bearer and
//! asserts a Denied swap yields a BARE 403 with an EMPTY body (fail-closed, no oracle) — this path
//! never fetches a key and never performs real DNS, so it is safe offline.
use std::sync::{Arc, Mutex};

use envctl_secrets::seam::{NoMint, SystemClock, UpstreamError, UsbProbe};
use envctl_secrets::vault::{InMemStore, Store};
use envctl_secrets::{
    EgressReq, EgressResp, Engine, EventSink, Method, Provider, RelayKind, RelayPolicy,
    SecretEvent, SecretMeta, SwapMode, SwapOutcome, Unlock, Upstream,
};
use zeroize::Zeroizing;

const REAL_KEY: &[u8] = b"sk-ant-REAL-PR2A-DEADBEEF";

/// A USB probe that never possesses a keyfile (so a passphrase-only vault's USB gate is vacuous).
struct AbsentUsb;
impl UsbProbe for AbsentUsb {
    fn keyfile_for(&self, _uuid: &str) -> Option<Zeroizing<Vec<u8>>> {
        None
    }
}

/// Captures the EXACT bytes handed to `send` (the real key) and the forwarded headers (to prove the
/// bearer never reaches the upstream). Returns a canned 200.
#[derive(Clone)]
struct CapturingUpstream {
    seen_key: Arc<Mutex<Option<Vec<u8>>>>,
    seen_headers: Arc<Mutex<Vec<(String, String)>>>,
}
#[async_trait::async_trait]
impl Upstream for CapturingUpstream {
    async fn send(
        &self,
        req: EgressReq,
        real_key: &Zeroizing<Vec<u8>>,
    ) -> Result<EgressResp, UpstreamError> {
        *self.seen_key.lock().unwrap() = Some(real_key.to_vec());
        *self.seen_headers.lock().unwrap() = req.headers.clone();
        Ok(EgressResp {
            status: 200,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            allowed: true,
        })
    }
}

fn paths() -> envctl_secrets::paths::Paths {
    envctl_secrets::paths::Paths::under(
        std::env::temp_dir().join(format!("envctl-proxy-pr2a-{}", std::process::id())),
    )
}

fn anthropic_policy() -> RelayPolicy {
    RelayPolicy {
        relay_id: "claude-main".to_string(),
        kind: RelayKind::Named,
        provider: Provider::Anthropic,
        secret_name: "anthropic_key".to_string(),
        swap: SwapMode::BaseUrlRepoint {
            upstream_base: "https://api.anthropic.com".to_string(),
        },
        host_allow: vec!["api.anthropic.com".to_string()],
        path_allow: vec!["/v1/".to_string()],
        method_allow: vec![Method::Post],
        policy_ttl_secs: 86_400,
        rate_per_min: Some(600),
        quota_total_requests: Some(1_000_000),
        quota_total_bytes: Some(1_000_000_000),
        enabled: true,
        revoked: false,
    }
}

fn post_req(peer_uid: Option<u32>) -> EgressReq {
    EgressReq {
        method: Method::Post,
        host: "api.anthropic.com".to_string(),
        path: "/v1/messages".to_string(),
        // The child would forward its (bearer-carrying) headers; the engine swap injects the real key
        // INSIDE Upstream::send, so the headers the upstream sees here must NOT contain the bearer.
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        bytes_out: 64,
        peer_uid,
        peer_pid: None,
    }
}

fn events_contain(events: &[SecretEvent], needle: &str) -> bool {
    events.iter().any(|e| {
        serde_json::to_string(e)
            .map(|s| s.contains(needle))
            .unwrap_or(false)
    })
}

/// The swap boundary: the proxy calls `relay_swap(bearer, egress, sink)`; the engine extracts the
/// REAL key and hands it (only) to `Upstream::send`. We assert the real key reached the upstream, the
/// bearer did NOT, and the real key never leaked into the event stream.
#[tokio::test]
async fn proxy_swap_delivers_real_key_only_and_bearer_never_leaks() {
    let cap = CapturingUpstream {
        seen_key: Arc::new(Mutex::new(None)),
        seen_headers: Arc::new(Mutex::new(Vec::new())),
    };
    let engine = Engine::with_seams(
        paths(),
        Box::new(InMemStore::new()) as Box<dyn Store>,
        Box::new(SystemClock),
        Box::new(AbsentUsb),
        Box::new(NoMint),
        Box::new(cap.clone()),
    )
    .expect("with_seams");

    let (sink, rx) = EventSink::channel();
    let fast = envctl_secrets::keyslot::Argon2Params {
        m_kib: envctl_secrets::keyslot::ARGON2_M_KIB_FLOOR,
        t_cost: envctl_secrets::keyslot::ARGON2_T_COST_FLOOR,
        p_lanes: 1,
    };
    engine
        .init_vault(
            Zeroizing::new("correct horse battery staple".to_string()),
            None,
            None,
            fast,
            &sink,
        )
        .expect("init_vault");
    engine
        .unlock(
            Unlock::Passphrase(Zeroizing::new("correct horse battery staple".to_string())),
            &sink,
        )
        .expect("unlock");
    engine
        .secret_put(
            SecretMeta {
                name: "anthropic_key".to_string(),
                provider: Provider::Anthropic,
                note: String::new(),
                broker_only: true,
            },
            Zeroizing::new(REAL_KEY.to_vec()),
            &sink,
        )
        .expect("secret_put");

    let bearer = engine
        .relay_mint(anthropic_policy(), 3600, Some(1000), None, &sink)
        .expect("relay_mint");
    let _ = rx.try_iter().collect::<Vec<_>>();

    // Drive the swap EXACTLY as the proxy `handle` does: relay_swap(bearer, egress, sink).
    let outcome = engine
        .relay_swap(&bearer.raw, &post_req(Some(1000)), &sink)
        .await;
    assert!(
        matches!(outcome, SwapOutcome::Allowed(ref r) if r.status == 200),
        "the swap must be Allowed with the upstream's 200"
    );

    // 1. The REAL key reached Upstream::send.
    assert_eq!(
        cap.seen_key.lock().unwrap().as_deref(),
        Some(REAL_KEY),
        "the real key must reach the upstream (the swap happened)"
    );

    // 2. The BEARER never reached the upstream — neither the raw bearer nor its token_id appears in
    //    the forwarded headers the upstream saw.
    let raw = bearer.raw.to_string();
    let headers = cap.seen_headers.lock().unwrap().clone();
    let header_blob = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !header_blob.contains(&raw),
        "the raw bearer must NOT reach the upstream"
    );
    assert!(
        !header_blob.contains(&bearer.token_id),
        "the bearer token_id must NOT reach the upstream"
    );

    // 3. The REAL key never leaked into the event stream.
    let real_str = String::from_utf8(REAL_KEY.to_vec()).unwrap();
    let events: Vec<SecretEvent> = rx.try_iter().collect();
    assert!(
        !events_contain(&events, &real_str),
        "the real key must not appear in any emitted event"
    );
    assert!(
        !events_contain(&events, &raw),
        "the raw bearer must not appear in any emitted event"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SecretEvent::RelaySwapped { allowed: true, .. })),
        "a RelaySwapped{{allowed:true}} event must be emitted"
    );
}

/// The REAL proxy listener: a request carrying a FORGED bearer (valid token_id, wrong secret) is
/// DENIED (`UnknownBearer`), and the proxy returns a BARE 403 with an EMPTY body (fail-closed; no
/// oracle). A deny never fetches a key and never does real DNS, so this is safe offline. It exercises
/// `serve_proxy` + `handle` end to end (bind, accept, extract bearer, relay_swap, map outcome).
#[tokio::test]
async fn proxy_denies_forged_bearer_with_bare_403() {
    let cap = CapturingUpstream {
        seen_key: Arc::new(Mutex::new(None)),
        seen_headers: Arc::new(Mutex::new(Vec::new())),
    };
    let engine = Engine::with_seams(
        paths(),
        Box::new(InMemStore::new()) as Box<dyn Store>,
        Box::new(SystemClock),
        Box::new(AbsentUsb),
        Box::new(NoMint),
        Box::new(cap.clone()),
    )
    .expect("with_seams");

    // Unlock a vault with the real secret + a minted bearer so we can FORGE its secret (a deny that
    // reaches decide()? no — a forged secret fails the bearer MAC => UnknownBearer, no key fetch).
    let sink = EventSink::null();
    let fast = envctl_secrets::keyslot::Argon2Params {
        m_kib: envctl_secrets::keyslot::ARGON2_M_KIB_FLOOR,
        t_cost: envctl_secrets::keyslot::ARGON2_T_COST_FLOOR,
        p_lanes: 1,
    };
    engine
        .init_vault(Zeroizing::new("pw".to_string()), None, None, fast, &sink)
        .expect("init_vault");
    engine
        .unlock(Unlock::Passphrase(Zeroizing::new("pw".to_string())), &sink)
        .expect("unlock");
    engine
        .secret_put(
            SecretMeta {
                name: "anthropic_key".to_string(),
                provider: Provider::Anthropic,
                note: String::new(),
                broker_only: true,
            },
            Zeroizing::new(REAL_KEY.to_vec()),
            &sink,
        )
        .expect("secret_put");
    let bearer = engine
        .relay_mint(
            anthropic_policy(),
            3600,
            Some(rustix::process::getuid().as_raw()),
            None,
            &sink,
        )
        .expect("mint");
    // Forge: real token_id, attacker-chosen secret => MAC verify fails => UnknownBearer.
    let forged = format!(
        "evrelay_{}_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        bearer.token_id
    );

    let owner_uid = rustix::process::getuid().as_raw();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (addr, handle) = envctl_secretd::proxy::serve_proxy(engine, owner_uid, async move {
        let _ = shutdown_rx.await;
    })
    .await
    .expect("serve_proxy binds loopback");
    assert!(addr.ip().is_loopback(), "proxy must bind loopback only");

    // Hand-roll a minimal HTTP/1.1 request carrying the forged bearer in the Anthropic x-api-key
    // header over the loopback TCP socket (no TLS — the proxy listener is plaintext loopback).
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let req = format!(
        "POST /v1/messages HTTP/1.1\r\nHost: api.anthropic.com\r\nx-api-key: {forged}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await.expect("write");
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).await.expect("read");
    let text = String::from_utf8_lossy(&resp);

    assert!(
        text.starts_with("HTTP/1.1 403"),
        "a forged bearer must yield a bare 403, got: {}",
        text.lines().next().unwrap_or_default()
    );
    // Bare: the response carries NO body (empty). Split headers/body on the blank line.
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(
        body.is_empty(),
        "a refusal must have an empty body (no oracle), got body: {body:?}"
    );
    // The forged bearer never reached the upstream (no key fetched).
    assert!(
        cap.seen_key.lock().unwrap().is_none(),
        "a forged bearer must NOT reach the upstream"
    );

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
