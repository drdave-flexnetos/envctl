//! The relay data-plane proxy: a loopback hyper server that swaps an ephemeral bearer for the real
//! provider key WITHOUT the real key ever reaching the child, the proxy logs, or any error string.
//!
//! A child process (spawned by `env-ctl run`, wired in PR-2b) is pointed at this listener via a
//! base-URL repoint. It sends an ordinary HTTPS-shaped request carrying ONLY the ephemeral bearer in
//! the provider auth header. [`handle`] extracts that bearer, builds an [`EgressReq`], and calls the
//! engine's [`Engine::relay_swap`], which verifies/decides and — only on an `Allowed` outcome —
//! extracts the REAL key from the unlocked vault and hands it to [`DaemonUpstream::send`]. The real
//! key is confined entirely to `send`: it is injected into the upstream auth header, sent over a TLS
//! client whose roots are seeded ONLY from `webpki_roots::TLS_SERVER_ROOTS` (FS-S7), and never
//! logged, never put in an event, never returned, never placed in an error string.
//!
//! Response body flow ("Option C"): the engine's `EgressResp` carries status + headers only, so the
//! upstream body is streamed out-of-band. Each request constructs a FRESH `DaemonUpstream` carrying
//! a per-request body sink (`tokio::sync::mpsc::Sender<Bytes>`); `send` reads the upstream response
//! head, then spawns a detached pump that forwards body chunks into that sink while it returns the
//! head to the proxy. [`handle`] wraps the matching receiver in a `StreamBody` for the hyper
//! response, so the body streams to the child concurrently — bounded by the channel capacity.
//!
//! PR-2a scope: the BaseUrlRepoint egress path. `CONNECT`/MITM (`HTTPS_PROXY`) is answered `501`
//! and deferred to PR-3 (it needs the local CA stack).
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use envctl_secrets::seam::UpstreamError;
use envctl_secrets::{EgressReq, EgressResp, Engine, EventSink, Method, Provider, SwapOutcome};
use http_body_util::{BodyExt, Empty, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tonic::codegen::tokio_stream::wrappers::ReceiverStream;
use zeroize::Zeroizing;

/// Max buffered body chunks between `send`'s upstream pump and the proxy's downstream writer. A
/// bounded channel keeps memory flat under a slow client (backpressure) without spilling.
const BODY_CHANNEL_CAP: usize = 16;

/// The hyper response body type the proxy serves: an out-of-band stream of upstream chunks, or an
/// empty body for refusals / non-streamed paths. `Infallible` data error — the upstream errors are
/// surfaced by simply ending the stream, never by leaking a key-bearing message.
type ProxyBody = http_body_util::Either<
    StreamBody<ReceiverStream<Result<Frame<Bytes>, Infallible>>>,
    Empty<Bytes>,
>;

fn stream_body(rx: tokio::sync::mpsc::Receiver<Result<Frame<Bytes>, Infallible>>) -> ProxyBody {
    http_body_util::Either::Left(StreamBody::new(ReceiverStream::new(rx)))
}

fn empty_body() -> ProxyBody {
    http_body_util::Either::Right(Empty::new())
}

// ============================================================================================
// upstream TLS client (FS-S7: webpki-roots ONLY)
// ============================================================================================

/// Build the rustls `ClientConfig` for the upstream egress TLS, seeding the `RootCertStore` from
/// `webpki_roots::TLS_SERVER_ROOTS` **ONLY** (FS-S7 / CF-6). NEVER the OS / native root store, NEVER
/// the local MITM CA, and NEVER any accept-invalid-certs escape hatch. The ring `CryptoProvider` is
/// selected explicitly so trust does not depend on process-default install order (the daemon installs
/// ring at startup, but being explicit keeps this correct in isolation, e.g. the unit test).
pub fn upstream_tls_config() -> rustls::ClientConfig {
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring provider supports the safe default protocol versions")
        .with_root_certificates(upstream_root_store())
        .with_no_client_auth()
}

/// The upstream trust anchors: EXACTLY `webpki_roots::TLS_SERVER_ROOTS`, nothing else (FS-S7). Factored
/// out so the unit test can assert the anchor set is precisely the frozen webpki set — the
/// `ClientConfig` does not expose its root store after building, so we assert at the source.
fn upstream_root_store() -> rustls::RootCertStore {
    rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    }
}

