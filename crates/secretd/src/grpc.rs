//! gRPC service implementations (Vault / Relay / Lock / Audit / Certs) over the engine.
//!
//! Each mutating RPC server-streams `Event`s (bridged from the engine's std-mpsc `SecretEvent`
//! stream via [`crate::audit::run_streaming`]); unary RPCs run the SYNC engine call on
//! `spawn_blocking` and map its result. Security OUTCOMES are committed to the durable hash-chained
//! audit log by the engine BEFORE the RPC returns (HF-14).
//!
//! REVEAL / broker_only invariant: the daemon NEVER re-implements the reveal gate — `Vault.Get`
//! forwards `reveal`/`apply` to `engine.secret_get`, which refuses a broker_only reveal and
//! apply-gates an allowed one. A refusal surfaces as `Status::permission_denied` with an EMPTY
//! value, so the real key never crosses the wire for a broker_only secret.
//!
//! Several RPCs return `Unimplemented` for Phase 6 (documented per-RPC): the engine exposes no
//! public path for them and the engine crate is UNTOUCHED. They are: `Vault.List`, `Vault.Rm`,
//! `Vault.Rotate`, `Relay.Create`, `Relay.List`, `Audit.Query`, and ALL of `Certs.*`.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use envctl_secrets::{Engine, EventSink, Unlock};
use envctl_secrets_proto::v1;
use tonic::{Request, Response, Status};
use zeroize::Zeroizing;

use crate::audit::{run_streaming, EventStream};
use crate::conv;

/// Shared daemon-side state the engine does not expose publicly. The engine remains the authority;
/// these are best-effort mirrors for `Lock.Status` (the engine has no public `status()`).
#[derive(Clone, Default)]
pub struct DaemonState {
    /// Mirror of the last successful Unlock/Lock outcome. The engine is the true authority.
    pub unlocked: Arc<AtomicBool>,
}

/// Map a `JoinError` from `spawn_blocking` to an internal status.
fn join_err(e: tokio::task::JoinError) -> Status {
    Status::internal(format!("blocking task failed: {e}"))
}

// ============================================================================================
// Vault
// ============================================================================================

#[derive(Clone)]
pub struct VaultSvc {
    pub engine: Engine,
}

#[tonic::async_trait]
impl v1::vault_server::Vault for VaultSvc {
    type AddStream = EventStream;
    type RmStream = EventStream;
    type RotateStream = EventStream;

    async fn add(
        &self,
        request: Request<v1::AddSecretReq>,
    ) -> Result<Response<Self::AddStream>, Status> {
        let req = request.into_inner();
        let meta = conv::add_req_to_meta(&req);
        // Move the value into a Zeroizing buffer; the proto buffer is dropped with `req`.
        let body = Zeroizing::new(req.value);
        let stream = run_streaming(self.engine.clone(), move |engine, sink: &EventSink| {
            engine.secret_put(meta, body, sink)
        });
        Ok(Response::new(stream))
    }

