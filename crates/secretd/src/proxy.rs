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
//! PR-2a scope: the BaseUrlRepoint egress path (origin/absolute-form requests on a plain loopback
//! connection). PR-3b adds the `CONNECT`/MITM (`HTTPS_PROXY`) ingress (feature `mitm-ca`): the proxy
//! terminates the child's TLS with an engine-minted per-host leaf, reads the plaintext request, and
//! reuses the SAME `relay_swap` egress. The MITM path changes only the INGRESS; everything after the
//! bearer is read (`DaemonUpstream`, the per-request body channel, the task-local) is unchanged.
//! With `--no-default-features` (no `mitm-ca`) the CONNECT branch falls back to the historical `501`.
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
        // The daemon sets `HTTPS_PROXY` in CHILD environments (the MITM injection). `.no_proxy()`
        // ensures the daemon's OWN upstream client never honors an ambient `HTTPS_PROXY`/`http_proxy`
        // from its own environment — otherwise the real-key egress could be looped back through the
        // very proxy it published for children (an egress-hijack / SSRF foothold). Fail-safe: the
        // upstream always goes DIRECT to the provider over the frozen webpki-roots TLS.
        .no_proxy()
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

/// Test-only hook: from inside a test `Upstream::send` (which runs in the same task as the proxy's
/// `EGRESS_CTX.scope`), pump a complete response body into THIS request's body sink so the proxy
/// streams it back to the client — exactly as `DaemonUpstream::send`'s detached pump does for a real
/// upstream. Returns `false` if no per-request context is present (misuse). This lets an integration
/// test exercise the full ingress→swap→streamed-response path with a recording upstream, WITHOUT
/// making a real network call. It carries no key material and is inert in production (never called).
#[doc(hidden)]
pub async fn __test_pump_response_body(body: Bytes) -> bool {
    let tx = EGRESS_CTX
        .try_with(|c| c.borrow_mut().take().map(|e| e.body_tx))
        .ok()
        .flatten();
    let Some(tx) = tx else {
        return false;
    };
    if !body.is_empty() {
        let _ = tx.send(Ok(Frame::data(body))).await;
    }
    // Dropping `tx` ends the ReceiverStream → the hyper response body completes.
    true
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

/// Handle one inbound proxy request on a PLAIN loopback connection (the BaseUrlRepoint ingress).
/// `CONNECT` is dispatched to the MITM ingress (feature `mitm-ca`) or the historical `501` fallback;
/// every other method is an origin/absolute-form request resolved here. NEVER echoes a body or
/// header on a refusal (no oracle); NEVER logs request/response bodies or the auth header.
async fn handle(ctx: ProxyCtx, req: Request<Incoming>) -> Result<Response<ProxyBody>, Infallible> {
    // CONNECT establishes an HTTPS_PROXY tunnel: terminate the child's TLS with an engine-minted leaf
    // (PR-3b) and serve the decrypted requests through the SAME relay_swap. When the `mitm-ca` feature
    // is off there is no local CA, so we fall back to the historical 501 (swap nothing).
    if req.method() == hyper::Method::CONNECT {
        #[cfg(feature = "mitm-ca")]
        {
            return Ok(mitm::handle_connect(ctx, req).await);
        }
        #[cfg(not(feature = "mitm-ca"))]
        {
            return Ok(bare(StatusCode::NOT_IMPLEMENTED));
        }
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

    // Plain ingress: no TLS is terminated here, so there is no observed SNI (`None`). The non-MITM
    // swap modes ignore SNI; a `ProxyMitm` policy presented over this plain path fails closed.
    let (parts, body) = req.into_parts();
    Ok(swap_and_respond(&ctx, method, host, path, &parts.headers, body, None).await)
}

/// The shared egress core: from a verified-host request (plain OR MITM-decrypted), extract the
/// bearer, build the `EgressReq`, drive `relay_swap`, and map the `SwapOutcome` to a hyper response.
/// `observed_sni` is `Some(host)` for the MITM ingress (the TLS handshake name, pinned to the
/// CONNECT target) and `None` for the plain ingress. The real key never enters this function — it is
/// confined entirely to the engine's `Upstream::send`.
async fn swap_and_respond<B>(
    ctx: &ProxyCtx,
    method: Method,
    host: String,
    path: String,
    headers_in: &hyper::HeaderMap,
    body: B,
    observed_sni: Option<String>,
) -> Response<ProxyBody>
where
    B: hyper::body::Body<Data = Bytes>,
{
    let provider = provider_for_host(&host);
    let bearer = match extract_bearer(headers_in, provider) {
        Some(b) => b,
        // No bearer => the engine would refuse as UnknownBearer; short-circuit to a bare 403 without
        // touching the vault.
        None => return bare(StatusCode::FORBIDDEN),
    };

    // Snapshot the forwarded request headers (the auth header is dropped inside `send`).
    let headers: Vec<(String, String)> = headers_in
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|val| (k.as_str().to_string(), val.to_string()))
        })
        .collect();

    // Drain the inbound body (the swap is for control-plane-sized requests; collect bounds it). The
    // bytes_out feeds the engine's byte-budget quota. A body read error => bad request.
    let body_bytes = match body.collect().await {
        Ok(c) => c.to_bytes(),
        Err(_) => return bare(StatusCode::BAD_REQUEST),
    };
    let bytes_out = body_bytes.len() as u64;

    let egress = EgressReq {
        method,
        host: host.clone(),
        path,
        headers,
        bytes_out,
        peer_uid: ctx.peer_uid,
        // The proxy does not resolve the child pid per request; the bearer is uid-bound at mint and
        // re-checked here by uid. PR-2b deliberately mints uid-only (`client_pid = 0`): `decide`
        // checks pid only when bound, so a pid-bound bearer would `PeerMismatch`-deny every egress
        // here. Same-uid trust is the local boundary; the bearer stays short-lived + scoped + USB-gated.
        peer_pid: None,
        observed_sni,
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
            builder
                .body(stream_body(body_rx))
                .unwrap_or_else(|_| bare(StatusCode::BAD_GATEWAY))
        }
        // Fail-closed: a deny or an internal refusal is a BARE status with NO body and NO header echo
        // (no oracle, no key/error leak). The engine already fetched no key on a deny.
        SwapOutcome::Denied(_) => bare(StatusCode::FORBIDDEN),
        SwapOutcome::InternalRefused(_) => bare(StatusCode::BAD_GATEWAY),
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