/// Construct the upstream reqwest client ONCE at startup with the frozen webpki-roots TLS config
/// (`use_preconfigured_tls`). Cheaply `Arc`-cloned per request thereafter.
fn build_upstream_client() -> reqwest::Client {
    reqwest::ClientBuilder::new()
        .use_preconfigured_tls(upstream_tls_config())
        .build()
        .expect("reqwest client with preconfigured webpki-roots TLS must build")
}

// ============================================================================================
// provider auth-header mapping
// ============================================================================================

/// The provider's auth header NAME + a closure of whether the value is prefixed with `Bearer `.
/// Anthropic carries the key bare in `x-api-key`; OpenAI/Generic/GitHub use `Authorization: Bearer`.
/// This is the SINGLE source of truth for both directions: `handle` reads the BEARER out of this
/// header, and `DaemonUpstream::send` injects the REAL key into the SAME header for the upstream.
struct AuthHeader {
    name: &'static str,
    bearer_scheme: bool,
}

fn auth_header_for(provider: Provider) -> AuthHeader {
    match provider {
        Provider::Anthropic => AuthHeader {
            name: "x-api-key",
            bearer_scheme: false,
        },
        Provider::Openai | Provider::Github | Provider::Generic => AuthHeader {
            name: "authorization",
            bearer_scheme: true,
        },
    }
}

/// Map a verified upstream host back to its provider, so `handle` reads the bearer from the right
/// header and `send` injects the real key into the right header. Mirrors the engine's
/// `canonical_upstreams` allowlist (the engine re-checks the host fence, so an unknown host here is
/// harmless — it simply yields a `Generic` header guess that the engine then refuses).
fn provider_for_host(host: &str) -> Provider {
    if host.eq_ignore_ascii_case("api.anthropic.com") {
        Provider::Anthropic
    } else if host.eq_ignore_ascii_case("api.openai.com") {
        Provider::Openai
    } else if host.eq_ignore_ascii_case("api.github.com")
        || host.eq_ignore_ascii_case("uploads.github.com")
    {
        Provider::Github
    } else {
        Provider::Generic
    }
}