    async fn get(
        &self,
        request: Request<v1::GetSecretReq>,
    ) -> Result<Response<v1::GetSecretResp>, Status> {
        let req = request.into_inner();
        let name = req.name.clone();
        let reveal = req.reveal;
        // `confirm` is an EXTRA control-plane belt: a reveal that is not BOTH applied AND confirmed is
        // treated as a dry-run (apply=false passed through), so the engine never reveals. We still
        // forward `apply` truthfully so the ENGINE stays the authority on the reveal gate.
        let apply = req.apply && req.confirm;
        let engine = self.engine.clone();
        let res = tokio::task::spawn_blocking(move || {
            let sink = EventSink::null();
            engine.secret_get(&name, reveal, apply, &sink)
        })
        .await
        .map_err(join_err)?;

        match res {
            Ok(value) => {
                // The engine bails (Err) for BOTH a broker_only reveal and a `reveal && !apply`
                // dry-run, so reaching `Ok` with `reveal == true` means an APPLIED, allowed reveal
                // (apply was folded above and enforced by the engine). `revealed` therefore tracks
                // `reveal` directly — NOT value-emptiness — so a genuinely zero-length secret still
                // reports `revealed = true` on a successful reveal. The value is forwarded as-is on a
                // reveal and is the engine's empty buffer on a non-reveal (metadata-only) read.
                let revealed = reveal;
                // Phase 6 honesty: the engine exposes no public metadata read path, so we report
                // `meta: None` rather than fabricating all-false fields (which would misleadingly
                // claim broker_only=false / version=0 for an unknown secret). A real metadata
                // accessor populates this when it lands.
                Ok(Response::new(v1::GetSecretResp {
                    meta: None,
                    value: if revealed { value.to_vec() } else { Vec::new() },
                    revealed,
                }))
            }
            // A refusal (broker_only / apply-not-set) or any engine error: the real key NEVER crosses
            // the wire — the value is empty and the status carries no key material.
            Err(e) => Err(Status::permission_denied(e.to_string())),
        }
    }

    async fn list(
        &self,
        _request: Request<v1::ListSecretReq>,
    ) -> Result<Response<v1::ListSecretResp>, Status> {
        // The engine exposes no public secret-list path (the store is private); UNIMPLEMENTED in
        // Phase 6 (a thin public `Engine` list lands later).
        Err(Status::unimplemented("Vault.List is not available in Phase 6"))
    }

    async fn rm(
        &self,
        _request: Request<v1::RmSecretReq>,
    ) -> Result<Response<Self::RmStream>, Status> {
        // No public `secret_rm` on the engine (engine UNTOUCHED).
        Err(Status::unimplemented("Vault.Rm is not available in Phase 6"))
    }

    async fn rotate(
        &self,
        _request: Request<v1::RotateReq>,
    ) -> Result<Response<Self::RotateStream>, Status> {
        // No public `secret_rotate` on the engine (engine UNTOUCHED).
        Err(Status::unimplemented("Vault.Rotate is not available in Phase 6"))
    }
}

// ============================================================================================
// Relay
// ============================================================================================

#[derive(Clone)]
pub struct RelaySvc {
    pub engine: Engine,
}

#[tonic::async_trait]
impl v1::relay_server::Relay for RelaySvc {
    type CreateStream = EventStream;

    async fn create(
        &self,
        _request: Request<v1::CreateRelayReq>,
    ) -> Result<Response<Self::CreateStream>, Status> {
        // No public create-policy verb on the engine (policies are persisted as a side effect of
        // `relay_mint`); UNIMPLEMENTED in Phase 6.
        Err(Status::unimplemented(
            "Relay.Create is not available in Phase 6 (policy is persisted by Mint)",
        ))
    }