/// Parse the bare host from a `CONNECT host[:port]` target (the request-target of a CONNECT is an
/// `authority`, e.g. `api.anthropic.com:443`). Accepts ANY port (OQ4: the per-request swap re-fences
/// the host against the provider allowlist regardless of the tunnel port), strips it to the host,
/// and lowercases nothing (the engine compares case-insensitively). `None` for an empty/garbled
/// target so the CONNECT is refused before any leaf is minted. Only the MITM ingress (feature
/// `mitm-ca`) parses CONNECT targets; gated so a non-MITM build has no dead code.
#[cfg(feature = "mitm-ca")]
fn connect_host_from_target(target: &str) -> Option<String> {
    // Strip a userinfo prefix defensively, then the `:port` suffix. IPv6 literals are not a real
    // upstream here (the canonical providers are DNS names), so the simple rsplit on ':' is safe.
    let host = target.split('@').next_back().unwrap_or(target);
    let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    let host = host.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

// ============================================================================================
// MITM ingress (HTTPS_PROXY / CONNECT) — feature `mitm-ca`
// ============================================================================================

/// The CONNECT/MITM ingress: terminate the child's TLS with an engine-minted per-host leaf and serve
/// the decrypted requests through the SAME `relay_swap`. The proxy holds ONLY the short-lived,
/// in-RAM leaf key for the CONNECT-target host; it never sees the CA key (sealed in the engine) nor
/// the real provider key (confined to `Upstream::send`). Fail-closed: an uncovered/locked host is
/// minted NO leaf and the tunnel is refused BEFORE any TLS handshake (no oracle, no half-MITM).
#[cfg(feature = "mitm-ca")]
mod mitm {
    use super::{bare, connect_host_from_target, swap_and_respond, ProxyBody, ProxyCtx};
    use envctl_secrets::EventSink;
    use hyper::body::Incoming;
    use hyper::service::service_fn;
    use hyper::{Request, Response, StatusCode};
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use std::sync::Arc;

    /// Per-host MITM cert resolver. Holds the CONNECT-target host plus the cached `CertifiedKey`
    /// built from the engine-minted leaf chain + ephemeral leaf key. Anti-fronting: if the inbound
    /// ClientHello carries a genuine SNI that differs from the CONNECT host, it returns `None` and the
    /// handshake fails — a child cannot CONNECT to one host and then TLS-front to another.
    ///
    /// NOTE: this type is the `ResolvesServerCert` impl and is deliberately named WITHOUT the
    /// `edge`/`relay_tls`/`inbound` tokens (shape FS-S25), since it lives in the MITM ingress and must
    /// not be confused with the (forbidden) edge/relay-TLS inbound termination surface.
    pub(super) struct MitmCertResolver {
        host: String,
        certified: Arc<rustls::sign::CertifiedKey>,
    }

    impl std::fmt::Debug for MitmCertResolver {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // Never print key material; the host is already public (it is the CONNECT target).
            f.debug_struct("MitmCertResolver")
                .field("host", &self.host)
                .finish_non_exhaustive()
        }
    }

    impl MitmCertResolver {
        /// Build the resolver from the engine's `issue_leaf_for_covered_host` output: the leaf chain
        /// (leaf-then-CA) and the ephemeral ECDSA leaf key (rcgen ring → PKCS#8). The key is turned
        /// into a ring `SigningKey` via `any_ecdsa_type`; a non-ECDSA key (should never happen for our
        /// rcgen ring leaves) is an `Err` so we fail closed rather than serve a broken cert.
        pub(super) fn new(
            host: String,
            chain: Vec<rustls::pki_types::CertificateDer<'static>>,
            key: rustls::pki_types::PrivateKeyDer<'static>,
        ) -> anyhow::Result<Self> {
            let signing_key = rustls::crypto::ring::sign::any_ecdsa_type(&key)
                .map_err(|e| anyhow::anyhow!("leaf key is not a usable ECDSA signing key: {e}"))?;
            let certified = Arc::new(rustls::sign::CertifiedKey::new(chain, signing_key));
            Ok(MitmCertResolver { host, certified })
        }
    }

    impl rustls::server::ResolvesServerCert for MitmCertResolver {
        fn resolve(
            &self,
            client_hello: rustls::server::ClientHello<'_>,
        ) -> Option<Arc<rustls::sign::CertifiedKey>> {
            // Anti-fronting: a genuine SNI that disagrees with the CONNECT target is refused (None →
            // the handshake fails with no cert). An absent SNI (some clients omit it on an IP/CONNECT
            // tunnel) is allowed against the CONNECT-target leaf — the egress swap re-fences the host.
            if let Some(sni) = client_hello.server_name() {
                if !sni.eq_ignore_ascii_case(&self.host) {
                    return None;
                }
            }
            Some(self.certified.clone())
        }
    }

    /// Handle a CONNECT: mint the per-host leaf (fail-closed), reply `200`, upgrade the tunnel,
    /// terminate the child's TLS, and serve the decrypted requests through `relay_swap` (keep-alive
    /// across multiple requests per tunnel). Returns the response sent to the child for the CONNECT
    /// itself (`200` to proceed, or a bare `502` refusal with NO upgrade).
    pub(super) async fn handle_connect(
        ctx: ProxyCtx,
        req: Request<Incoming>,
    ) -> Response<ProxyBody> {
        // 1. Parse the CONNECT target host (accept any port; strip to host). A garbled target is a
        //    bare 400 with no leaf, no upgrade.
        let target = req
            .uri()
            .authority()
            .map(|a| a.as_str().to_string())
            .or_else(|| req.uri().host().map(|h| h.to_string()));
        let host = match target.as_deref().and_then(connect_host_from_target) {
            Some(h) => h,
            None => return bare(StatusCode::BAD_REQUEST),
        };

        // 2. Mint the per-host leaf BEFORE any handshake. Fail-closed: an uncovered host / locked
        //    vault / absent CA yields NO leaf and a bare 502 with NO upgrade — the child's TLS is
        //    never terminated, so there is no MITM and no oracle. The mint side-effects (audit) live
        //    in the engine; the proxy passes a null sink (no secrets in logs).
        let sink = EventSink::null();
        let (chain, key) = match ctx.engine.issue_leaf_for_covered_host(&host, &sink) {
            Ok(pair) => pair,
            Err(_) => return bare(StatusCode::BAD_GATEWAY),
        };

        // 3. Build the per-connection rustls ServerConfig from the minted leaf via the anti-fronting
        //    resolver. ring provider + safe protocol versions, no client auth. A build failure (e.g.
        //    a non-ECDSA key) fails closed with a bare 502, no upgrade.
        let resolver = match MitmCertResolver::new(host.clone(), chain, key) {
            Ok(r) => r,
            Err(_) => return bare(StatusCode::BAD_GATEWAY),
        };
        let server_config = match rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        {
            Ok(b) => b
                .with_no_client_auth()
                .with_cert_resolver(Arc::new(resolver)),
            Err(_) => return bare(StatusCode::BAD_GATEWAY),
        };
        let server_config = Arc::new(server_config);

        // 4. Spawn the tunnel: after we return `200`, hyper hands us the raw upgraded stream; we
        //    terminate TLS on it and serve the decrypted requests. The CONNECT response itself is the
        //    empty-bodied 200; the upgrade future resolves once the child sees it.
        let connect_host = host;
        tokio::spawn(async move {
            let upgraded = match hyper::upgrade::on(req).await {
                Ok(u) => u,
                Err(e) => {
                    tracing::debug!(error = %e, "mitm CONNECT upgrade failed");
                    return;
                }
            };
            let acceptor = tokio_rustls::TlsAcceptor::from(server_config);
            let tls_stream = match acceptor.accept(TokioIo::new(upgraded)).await {
                Ok(s) => s,
                Err(e) => {
                    // A handshake failure (incl. the anti-fronting None → no cert) ends the tunnel
                    // with no plaintext ever read. No key material is in scope here.
                    tracing::debug!(error = %e, "mitm TLS handshake failed");
                    return;
                }
            };

            // Serve the decrypted requests with the auto (HTTP/1+2) builder so keep-alive / multiple
            // requests per tunnel work. The host is PINNED to the CONNECT target: the decrypted
            // requests arrive in origin-form, and we do NOT trust a divergent inner Host header.
            let svc_ctx = ctx.clone();
            let svc_host = connect_host.clone();
            let service =
                service_fn(move |dreq| handle_decrypted(svc_ctx.clone(), svc_host.clone(), dreq));
            if let Err(e) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(tls_stream), service)
                .await
            {
                tracing::debug!(error = %e, "mitm decrypted connection ended");
            }
        });

        // The 200 has no body; the upgrade carries the tunnel.
        Response::builder()
            .status(StatusCode::OK)
            .body(super::empty_body())
            .expect("200 Connection Established with empty body is always constructible")
    }

    /// Serve ONE decrypted request from inside a MITM tunnel. The host is pinned to the CONNECT
    /// target (`connect_host`), NOT read from the request (origin-form URIs + an untrusted inner Host
    /// must not redirect the egress). Everything after the host pin is the SHARED `swap_and_respond`
    /// core — same bearer extraction, `relay_swap`, body streaming, and fail-closed mapping as the
    /// plain ingress. `observed_sni` is `Some(connect_host)`: the handshake matched it (anti-fronting),
    /// so the engine's `ProxyMitm` SNI==Host check passes against a real, TLS-observed name.
    async fn handle_decrypted(
        ctx: ProxyCtx,
        connect_host: String,
        req: Request<Incoming>,
    ) -> Result<Response<ProxyBody>, std::convert::Infallible> {
        let method = match super::method_from_hyper(req.method()) {
            Some(m) => m,
            None => return Ok(bare(StatusCode::METHOD_NOT_ALLOWED)),
        };
        let path = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str().to_string())
            .unwrap_or_else(|| "/".to_string());
        let observed_sni = Some(connect_host.clone());
        let (parts, body) = req.into_parts();
        Ok(swap_and_respond(
            &ctx,
            method,
            connect_host,
            path,
            &parts.headers,
            body,
            observed_sni,
        )
        .await)
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
                        // `_with_upgrades` so a CONNECT handler can `hyper::upgrade::on(req)` the raw
                        // stream for the MITM TLS termination (PR-3b). Non-upgrading requests are
                        // served exactly as before; the historical 501 CONNECT path never upgrades.
                        if let Err(e) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                            .serve_connection_with_upgrades(io, service)
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

    // ---- PR-3b MITM ingress unit coverage ----------------------------------------------------

    #[cfg(feature = "mitm-ca")]
    #[test]
    fn connect_host_strips_port_and_userinfo() {
        // ANY port is accepted and stripped to the bare host (OQ4: the swap re-fences the host).
        assert_eq!(
            connect_host_from_target("api.anthropic.com:443").as_deref(),
            Some("api.anthropic.com")
        );
        assert_eq!(
            connect_host_from_target("api.openai.com:8443").as_deref(),
            Some("api.openai.com")
        );
        // No port: the whole authority is the host.
        assert_eq!(
            connect_host_from_target("api.github.com").as_deref(),
            Some("api.github.com")
        );
        // Defensive userinfo strip.
        assert_eq!(
            connect_host_from_target("user@host.example:443").as_deref(),
            Some("host.example")
        );
        // Empty / garbled → None (the CONNECT is refused before any leaf is minted).
        assert!(connect_host_from_target("").is_none());
        assert!(connect_host_from_target(":443").is_none());
    }

    #[cfg(feature = "mitm-ca")]
    mod mitm_unit {
        use super::super::mitm::MitmCertResolver;
        use envctl_secrets::broker::{Method, Provider, RelayKind};
        use envctl_secrets::seam::{NoMint, SystemClock, UpstreamError, UsbProbe};
        use envctl_secrets::vault::InMemStore;
        use envctl_secrets::{
            paths::Paths, EgressReq, EgressResp, Engine, EventSink, RelayPolicy, SwapMode, Unlock,
            Upstream,
        };
        use zeroize::Zeroizing;

        const USB_UUID: &str = "MITM-UNIT-USB";

        struct PresentUsb(Zeroizing<Vec<u8>>);
        impl UsbProbe for PresentUsb {
            fn keyfile_for(&self, uuid: &str) -> Option<Zeroizing<Vec<u8>>> {
                (uuid == USB_UUID).then(|| self.0.clone())
            }
        }

        struct NullUpstream;
        #[async_trait::async_trait]
        impl Upstream for NullUpstream {
            async fn send(
                &self,
                _req: EgressReq,
                _real_key: &Zeroizing<Vec<u8>>,
            ) -> Result<EgressResp, UpstreamError> {
                Err(UpstreamError::Io("not wired in mitm unit test".into()))
            }
        }

        /// An unlocked engine with a CA + a covering `ProxyMitm` policy for `host`, so
        /// `issue_leaf_for_covered_host(host)` mints a real leaf. Mirrors the e2e seam.
        fn covered_engine(host: &str) -> (Engine, EventSink) {
            let root = std::env::temp_dir().join(format!(
                "envctl-mitm-unit-{}-{}",
                std::process::id(),
                host.replace('.', "_")
            ));
            let _ = std::fs::remove_dir_all(&root);
            let paths = Paths::under(root);
            std::fs::create_dir_all(&paths.runtime).unwrap();
            let keyfile = Zeroizing::new(vec![0x42u8; 64]);
            let engine = Engine::with_seams(
                paths,
                Box::new(InMemStore::new()),
                Box::new(SystemClock),
                Box::new(PresentUsb(keyfile.clone())),
                Box::new(NoMint),
                Box::new(NullUpstream),
            )
            .expect("with_seams");
            let sink = EventSink::null();
            engine
                .init_vault(
                    Zeroizing::new("correct horse battery staple".to_string()),
                    Some(USB_UUID.to_string()),
                    Some(keyfile),
                    Default::default(),
                    &sink,
                )
                .expect("init_vault");
            engine
                .unlock(
                    Unlock::Passphrase(Zeroizing::new("correct horse battery staple".to_string())),
                    &sink,
                )
                .expect("unlock");
            engine.ca_init(true, &sink).expect("ca_init");
            // Mint a bearer under a covering ProxyMitm policy — the mint persists the policy, which is
            // what the relay-coverage gate of issue_leaf_for_covered_host consults.
            let spec = RelayPolicy {
                relay_id: "mitm-unit".to_string(),
                kind: RelayKind::Named,
                provider: Provider::Anthropic,
                secret_name: "anthropic".to_string(),
                swap: SwapMode::ProxyMitm,
                host_allow: vec![host.to_string()],
                path_allow: vec!["/".to_string()],
                method_allow: vec![Method::Post, Method::Get],
                policy_ttl_secs: 3600,
                rate_per_min: None,
                quota_total_requests: None,
                quota_total_bytes: None,
                enabled: true,
                revoked: false,
            };
            let uid = Some(rustix::process::getuid().as_raw());
            engine
                .relay_mint(spec, 3600, uid, None, &sink)
                .expect("relay_mint persists the covering policy");
            (engine, sink)
        }

        #[test]
        fn resolver_builds_from_minted_leaf_and_caches_chain() {
            let host = "api.anthropic.com";
            let (engine, sink) = covered_engine(host);
            let (chain, key) = engine
                .issue_leaf_for_covered_host(host, &sink)
                .expect("covered host mints a leaf");
            // leaf + CA → a 2-cert chain.
            assert_eq!(chain.len(), 2, "leaf chain is leaf-then-CA");
            // The resolver builds from the ECDSA leaf (rcgen ring) without error and caches it.
            let resolver = MitmCertResolver::new(host.to_string(), chain, key)
                .expect("ECDSA leaf builds a CertifiedKey");
            // Debug must not panic and must carry the host (public) but no key material.
            let dbg = format!("{resolver:?}");
            assert!(dbg.contains(host));
        }

        #[test]
        fn uncovered_host_mints_no_leaf() {
            // The covering policy is for api.anthropic.com; an UNCOVERED host is refused with NO leaf
            // (fail-closed) — there is nothing to build a resolver from, so the CONNECT yields a 502.
            let (engine, sink) = covered_engine("api.anthropic.com");
            let err = engine
                .issue_leaf_for_covered_host("evil.example.com", &sink)
                .unwrap_err();
            assert!(
                format!("{err}").contains("not covered"),
                "uncovered host must be refused, got {err}"
            );
        }
    }
}
