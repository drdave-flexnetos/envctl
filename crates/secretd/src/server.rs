//! Server assembly: build the tonic `Server` (all five services behind the per-service
//! `OwnerGuard` peercred interceptor) and serve it over a `UnixListener`-derived incoming stream.
//!
//! This is factored out of `main.rs` so the e2e test can stand up the IDENTICAL service stack over
//! its own tempdir UDS + a test-constructed engine.
use envctl_secrets::Engine;
use envctl_secrets_proto::v1::{
    audit_server::AuditServer, certs_server::CertsServer, lock_server::LockServer,
    relay_server::RelayServer, vault_server::VaultServer,
};
use tonic::codegen::tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;

use crate::grpc::{AuditSvc, CertsSvc, DaemonState, LockSvc, RelaySvc, VaultSvc};
use crate::peercred::OwnerGuard;

/// Build the configured tonic `Server` future serving over `listener`, with every service gated by
/// the `SO_PEERCRED` owner interceptor for `owner_uid`. `shutdown` resolves to trigger graceful
/// shutdown. The returned future completes when the server stops.
pub async fn serve(
    engine: Engine,
    owner_uid: u32,
    listener: tokio::net::UnixListener,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), tonic::transport::Error> {
    serve_with_state(
        engine,
        owner_uid,
        listener,
        DaemonState::default(),
        shutdown,
    )
    .await
}

/// As [`serve`], but with a caller-provided [`DaemonState`] so `main` can publish the relay proxy's
/// bound loopback address into the SAME state the `Relay.Mint` handler reads (PR-2b).
pub async fn serve_with_state(
    engine: Engine,
    owner_uid: u32,
    listener: tokio::net::UnixListener,
    state: DaemonState,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), tonic::transport::Error> {
    let guard = OwnerGuard::new(owner_uid);

    let incoming = UnixListenerStream::new(listener);

    Server::builder()
        .add_service(VaultServer::with_interceptor(
            VaultSvc {
                engine: engine.clone(),
            },
            guard.clone(),
        ))
        .add_service(RelayServer::with_interceptor(
            RelaySvc {
                engine: engine.clone(),
            },
            guard.clone(),
        ))
        .add_service(LockServer::with_interceptor(
            LockSvc {
                engine: engine.clone(),
                state: state.clone(),
            },
            guard.clone(),
        ))
        .add_service(AuditServer::with_interceptor(
            AuditSvc {
                engine: engine.clone(),
            },
            guard.clone(),
        ))
        .add_service(CertsServer::with_interceptor(CertsSvc { engine }, guard))
        .serve_with_incoming_shutdown(incoming, shutdown)
        .await
}