    async fn revoke(
        &self,
        request: Request<v1::RevokeRelayReq>,
    ) -> Result<Response<v1::RevokeResp>, Status> {
        let req = request.into_inner();
        // Root-of-trust verb: an `apply` without `confirm` DOWNGRADES to a dry-run.
        let apply = req.apply && req.confirm;
        let name = req.name;
        let engine = self.engine.clone();
        let n = tokio::task::spawn_blocking(move || {
            let sink = EventSink::null();
            engine.relay_revoke(&name, apply, &sink)
        })
        .await
        .map_err(join_err)?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(v1::RevokeResp {
            count_revoked: n,
            dry_run: !apply,
        }))
    }

    async fn revoke_bearer(
        &self,
        request: Request<v1::RevokeBearerReq>,
    ) -> Result<Response<v1::RevokeResp>, Status> {
        let req = request.into_inner();
        let apply = req.apply;
        let token_id = req.token_id;
        let engine = self.engine.clone();
        let n = tokio::task::spawn_blocking(move || {
            let sink = EventSink::null();
            engine.relay_revoke_bearer(&token_id, apply, &sink)
        })
        .await
        .map_err(join_err)?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(v1::RevokeResp {
            count_revoked: n,
            dry_run: !apply,
        }))
    }

    async fn list(
        &self,
        _request: Request<v1::ListRelayReq>,
    ) -> Result<Response<v1::ListRelayResp>, Status> {
        // No public relay-list path on the engine; UNIMPLEMENTED in Phase 6.
        Err(Status::unimplemented("Relay.List is not available in Phase 6"))
    }

    async fn mint(
        &self,
        request: Request<v1::MintReq>,
    ) -> Result<Response<v1::MintResp>, Status> {
        // The peer uid is the connect-time-frozen owner uid (peercred-gated channel); the peer pid
        // is the client-supplied target pid (advisory peer binding for `env-ctl run` ephemerals).
        let peer_uid = request
            .extensions()
            .get::<tonic::transport::server::UdsConnectInfo>()
            .and_then(|i| i.peer_cred)
            .map(|c| c.uid());
        let req = request.into_inner();

        // Reject a TTL that does not fit i64 at the boundary (the engine clamps to <=24h, but must
        // not be handed a wrapping value).
        let ttl_secs: i64 = i64::try_from(req.ttl_secs)
            .map_err(|_| Status::invalid_argument("ttl_secs does not fit i64"))?;
        let peer_pid = (req.client_pid != 0).then_some(req.client_pid);

        // Phase 6: no public policy load, so synthesize the policy from the request. `relay_mint`
        // persists it as a side effect and mints a <=24h, USB-gated, peer-bound bearer against it.
        let spec = conv::mint_req_to_policy(&req);
        let engine = self.engine.clone();

        let bearer = tokio::task::spawn_blocking(move || {
            let sink = EventSink::null();
            engine.relay_mint(spec, ttl_secs, peer_uid, peer_pid, &sink)
        })
        .await
        .map_err(join_err)?
        .map_err(|e| Status::permission_denied(e.to_string()))?;

        // The raw bearer goes to the OWNER only (peercred-gated UDS); the REAL key is NEVER here.
        Ok(Response::new(v1::MintResp {
            bearer: bearer.raw.to_string(),
            expires_at: bearer.expires_at,
            injection: None, // injection_template is not wired in Phase 6
            token_id: bearer.token_id,
        }))
    }
}

// ============================================================================================
// Lock
// ============================================================================================

#[derive(Clone)]
pub struct LockSvc {
    pub engine: Engine,
    pub state: DaemonState,
}

#[tonic::async_trait]
impl v1::lock_server::Lock for LockSvc {
    type UnlockStream = EventStream;
    type LockNowStream = EventStream;

    async fn status(
        &self,
        _request: Request<v1::StatusReq>,
    ) -> Result<Response<v1::StatusResp>, Status> {
        // PARTIAL within the public-API constraint: `unlocked` mirrors the last Unlock/Lock outcome
        // (engine is the authority); `usb_possessed`/`active_relays`/`secret_count` have no public
        // query path and are reported best-effort (false/0).
        Ok(Response::new(v1::StatusResp {
            unlocked: self.state.unlocked.load(Ordering::SeqCst),
            usb_possessed: false,
            active_relays: 0,
            secret_count: 0,
        }))
    }

    async fn unlock(
        &self,
        request: Request<v1::UnlockReq>,
    ) -> Result<Response<Self::UnlockStream>, Status> {
        let req = request.into_inner();
        // Wrap the passphrase in Zeroizing immediately; the proto String is dropped with `req`.
        let unlock = match req.passphrase {
            Some(pp) => Unlock::Passphrase(Zeroizing::new(pp)),
            None => Unlock::Usb,
        };
        let flag = self.state.unlocked.clone();
        let stream = run_streaming(self.engine.clone(), move |engine, sink: &EventSink| {
            let r = engine.unlock(unlock, sink);
            if r.is_ok() {
                flag.store(true, Ordering::SeqCst);
            }
            r.map(|_state| ())
        });
        Ok(Response::new(stream))
    }

