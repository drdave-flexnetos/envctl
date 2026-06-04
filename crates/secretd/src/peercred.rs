//! The `SO_PEERCRED` owner-only gate.
//!
//! tonic 0.12, serving over a `UnixListener`-derived incoming stream, stamps each `Request`'s
//! extensions with `tonic::transport::server::UdsConnectInfo { peer_addr, peer_cred }`, where
//! `peer_cred: Option<tokio::net::unix::UCred>` is the connect-time-frozen `SO_PEERCRED`. We gate on
//! it with a per-RPC tower `Interceptor`: it runs in tower's request path strictly BEFORE the
//! handler body, so a uid mismatch is rejected before any `spawn_blocking`/`engine.*` call and
//! before any durable audit row is written — exactly the "reject uid != owner BEFORE any engine
//! call" requirement.
//!
//! Filesystem perms (socket `0600`, dirs `0700`) are the FIRST wall; this uid check is the
//! authoritative one (defense in depth). The interceptor runs on EVERY call, so the uid is re-read
//! per request (the creds are connect-time-frozen, but re-checking each call narrows the documented
//! connect-time TOCTOU). Local-root bypass is an out-of-threat-model residual.
//!
//! NOTE ON THE rustix FALLBACK: the research doc also offers reading the creds directly off the
//! accepted FD via `rustix::net::sockopt::socket_peercred(fd)`. That is only needed for a CUSTOM
//! accept loop (the relay/manual-accept case, out of scope here). For the control plane we use
//! `serve_with_incoming` + the tonic `UdsConnectInfo` interceptor — the fewest-moving-parts path.
use tonic::service::Interceptor;
use tonic::transport::server::UdsConnectInfo;
use tonic::{Request, Status};

/// A `Clone`able tower interceptor that admits a request only if its peer's `SO_PEERCRED` uid equals
/// `owner_uid`. FAIL CLOSED: a missing `UdsConnectInfo` (no UDS) or a `None` `peer_cred` (no
/// `SO_PEERCRED`) is `permission_denied`. Only the uid is an authz key; `pid` is advisory/log-only.
#[derive(Clone)]
pub struct OwnerGuard {
    owner_uid: u32,
}

impl OwnerGuard {
    pub fn new(owner_uid: u32) -> Self {
        OwnerGuard { owner_uid }
    }
}

impl Interceptor for OwnerGuard {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        // Each rejection is logged at WARN so repeated denials (a non-owner local uid probing the
        // socket) are observable in the daemon log; the rejection still happens BEFORE any handler.
        let info = req.extensions().get::<UdsConnectInfo>().ok_or_else(|| {
            tracing::warn!("peercred deny: no UdsConnectInfo (non-UDS transport?)");
            Status::permission_denied("no peer credentials")
        })?;
        let cred = info.peer_cred.ok_or_else(|| {
            tracing::warn!("peercred deny: no SO_PEERCRED on the connection");
            Status::permission_denied("no SO_PEERCRED")
        })?;
        if cred.uid() != self.owner_uid {
            tracing::warn!(
                peer_uid = cred.uid(),
                owner_uid = self.owner_uid,
                "peercred deny: uid mismatch"
            );
            return Err(Status::permission_denied("uid mismatch"));
        }
        Ok(req)
    }
}

/// The owner uid the daemon enforces: the process's own uid, read once at startup. Matches the
/// engine's `current_uid()` (`rustix::process::getuid().as_raw()`), so the daemon and the engine
/// agree on who the owner is.
pub fn owner_uid() -> u32 {
    rustix::process::getuid().as_raw()
}