/// Extract the raw bearer string from the inbound request's provider auth header, stripping a
/// `Bearer ` scheme prefix when the provider uses one. `None` when the header is absent/garbled — the
/// engine then refuses (`UnknownBearer`); the real key is never fetched.
fn extract_bearer(headers: &hyper::HeaderMap, provider: Provider) -> Option<String> {
    let spec = auth_header_for(provider);
    let raw = headers.get(spec.name)?.to_str().ok()?.trim();
    let token = if spec.bearer_scheme {
        raw.strip_prefix("Bearer ")
            .or_else(|| raw.strip_prefix("bearer "))
            .unwrap_or(raw)
    } else {
        raw
    };
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn method_from_hyper(m: &hyper::Method) -> Option<Method> {
    Some(match *m {
        hyper::Method::GET => Method::Get,
        hyper::Method::HEAD => Method::Head,
        hyper::Method::POST => Method::Post,
        hyper::Method::PUT => Method::Put,
        hyper::Method::PATCH => Method::Patch,
        hyper::Method::DELETE => Method::Delete,
        hyper::Method::OPTIONS => Method::Options,
        hyper::Method::CONNECT => Method::Connect,
        _ => return None,
    })
}

fn reqwest_method(m: Method) -> reqwest::Method {
    match m {
        Method::Get => reqwest::Method::GET,
        Method::Head => reqwest::Method::HEAD,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        Method::Options => reqwest::Method::OPTIONS,
        Method::Connect => reqwest::Method::CONNECT,
    }
}

/// Headers that must NOT be forwarded verbatim to the upstream: the inbound auth header (we inject
/// the REAL key in its place) and hop-by-hop / host headers reqwest sets itself.
fn is_droppable_request_header(name: &str, auth_name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == auth_name
        || lower == "host"
        || lower == "content-length"
        || lower == "connection"
        || lower == "proxy-connection"
        || lower == "transfer-encoding"
        || lower == "keep-alive"
        || lower == "upgrade"
}

// ============================================================================================
// DaemonUpstream — the engine `Upstream` seam (the ONLY place the real key lives)
// ============================================================================================

/// The per-request egress context the proxy hands to the engine seam through a task-local. It
/// carries the verified provider (which auth header to inject the real key into) and the body sink
/// (where `send` pumps the upstream response body so the proxy can stream it to the child). It is set
/// by [`handle`] immediately before awaiting `relay_swap`; the engine's `relay_swap` awaits
/// `Upstream::send` IN THE SAME TASK, so `send` reads exactly this request's context — no engine API
/// change, and the long-lived seam stays a single shared object.
struct RequestEgress {
    provider: Provider,
    body_tx: tokio::sync::mpsc::Sender<Result<Frame<Bytes>, Infallible>>,
}

tokio::task_local! {
    static EGRESS_CTX: std::cell::RefCell<Option<RequestEgress>>;
}

/// The long-lived engine `Upstream` seam installed at daemon startup. It holds ONLY the shared
/// reqwest client (webpki-roots TLS); the per-request provider + body sink come from the
/// `EGRESS_CTX` task-local set by the handling task. The engine calls `send` ONLY on an `Allowed`
/// outcome, handing over the real key by reference; `send` injects it into the provider auth header
/// and never lets it escape.
pub struct DaemonUpstream {
    client: reqwest::Client,
}

impl DaemonUpstream {
    /// Construct the seam with the frozen webpki-roots TLS client built ONCE at startup.
    pub fn new() -> Self {
        DaemonUpstream {
            client: build_upstream_client(),
        }
    }
}

impl Default for DaemonUpstream {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl envctl_secrets::Upstream for DaemonUpstream {
    async fn send(
        &self,
        req: EgressReq,
        real_key: &Zeroizing<Vec<u8>>,
    ) -> Result<EgressResp, UpstreamError> {
        // Pull THIS request's provider + body sink from the task-local. A missing context is a
        // misuse (send reached without handle priming the task-local) — refuse with a fixed string.
        let (provider, body_tx) = EGRESS_CTX
            .try_with(|c| c.borrow_mut().take().map(|e| (e.provider, e.body_tx)))
            .ok()
            .flatten()
            .ok_or_else(|| UpstreamError::Io("upstream context missing".to_string()))?;
        let spec = auth_header_for(provider);
        // Build the auth header VALUE from the real key. This `Zeroizing<String>` and the request
        // are the only places the real key ever materializes; both are dropped at the end of `send`.
        let key_str = String::from_utf8_lossy(real_key);
        let auth_value: Zeroizing<String> = Zeroizing::new(if spec.bearer_scheme {
            format!("Bearer {key_str}")
        } else {
            key_str.into_owned()
        });

        let url = format!("https://{}{}", req.host, req.path);
        let mut builder = self
            .client
            .request(reqwest_method(req.method), &url)
            .header(spec.name, auth_value.as_str());
        // Forward the child's headers EXCEPT the auth header (real key replaces the bearer) and the
        // hop-by-hop/host headers reqwest manages.
        for (k, v) in &req.headers {
            if !is_droppable_request_header(k, spec.name) {
                builder = builder.header(k, v);
            }
        }

        // SEND. On ANY reqwest error, return a FIXED key-free string — never the URL (no creds in it,
        // but the host is uninteresting) and never the error's own text (a hostile/buggy adapter
        // could echo the auth header). The engine maps this to a 502 with no detail.
        let resp = self
            .client
            .execute(
                builder
                    .build()
                    .map_err(|_| UpstreamError::Io("upstream request build failed".to_string()))?,
            )
            .await
            .map_err(|_| UpstreamError::Io("upstream send failed".to_string()))?;

        let status = resp.status().as_u16();
        // Capture the response headers for the EgressResp head BEFORE the body is consumed.
        let mut out_headers: Vec<(String, String)> = Vec::new();
        for (k, v) in resp.headers().iter() {
            if let Ok(val) = v.to_str() {
                out_headers.push((k.as_str().to_string(), val.to_string()));
            }
        }

        // Spawn a detached pump that forwards the upstream body to the per-request sink while we
        // return the head to the proxy. The real key is NOT captured by this task (it lives only in
        // the already-consumed request); only opaque body bytes flow here.
        tokio::spawn(async move {
            let mut resp = resp;
            let tx = body_tx;
            loop {
                match resp.chunk().await {
                    Ok(Some(chunk)) => {
                        if tx.send(Ok(Frame::data(chunk))).await.is_err() {
                            break; // client hung up; stop pumping.
                        }
                    }
                    Ok(None) => break, // body complete.
                    Err(_) => break,   // upstream body error: end the stream (no key in scope).
                }
            }
            // tx drops here -> the ReceiverStream ends -> the hyper response body completes.
        });

        Ok(EgressResp {
            status,
            headers: out_headers,
            allowed: true,
        })
    }
}

// ============================================================================================
// request handling
// ============================================================================================

/// Per-connection context shared by every request on a loopback connection. The engine already
/// carries the long-lived `DaemonUpstream` seam (installed at startup), so the proxy holds only the
/// engine handle + the loopback peer uid; the per-request provider/body sink ride the task-local.
#[derive(Clone)]
struct ProxyCtx {
    engine: Engine,
    peer_uid: Option<u32>,
}

/// Handle one inbound proxy request: extract the bearer, build the `EgressReq`, drive `relay_swap`,
/// and map the `SwapOutcome` to a hyper response. NEVER echoes a body or header on a refusal (no
/// oracle); NEVER logs request/response bodies or the auth header.
async fn handle(ctx: ProxyCtx, req: Request<Incoming>) -> Result<Response<ProxyBody>, Infallible> {
    // CONNECT / MITM is deferred to PR-3 (needs the local CA): answer 501, swap nothing.
    if req.method() == hyper::Method::CONNECT {
        return Ok(bare(StatusCode::NOT_IMPLEMENTED));
    }

    // Determine the target host. For a base-URL repoint the child sends an origin-form request whose
    // Host header names the verified provider host; an absolute-form URI (forward-proxy style) names
    // it in the URI authority. Prefer the URI authority, else the Host header.
    let method = match method_from_hyper(req.method()) {
        Some(m) => m,
        None => return Ok(bare(StatusCode::METHOD_NOT_ALLOWED)),
    };
    let host = match request_host(&req) {
        Some(h) => h,
        None => return Ok(bare(StatusCode::BAD_REQUEST)),
    };
    let path = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());

    let provider = provider_for_host(&host);
    let bearer = match extract_bearer(req.headers(), provider) {
        Some(b) => b,
        // No bearer => the engine would refuse as UnknownBearer; short-circuit to a bare 403 without
        // touching the vault.
        None => return Ok(bare(StatusCode::FORBIDDEN)),
    };

    // Snapshot the forwarded request headers (the auth header is dropped inside `send`).
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|val| (k.as_str().to_string(), val.to_string()))
        })
        .collect();

    // Drain the inbound body (the swap is for control-plane-sized requests; collect bounds it). The
    // bytes_out feeds the engine's byte-budget quota. A body read error => bad request.
    let body_bytes = match req.into_body().collect().await {
        Ok(c) => c.to_bytes(),
        Err(_) => return Ok(bare(StatusCode::BAD_REQUEST)),
    };
    let bytes_out = body_bytes.len() as u64;

    let egress = EgressReq {
        method,
        host: host.clone(),
        path,
        headers,
        bytes_out,
        peer_uid: ctx.peer_uid,
        // The proxy does not (in PR-2a) resolve the child pid per request; the bearer is uid-bound at
        // mint and re-checked here by uid. pid binding is advisory and wired in PR-2b.
        peer_pid: None,
    };

    // Per-request body channel. Its sender goes into the task-local egress context that the
    // long-lived `DaemonUpstream` seam reads inside `send`; its receiver becomes the response body.
    let (body_tx, body_rx) =
        tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(BODY_CHANNEL_CAP);
    let egress_ctx = std::cell::RefCell::new(Some(RequestEgress { provider, body_tx }));

    // Drive the swap. The engine owns key extraction + the decision; we never re-implement either.
    // A null sink: the daemon's durable audit happens inside the engine; the proxy emits nothing
    // here (no secrets in logs). The task-local is scoped to JUST this swap await, so each concurrent
    // request reads its own provider + body sink.
    let sink = EventSink::null();
    let bearer = Zeroizing::new(bearer);
    let outcome = EGRESS_CTX
        .scope(egress_ctx, async {
            ctx.engine.relay_swap(&bearer, &egress, &sink).await
        })
        .await;

    match outcome {
        SwapOutcome::Allowed(resp) => {
            let mut builder = Response::builder()
                .status(StatusCode::from_u16(resp.status).unwrap_or(StatusCode::BAD_GATEWAY));
            for (k, v) in &resp.headers {
                if !is_droppable_response_header(k) {
                    builder = builder.header(k, v);
                }
            }
            Ok(builder
                .body(stream_body(body_rx))
                .unwrap_or_else(|_| bare(StatusCode::BAD_GATEWAY)))
        }
        // Fail-closed: a deny or an internal refusal is a BARE status with NO body and NO header echo
        // (no oracle, no key/error leak). The engine already fetched no key on a deny.
        SwapOutcome::Denied(_) => Ok(bare(StatusCode::FORBIDDEN)),
        SwapOutcome::InternalRefused(_) => Ok(bare(StatusCode::BAD_GATEWAY)),
    }
}