    async fn lock_now(
        &self,
        _request: Request<v1::LockReq>,
    ) -> Result<Response<Self::LockNowStream>, Status> {
        let flag = self.state.unlocked.clone();
        let stream = run_streaming(self.engine.clone(), move |engine, sink: &EventSink| {
            let r = engine.lock(sink);
            if r.is_ok() {
                flag.store(false, Ordering::SeqCst);
            }
            r
        });
        Ok(Response::new(stream))
    }
}

// ============================================================================================
// Audit
// ============================================================================================

#[derive(Clone)]
pub struct AuditSvc {
    // Held for when Audit.Query is wired to a public engine read path (Phase 6: Unimplemented).
    #[allow(dead_code)]
    pub engine: Engine,
}

#[tonic::async_trait]
impl v1::audit_server::Audit for AuditSvc {
    async fn query(
        &self,
        _request: Request<v1::AuditQueryReq>,
    ) -> Result<Response<v1::AuditQueryResp>, Status> {
        // The engine's hash-chained audit log lives behind the private `store`; there is no public
        // `Engine::query_audit` (engine UNTOUCHED). UNIMPLEMENTED in Phase 6 — audit outcomes are
        // observed via the Event stream and unary RPC results until a public read path is added.
        Err(Status::unimplemented("Audit.Query is not available in Phase 6"))
    }
}

// ============================================================================================
// Certs (all Unimplemented — Phase 4+)
// ============================================================================================

#[derive(Clone)]
pub struct CertsSvc {
    // Held for when the CA path (ca_issue etc.) is wired (Phase 4+: all Unimplemented).
    #[allow(dead_code)]
    pub engine: Engine,
}

#[tonic::async_trait]
impl v1::certs_server::Certs for CertsSvc {
    type CaInitStream = EventStream;
    type CaRotateStream = EventStream;
    type IssueStream = EventStream;
    type RenewStream = EventStream;
    type RevokeStream = EventStream;
    type TrustApplyStream = EventStream;

    async fn ca_init(
        &self,
        _request: Request<v1::CaInitReq>,
    ) -> Result<Response<Self::CaInitStream>, Status> {
        Err(Status::unimplemented("Certs.CaInit is Phase 4+"))
    }
    async fn ca_rotate(
        &self,
        _request: Request<v1::CaRotateReq>,
    ) -> Result<Response<Self::CaRotateStream>, Status> {
        Err(Status::unimplemented("Certs.CaRotate is Phase 4+"))
    }
    async fn issue(
        &self,
        _request: Request<v1::IssueLeafReq>,
    ) -> Result<Response<Self::IssueStream>, Status> {
        Err(Status::unimplemented("Certs.Issue is Phase 4+"))
    }
    async fn renew(
        &self,
        _request: Request<v1::RenewLeafReq>,
    ) -> Result<Response<Self::RenewStream>, Status> {
        Err(Status::unimplemented("Certs.Renew is Phase 4+"))
    }
    async fn revoke(
        &self,
        _request: Request<v1::RevokeLeafReq>,
    ) -> Result<Response<Self::RevokeStream>, Status> {
        Err(Status::unimplemented("Certs.Revoke is Phase 4+"))
    }
    async fn trust_apply(
        &self,
        _request: Request<v1::TrustReq>,
    ) -> Result<Response<Self::TrustApplyStream>, Status> {
        Err(Status::unimplemented("Certs.TrustApply is Phase 4+"))
    }
    async fn list(
        &self,
        _request: Request<v1::ListCertReq>,
    ) -> Result<Response<v1::ListCertResp>, Status> {
        Err(Status::unimplemented("Certs.List is Phase 4+"))
    }
}
