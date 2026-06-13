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
    /// The relay proxy's bound `127.0.0.1:<port>` loopback address, published ONCE by `main` after
    /// `serve_proxy` binds. PR-2b's `Relay.Mint` reads it to fill `MintResp.injection` so the child
    /// is repointed at the proxy. `None` until the proxy has bound (or if it failed to bind).
    pub proxy_addr: Arc<std::sync::OnceLock<std::net::SocketAddr>>,
}

/// Map a `JoinError` from `spawn_blocking` to an internal status.
fn join_err(e: tokio::task::JoinError) -> Status {
    Status::internal(format!("blocking task failed: {e}"))
}

/// Read the USB keyslot keyfile for `partuuid` via the engine's `UsbProbe` seam (the same seam the
/// engine's unlock path uses to PROVE possession). The keyfile is HKDF IKM only — it never crosses
/// the wire and is never persisted (the engine drops it after wrapping the slot).
///
/// Forwards to `RealUsbProbe::keyfile_for`. Built with `--features seed-factor`, that resolves the
/// keyfile from the **Cognitum Seed** (a deterministic, PARTUUID-bound Ed25519 signature) — so the
/// material is identical at init and at unlock for the same `partuuid`. Without the feature the seam
/// returns `None` and we refuse cleanly (fail-closed): a stock daemon has no USB backend, so the
/// operator enrolls the passphrase keyslot and completes USB enrollment with a seed-factor build +
/// the Seed present. `None` also covers a Seed that is unreachable/unpaired at runtime.
fn read_usb_keyfile(partuuid: &str) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    use envctl_secrets::{RealUsbProbe, UsbProbe};
    RealUsbProbe.keyfile_for(partuuid).ok_or_else(|| {
        "USB possession not proven: the Cognitum Seed USB factor is unavailable (Seed unreachable / \
         unpaired, or this secretd was not built with --features seed-factor). Enroll the passphrase \
         keyslot as recovery and retry USB enrollment with the Seed present."
            .to_string()
    })
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
    type InitStream = EventStream;
    type AddStream = EventStream;
    type RmStream = EventStream;
    type RotateStream = EventStream;

    /// Vault.Init — genesis: mint the DEK + enroll the passphrase keyslot (and optionally a USB
    /// keyslot) over `Engine::init_vault`. Owner-only (the SO_PEERCRED interceptor already gated the
    /// channel). FAIL-CLOSED + apply-gated:
    ///   * `apply=false` (the default) is a DRY-RUN: it emits a preview of what init WOULD do and
    ///     mutates NOTHING (no DEK, no keyslot, no audit row).
    ///   * `apply=true` runs the real `init_vault`, which itself REFUSES to clobber an existing vault
    ///     (engine guard) and re-validates the Argon2 floor.
    /// The daemon FORCES the hardened Argon2 params server-side ([`conv::forced_argon2_params`]); the
    /// client never supplies KDF params. The optional passphrase is owner-only over the UDS and is
    /// zeroized after `init_vault` derives from it. For a USB keyslot the keyfile is read via the
    /// `UsbProbe` seam by PARTUUID — it is NEVER carried on the wire.
    async fn init(
        &self,
        request: Request<v1::InitReq>,
    ) -> Result<Response<Self::InitStream>, Status> {
        let req = request.into_inner();
        // Validate USB fields at the boundary (enroll_usb REQUIRES a PARTUUID). Fails closed.
        let usb_uuid = conv::init_usb_uuid(&req)?;
        let apply = req.apply;
        // Move the optional passphrase into a Zeroizing buffer immediately; the proto String drops
        // with `req`. A missing passphrase means a USB-only enrollment (no passphrase keyslot is
        // valid only if a USB slot is enrolled — the engine requires at least one factor).
        let passphrase: Zeroizing<String> = Zeroizing::new(req.passphrase.unwrap_or_default());
        let params = conv::forced_argon2_params();

        let stream = run_streaming(self.engine.clone(), move |engine, sink: &EventSink| {
            use envctl_secrets::event::{SecretEvent, Stream};
            // DRY-RUN (the default, CF-8): preview only — mutate nothing.
            if !apply {
                let usb_note = match &usb_uuid {
                    Some(u) => format!(" + a USB keyslot for PARTUUID {u}"),
                    None => String::new(),
                };
                sink.emit(SecretEvent::Log {
                    source: "vault.init".to_string(),
                    stream: Stream::Stdout,
                    line: format!(
                        "DRY-RUN: would initialize a fresh vault (passphrase keyslot{usb_note}; \
                         Argon2id m={} KiB, t={}, p={}). Re-run with --apply to mutate.",
                        params.m_kib, params.t_cost, params.p_lanes
                    ),
                });
                return Ok(());
            }

            // APPLY: read the USB keyfile via the seam (possession is proven cryptographically by the
            // engine when it wraps the slot). The keyfile NEVER crosses the wire.
            let usb_keyfile = match &usb_uuid {
                Some(uuid) => match read_usb_keyfile(uuid) {
                    Ok(kf) => Some(kf),
                    Err(e) => anyhow::bail!(e),
                },
                None => None,
            };

            // The engine refuses to clobber an existing vault and re-validates the Argon2 floor.
            engine.init_vault(passphrase, usb_uuid.clone(), usb_keyfile, params, sink)
        });
        Ok(Response::new(stream))
    }

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
        Err(Status::unimplemented(
            "Vault.List is not available in Phase 6",
        ))
    }

    async fn rm(
        &self,
        _request: Request<v1::RmSecretReq>,
    ) -> Result<Response<Self::RmStream>, Status> {
        // No public `secret_rm` on the engine (engine UNTOUCHED).
        Err(Status::unimplemented(
            "Vault.Rm is not available in Phase 6",
        ))
    }

    async fn rotate(
        &self,
        _request: Request<v1::RotateReq>,
    ) -> Result<Response<Self::RotateStream>, Status> {
        // No public `secret_rotate` on the engine (engine UNTOUCHED).
        Err(Status::unimplemented(
            "Vault.Rotate is not available in Phase 6",
        ))
    }
}