/// Response hop-by-hop headers reqwest/hyper manage; drop them when re-emitting the upstream head.
fn is_droppable_response_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "connection"
        || lower == "transfer-encoding"
        || lower == "keep-alive"
        || lower == "content-length"
        || lower == "upgrade"
}

/// A bare response: the given status, an EMPTY body, no headers. Used for every refusal and every
/// deferred/unsupported path so the proxy never leaks an oracle.
fn bare(status: StatusCode) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .body(empty_body())
        .expect("bare response with empty body is always constructible")
}

/// The verified target host: the URI authority (absolute-form) if present, else the `Host` header.
/// Strips any `:port` suffix.
fn request_host(req: &Request<Incoming>) -> Option<String> {
    let raw = req
        .uri()
        .authority()
        .map(|a| a.as_str().to_string())
        .or_else(|| {
            req.headers()
                .get(hyper::header::HOST)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })?;
    let host = raw.split('@').next_back().unwrap_or(&raw);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

// ============================================================================================
// serve
// ============================================================================================

/// Bind the relay proxy on an ephemeral loopback port (`127.0.0.1:0`) and serve it with the
/// `hyper_util` auto (HTTP/1+2) connection builder until `shutdown` resolves. Returns the bound
/// `SocketAddr` so `main` can publish `127.0.0.1:<port>` into `DaemonState` for PR-2b's Mint.
///
/// Defense in depth: the listener is loopback-only by bind, and every accepted connection's peer is
/// re-checked to be loopback before it is served (a non-loopback peer is dropped without a response).
pub async fn serve_proxy(
    engine: Engine,
    owner_uid: u32,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let local_addr = listener.local_addr()?;

    let handle = tokio::spawn(async move {
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("relay proxy shutting down");
                    break;
                }
                accept = listener.accept() => {
                    let (stream, peer) = match accept {
                        Ok(pair) => pair,
                        Err(e) => {
                            tracing::warn!(error = %e, "relay proxy accept failed");
                            continue;
                        }
                    };
                    // Defense in depth: refuse any non-loopback peer outright.
                    if !peer.ip().is_loopback() {
                        tracing::warn!(peer = %peer, "relay proxy rejected non-loopback peer");
                        continue;
                    }
                    let ctx = ProxyCtx {
                        engine: engine.clone(),
                        // The loopback peer's uid is the owner (the daemon is owner-only); the bearer
                        // is uid-bound at mint and re-checked by the engine. SO_PEERCRED on the TCP
                        // loopback socket is not readily available here, so we pass the daemon owner
                        // uid (the only principal that can reach a 0600-gated owner-only daemon).
                        peer_uid: Some(owner_uid),
                    };
                    let io = TokioIo::new(stream);
                    tokio::spawn(async move {
                        let service = service_fn(move |req| handle(ctx.clone(), req));
                        if let Err(e) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                            .serve_connection(io, service)
                            .await
                        {
                            tracing::debug!(error = %e, "relay proxy connection ended");
                        }
                    });
                }
            }
        }
    });

    Ok((local_addr, handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::body::Body as _;

    #[test]
    fn auth_header_anthropic_is_x_api_key_bare() {
        let h = auth_header_for(Provider::Anthropic);
        assert_eq!(h.name, "x-api-key");
        assert!(!h.bearer_scheme, "anthropic carries the key bare");
    }

    #[test]
    fn auth_header_openai_generic_github_is_bearer() {
        for p in [Provider::Openai, Provider::Generic, Provider::Github] {
            let h = auth_header_for(p);
            assert_eq!(h.name, "authorization");
            assert!(h.bearer_scheme, "{p:?} uses Authorization: Bearer");
        }
    }

    #[test]
    fn tls_roots_are_exactly_webpki_no_native() {
        // FS-S7: the upstream trust anchors are EXACTLY the frozen webpki-roots set — no native/OS
        // roots are merged in. The count equality proves nothing else was added.
        let store = upstream_root_store();
        assert_eq!(
            store.roots.len(),
            webpki_roots::TLS_SERVER_ROOTS.len(),
            "upstream roots must equal webpki-roots exactly (no native roots)"
        );
        assert!(
            !store.is_empty(),
            "webpki-roots must seed a non-empty trust store"
        );
        // The config must still build atop these roots (proves the ring provider + webpki roots
        // compose into a usable client config — no native-roots feature is needed or used).
        let _cfg = upstream_tls_config();
    }

    #[test]
    fn provider_for_host_maps_canonical_hosts() {
        assert_eq!(provider_for_host("api.anthropic.com"), Provider::Anthropic);
        assert_eq!(provider_for_host("api.openai.com"), Provider::Openai);
        assert_eq!(provider_for_host("api.github.com"), Provider::Github);
        assert_eq!(provider_for_host("evil.example.com"), Provider::Generic);
    }

    #[test]
    fn extract_bearer_strips_bearer_scheme_for_openai() {
        let mut h = hyper::HeaderMap::new();
        h.insert("authorization", "Bearer evrelay_abc_def".parse().unwrap());
        assert_eq!(
            extract_bearer(&h, Provider::Openai).as_deref(),
            Some("evrelay_abc_def")
        );
    }

    #[test]
    fn extract_bearer_reads_x_api_key_bare_for_anthropic() {
        let mut h = hyper::HeaderMap::new();
        h.insert("x-api-key", "evrelay_abc_def".parse().unwrap());
        assert_eq!(
            extract_bearer(&h, Provider::Anthropic).as_deref(),
            Some("evrelay_abc_def")
        );
    }

    #[test]
    fn extract_bearer_absent_is_none() {
        let h = hyper::HeaderMap::new();
        assert!(extract_bearer(&h, Provider::Anthropic).is_none());
    }

    #[test]
    fn bare_refusal_has_empty_body_and_status() {
        // A Denied/InternalRefused maps to a bare status with an EMPTY body (no oracle).
        let denied = bare(StatusCode::FORBIDDEN);
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            denied.body().size_hint().exact(),
            Some(0),
            "a refusal body must be empty"
        );
        let refused = bare(StatusCode::BAD_GATEWAY);
        assert_eq!(refused.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(refused.body().size_hint().exact(), Some(0));
    }
}