// ============================================================================================
// Relay
// ============================================================================================

#[derive(Clone)]
pub struct RelaySvc {
    pub engine: Engine,
    pub state: DaemonState,
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
        Err(Status::unimplemented(
            "Relay.List is not available in Phase 6",
        ))
    }

    async fn mint(&self, request: Request<v1::MintReq>) -> Result<Response<v1::MintResp>, Status> {
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
        // Capture the provider + data-plane mode BEFORE the spec is moved into the blocking closure;
        // they shape the child env injection built once the bearer is minted.
        let provider = spec.provider;
        let mode = conv::dataplane_mode_from_swap(&spec.swap);
        let engine = self.engine.clone();

        let bearer = tokio::task::spawn_blocking(move || {
            let sink = EventSink::null();
            engine.relay_mint(spec, ttl_secs, peer_uid, peer_pid, &sink)
        })
        .await
        .map_err(join_err)?
        .map_err(|e| Status::permission_denied(e.to_string()))?;

        // PR-2b: build the child env injection so `env-ctl run` repoints the child at the loopback
        // relay proxy and overlays the BEARER (never the real key). FAIL-CLOSED: if the proxy has not
        // bound (no `proxy_addr`), we ship `injection: None` rather than fabricating an address — the
        // client then refuses to spawn rather than hand the child a half-built env.
        let injection = match self.state.proxy_addr.get() {
            Some(addr) => {
                let proxy_url = format!("http://{addr}");
                // `ca_pem_path` is empty for the BaseUrlRepoint plane (no MITM CA). For the
                // HttpsProxyMitm plane (PR-3b) the child MUST trust the engine-minted local CA, so we
                // materialize the public CA bundle here. FAIL-CLOSED: if no CA is initialized,
                // `engine.ca_pem_path()` errors and we refuse the mint rather than ship a MITM
                // injection whose child can never validate the leaf (no half-built env).
                let ca_pem_path = ca_pem_path_for_mode(&self.engine, mode)?;
                let resolved = envctl_secrets::inject::injection_template(
                    provider,
                    &bearer.raw,
                    &proxy_url,
                    &ca_pem_path,
                    mode,
                );
                Some(conv::injection_to_proto(&resolved))
            }
            None => None,
        };

        // The raw bearer goes to the OWNER only (peercred-gated UDS); the REAL key is NEVER here.
        Ok(Response::new(v1::MintResp {
            bearer: bearer.raw.to_string(),
            expires_at: bearer.expires_at,
            injection,
            token_id: bearer.token_id,
        }))
    }
}

/// Resolve the child-trust `ca_pem_path` for a data-plane `mode`. The `BaseUrlRepoint` /
/// `NativeSubtoken` planes do NOT terminate TLS, so the child uses its normal OS roots and the path
/// is empty. The `HttpsProxyMitm` plane terminates the child's TLS with an engine-minted leaf, so
/// the child MUST trust the engine's local CA: we materialize the public CA bundle and return its
/// path. FAIL-CLOSED: an uninitialized CA errors (`failed_precondition`) — we never ship a MITM
/// injection whose child can't validate the leaf.
// Boxing `tonic::Status` is non-idiomatic at the gRPC boundary (mirrors conv.rs's module allow); a
// `failed_precondition` here is the documented fail-closed path, so the large-Err is intentional.
#[allow(clippy::result_large_err)]
fn ca_pem_path_for_mode(
    engine: &Engine,
    mode: envctl_secrets::inject::DataPlaneMode,
) -> Result<String, Status> {
    use envctl_secrets::inject::DataPlaneMode;
    match mode {
        DataPlaneMode::HttpsProxyMitm => {
            #[cfg(feature = "mitm-ca")]
            {
                let path = engine
                    .ca_pem_path()
                    .map_err(|_| Status::failed_precondition("MITM CA not initialized"))?;
                Ok(path.to_string_lossy().into_owned())
            }
            #[cfg(not(feature = "mitm-ca"))]
            {
                let _ = engine;
                Err(Status::failed_precondition(
                    "MITM data plane requires the mitm-ca feature",
                ))
            }
        }
        DataPlaneMode::BaseUrlRepoint | DataPlaneMode::NativeSubtoken => Ok(String::new()),
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
        Err(Status::unimplemented(
            "Audit.Query is not available in Phase 6",
        ))
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
