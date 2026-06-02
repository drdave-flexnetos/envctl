//! End-to-end + targeted coverage of the REAL `secretd` daemon code.
//!
//! `secretd` is now a `lib` (`envctl_secretd`) consumed by a thin `main.rs` bin, so these tests drive
//! the PRODUCTION modules directly — `envctl_secretd::server::serve` (which assembles the five
//! services behind the real `peercred::OwnerGuard` interceptor), the real `grpc` handlers, the real
//! `conv` conversions, and the real `audit::run_streaming` bridge. No inline replica.
//!
//! Coverage:
//!   1. `e2e_control_plane_roundtrip_and_wire_secrecy` — the full happy path over a REAL UDS against
//!      the REAL server stack, AND the load-bearing invariant: a reveal of a broker_only secret is
//!      REFUSED (PermissionDenied) and the real key never appears in any byte the client receives.
//!   2. `e2e_authz_deny_non_owner` — the NEGATIVE authz path: a server whose `owner_uid` is NOT our
//!      uid rejects EVERY RPC with `PermissionDenied`, BEFORE any engine call (the engine is left
//!      locked, proving the guard fires first). This exercises the real `OwnerGuard` and the real
//!      `server::serve` wiring on the deny side.
//!   3. `conv_event_to_proto_drift` — the promised drift/round-trip test: the REAL
//!      `conv::event_to_proto` maps every `SecretEvent` arm to the expected proto `Event::Kind` (the
//!      with-twin arms) or to `None` (the twin-less arms), and `conv::mint_req_to_policy` /
//!      `conv::add_req_to_meta` produce the documented engine shapes.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use envctl_secrets::keyslot::Argon2Params;
use envctl_secrets::paths::Paths;
use envctl_secrets::seam::{NoMint, SystemClock, UpstreamError, UsbProbe};
use envctl_secrets::vault::{InMemStore, Store};
use envctl_secrets::{EgressReq, EgressResp, Engine, EventSink, Upstream};
use envctl_secrets_proto::v1;
use hyper_util::rt::TokioIo;
use tonic::transport::{Endpoint, Uri};
use tonic::Streaming;
use zeroize::Zeroizing;

const SENTINEL: &[u8] = b"REAL-KEY-SENTINEL";
const USB_UUID: &str = "E2E-USB-1234";

// ---- fakes -----------------------------------------------------------------------------------

/// A USB probe that hands back a fixed keyfile for `USB_UUID`, modeling proven possession.
struct PresentUsb {
    keyfile: Zeroizing<Vec<u8>>,
}
impl UsbProbe for PresentUsb {
    fn keyfile_for(&self, uuid: &str) -> Option<Zeroizing<Vec<u8>>> {
        if uuid == USB_UUID {
            Some(self.keyfile.clone())
        } else {
            None
        }
    }
}

/// A no-op upstream (the swap data plane is out of scope here).
#[derive(Clone)]
struct NullUpstream;
#[async_trait::async_trait]
impl Upstream for NullUpstream {
    async fn send(
        &self,
        _req: EgressReq,
        _real_key: &Zeroizing<Vec<u8>>,
    ) -> Result<EgressResp, UpstreamError> {
        Err(UpstreamError::Io("upstream not wired in e2e".into()))
    }
}

// ---- shared helpers ---------------------------------------------------------------------------

/// Build an engine over an in-mem store + present-USB seam. Does NOT init/unlock the vault.
fn make_engine(paths: &Paths, keyfile: &Zeroizing<Vec<u8>>) -> Engine {
    Engine::with_seams(
        paths.clone(),
        Box::new(InMemStore::new()) as Box<dyn Store>,
        Box::new(SystemClock),
        Box::new(PresentUsb {
            keyfile: keyfile.clone(),
        }),
        Box::new(NoMint),
        Box::new(NullUpstream),
    )
    .expect("with_seams")
}

/// A tempdir + 0700 runtime dir, unique per test name.
fn temp_paths(tag: &str) -> (PathBuf, Paths) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("envctl-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let paths = Paths::under(dir.clone());
    std::fs::create_dir_all(&paths.runtime).unwrap();
    std::fs::set_permissions(&paths.runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    (dir, paths)
}

/// Bind a UDS at `sock`, 0600, and return a listener.
fn bind(sock: &std::path::Path) -> tokio::net::UnixListener {
    use std::os::unix::fs::PermissionsExt;
    let listener = tokio::net::UnixListener::bind(sock).expect("bind UDS");
    std::fs::set_permissions(sock, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener
}

async fn connect(sock: PathBuf) -> tonic::transport::Channel {
    Endpoint::try_from("http://[::]:0")
        .unwrap()
        .connect_with_connector(tower::service_fn(move |_: Uri| {
            let sock = sock.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(sock).await?;
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await
        .expect("connect to daemon UDS")
}

async fn drain(mut s: Streaming<v1::Event>, buf: &Arc<Mutex<Vec<u8>>>) -> Vec<v1::Event> {
    let mut out = Vec::new();
    while let Some(ev) = s.message().await.expect("stream message") {
        // Capture every byte the client receives from the event stream (wire-secrecy assertion).
        buf.lock().unwrap().extend_from_slice(format!("{ev:?}").as_bytes());
        out.push(ev);
    }
    out
}

/// Does `haystack` contain `needle` as a contiguous subslice?
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

// ---- test 1: REAL server happy path + wire secrecy -------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn e2e_control_plane_roundtrip_and_wire_secrecy() {
    // Every byte the client ever RECEIVES from the daemon (responses + events) is appended here; the
    // load-bearing assertion checks the broker_only sentinel never appears in it.
    let wire: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    // 1. Tempdir paths + 0700 runtime dir.
    let (dir, paths) = temp_paths("ok");

    // 2. Engine constructed DIRECTLY with a present-USB seam (so Mint's USB gate passes). Init enrolls
    //    a passphrase slot AND a USB slot, then we unlock over the wire below.
    let keyfile = Zeroizing::new(vec![0x5Au8; 64]);
    let engine = make_engine(&paths, &keyfile);

    let sink0 = EventSink::null();
    engine
        .init_vault(
            Zeroizing::new("correct horse battery staple".to_string()),
            Some(USB_UUID.to_string()),
            Some(keyfile.clone()),
            Argon2Params::default(),
            &sink0,
        )
        .expect("init_vault");

    // 3. Serve the REAL daemon stack (envctl_secretd::server::serve) over the tempdir UDS, peercred-
    //    gated by our OWN uid so our connections pass. This drives the real OwnerGuard + grpc + conv +
    //    audit::run_streaming — NOT an inline replica.
    let sock = paths.control_socket();
    let listener = bind(&sock);
    let owner_uid = rustix::process::getuid().as_raw();

    let server_engine = engine.clone();
    let server = tokio::spawn(async move {
        envctl_secretd::server::serve(server_engine, owner_uid, listener, std::future::pending())
            .await
            .expect("serve");
    });

    // Give the listener a moment, then connect.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 4a. Unlock over the wire (passphrase).
    {
        let mut lock = v1::lock_client::LockClient::new(connect(sock.clone()).await);
        let stream = lock
            .unlock(v1::UnlockReq {
                passphrase: Some("correct horse battery staple".to_string()),
            })
            .await
            .expect("unlock rpc")
            .into_inner();
        let evs = drain(stream, &wire).await;
        assert!(
            evs.iter()
                .any(|e| matches!(&e.kind, Some(v1::event::Kind::VaultUnlocked(_)))),
            "unlock must emit VaultUnlocked, got {evs:?}"
        );
    }

    // 4b. Add a NORMAL secret and a BROKER_ONLY secret (value = the sentinel).
    {
        let mut vault = v1::vault_client::VaultClient::new(connect(sock.clone()).await);
        let s = vault
            .add(v1::AddSecretReq {
                name: "normal".into(),
                provider: v1::ProviderKind::Generic as i32,
                value: b"normal-value".to_vec(),
                note: String::new(),
                overwrite: false,
                broker_only: false,
            })
            .await
            .expect("add normal")
            .into_inner();
        let evs = drain(s, &wire).await;
        assert!(evs
            .iter()
            .any(|e| matches!(&e.kind, Some(v1::event::Kind::SecretWritten(_)))));

        let s = vault
            .add(v1::AddSecretReq {
                name: "brokeronly".into(),
                provider: v1::ProviderKind::Anthropic as i32,
                value: SENTINEL.to_vec(),
                note: String::new(),
                overwrite: false,
                broker_only: true,
            })
            .await
            .expect("add broker_only")
            .into_inner();
        let _ = drain(s, &wire).await;
    }

    // 4c. Get metadata-only on the normal secret (reveal=false): value empty, revealed=false.
    {
        let mut vault = v1::vault_client::VaultClient::new(connect(sock.clone()).await);
        let r = vault
            .get(v1::GetSecretReq {
                name: "normal".into(),
                reveal: false,
                apply: false,
                confirm: false,
            })
            .await
            .expect("get meta")
            .into_inner();
        wire.lock().unwrap().extend_from_slice(&r.value);
        assert!(!r.revealed, "metadata-only get must not reveal");
        assert!(r.value.is_empty(), "metadata-only get must have empty value");
        // Phase 6 honesty: the real grpc.rs reports meta:None (no fabricated all-false fields).
        assert!(r.meta.is_none(), "Phase 6 get must report meta:None");
    }

    // 4d. Reveal+apply+confirm on the NORMAL secret: the owner reveal escape hatch works.
    {
        let mut vault = v1::vault_client::VaultClient::new(connect(sock.clone()).await);
        let r = vault
            .get(v1::GetSecretReq {
                name: "normal".into(),
                reveal: true,
                apply: true,
                confirm: true,
            })
            .await
            .expect("reveal normal")
            .into_inner();
        wire.lock().unwrap().extend_from_slice(&r.value);
        assert!(r.revealed, "owner reveal of a normal secret must succeed");
        assert_eq!(r.value, b"normal-value", "revealed value must round-trip");
    }

    // 4e. Reveal+apply+confirm on the BROKER_ONLY secret: REFUSED, value empty.
    {
        let mut vault = v1::vault_client::VaultClient::new(connect(sock.clone()).await);
        let err = vault
            .get(v1::GetSecretReq {
                name: "brokeronly".into(),
                reveal: true,
                apply: true,
                confirm: true,
            })
            .await
            .expect_err("broker_only reveal MUST be refused");
        assert_eq!(
            err.code(),
            tonic::Code::PermissionDenied,
            "broker_only reveal must be permission_denied, got {err:?}"
        );
        // The refusal carries no key material, but capture the status text into the wire buffer too.
        wire.lock().unwrap().extend_from_slice(err.message().as_bytes());
    }

    // 4f. Dry-run reveal (reveal=true, apply=false): the engine refuses, daemon maps to PermissionDenied.
    {
        let mut vault = v1::vault_client::VaultClient::new(connect(sock.clone()).await);
        let err = vault
            .get(v1::GetSecretReq {
                name: "normal".into(),
                reveal: true,
                apply: false,
                confirm: false,
            })
            .await
            .expect_err("dry-run reveal MUST be refused by the engine");
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
        wire.lock().unwrap().extend_from_slice(err.message().as_bytes());
    }

    // 4g. Mint a bearer (USB gate passes via the present-USB seam). Drives the real grpc::Relay::mint
    //     + conv::mint_req_to_policy.
    let minted_bearer: String;
    {
        let mut relay = v1::relay_client::RelayClient::new(connect(sock.clone()).await);
        let r = relay
            .mint(v1::MintReq {
                relay: "eph-relay".into(),
                ephemeral: true,
                provider: v1::ProviderKind::Anthropic as i32,
                ttl_secs: 3600,
                client_pid: 0,
            })
            .await
            .expect("mint")
            .into_inner();
        wire.lock().unwrap().extend_from_slice(r.bearer.as_bytes());
        wire.lock().unwrap().extend_from_slice(r.token_id.as_bytes());
        assert!(!r.bearer.is_empty(), "minted bearer must be non-empty");
        assert!(
            r.bearer.starts_with("evrelay_"),
            "bearer must carry the evrelay_ prefix, got {}",
            r.bearer
        );
        assert!(!r.token_id.is_empty(), "token_id must be non-empty");
        minted_bearer = r.bearer;
    }

    // 4h. Audit.Query is Unimplemented in Phase 6; assert that contract against the REAL AuditSvc.
    {
        let mut audit = v1::audit_client::AuditClient::new(connect(sock.clone()).await);
        let err = audit
            .query(v1::AuditQueryReq {
                actor: None,
                relay: None,
                since: None,
                until: None,
                limit: 0,
            })
            .await
            .expect_err("Audit.Query is Unimplemented in Phase 6");
        assert_eq!(err.code(), tonic::Code::Unimplemented);
    }

    // 5. THE LOAD-BEARING ASSERTION: the broker_only plaintext sentinel never appeared in ANY byte
    //    the client received (reveal was refused, so its bytes never left the daemon), and the
    //    minted bearer is a random authenticator, not the key.
    let received = wire.lock().unwrap().clone();
    assert!(
        !contains(&received, SENTINEL),
        "broker_only plaintext sentinel leaked onto the wire!"
    );
    assert!(
        !contains(minted_bearer.as_bytes(), SENTINEL),
        "the minted bearer must not contain the real key sentinel"
    );

    // 6. Teardown.
    server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- test 2: REAL server NEGATIVE authz path -------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn e2e_authz_deny_non_owner() {
    // 1. Tempdir + a FRESH engine that is NEVER inited/unlocked. The OwnerGuard must reject before any
    //    engine call, so a locked engine is fine — if a handler ran, it would error differently.
    let (dir, paths) = temp_paths("deny");
    let keyfile = Zeroizing::new(vec![0x5Au8; 64]);
    let engine = make_engine(&paths, &keyfile);

    // 2. Serve the REAL stack with owner_uid set to a uid that is GUARANTEED not to match ours, so the
    //    real OwnerGuard denies every request. (uid + 1, wrapping is irrelevant: it is simply != ours.)
    let our_uid = rustix::process::getuid().as_raw();
    let wrong_owner = our_uid.wrapping_add(1);
    let sock = paths.control_socket();
    let listener = bind(&sock);

    let server = tokio::spawn(async move {
        envctl_secretd::server::serve(engine, wrong_owner, listener, std::future::pending())
            .await
            .expect("serve");
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 3. Every service's representative RPC must be PermissionDenied (uid mismatch), NOT Unimplemented
    //    or Internal — proving the guard fires BEFORE the handler body on EVERY service.
    let denied = |label: &str, code: tonic::Code| {
        assert_eq!(
            code,
            tonic::Code::PermissionDenied,
            "{label} must be PermissionDenied for a non-owner uid, got {code:?}"
        );
    };

    {
        let mut lock = v1::lock_client::LockClient::new(connect(sock.clone()).await);
        let e = lock.status(v1::StatusReq {}).await.expect_err("lock.status");
        denied("Lock.Status", e.code());
        let e = lock
            .unlock(v1::UnlockReq { passphrase: None })
            .await
            .expect_err("lock.unlock");
        denied("Lock.Unlock", e.code());
    }
    {
        let mut vault = v1::vault_client::VaultClient::new(connect(sock.clone()).await);
        let e = vault
            .get(v1::GetSecretReq {
                name: "x".into(),
                reveal: true,
                apply: true,
                confirm: true,
            })
            .await
            .expect_err("vault.get");
        denied("Vault.Get", e.code());
        let e = vault
            .add(v1::AddSecretReq {
                name: "x".into(),
                provider: 0,
                value: b"v".to_vec(),
                note: String::new(),
                overwrite: false,
                broker_only: false,
            })
            .await
            .expect_err("vault.add");
        denied("Vault.Add", e.code());
    }
    {
        let mut relay = v1::relay_client::RelayClient::new(connect(sock.clone()).await);
        let e = relay
            .mint(v1::MintReq {
                relay: "r".into(),
                ephemeral: true,
                provider: 0,
                ttl_secs: 3600,
                client_pid: 0,
            })
            .await
            .expect_err("relay.mint");
        denied("Relay.Mint", e.code());
        let e = relay
            .revoke(v1::RevokeRelayReq {
                name: "r".into(),
                apply: false,
                confirm: false,
            })
            .await
            .expect_err("relay.revoke");
        denied("Relay.Revoke", e.code());
    }
    {
        let mut audit = v1::audit_client::AuditClient::new(connect(sock.clone()).await);
        let e = audit
            .query(v1::AuditQueryReq {
                actor: None,
                relay: None,
                since: None,
                until: None,
                limit: 0,
            })
            .await
            .expect_err("audit.query");
        denied("Audit.Query", e.code());
    }
    {
        let mut certs = v1::certs_client::CertsClient::new(connect(sock.clone()).await);
        let e = certs
            .list(v1::ListCertReq {})
            .await
            .expect_err("certs.list");
        denied("Certs.List", e.code());
    }

    server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- test 3: conv drift / round-trip (the documented invariant) ------------------------------

#[test]
fn conv_event_to_proto_drift() {
    use envctl_secrets::broker::RelayKind;
    use envctl_secrets::event::{AuditOutcome, RunSummary};
    use envctl_secrets::keyslot::Factor;
    use envctl_secrets::{AuditRecord, SecretEvent, Stream};
    use envctl_secretd::conv;
    use v1::event::Kind;

    // The 11 arms WITH a proto twin map to a Some(Event { kind: Some(_) }) with the expected variant.
    macro_rules! has_twin {
        ($ev:expr, $pat:pat) => {{
            let p = conv::event_to_proto($ev).expect("expected a proto twin");
            assert!(
                matches!(p.kind, Some($pat)),
                "wrong proto Kind for {}",
                stringify!($pat)
            );
        }};
    }

    has_twin!(
        SecretEvent::VaultUnlocked {
            factor: Factor::Usb
        },
        Kind::VaultUnlocked(_)
    );
    has_twin!(SecretEvent::VaultLocked, Kind::VaultLocked(_));
    has_twin!(
        SecretEvent::SecretWritten {
            name: "n".into(),
            version: 1
        },
        Kind::SecretWritten(_)
    );
    has_twin!(
        SecretEvent::RelayMinted {
            relay: "r".into(),
            kind: RelayKind::Ephemeral,
            expires_at: "t".into()
        },
        Kind::RelayMinted(_)
    );
    has_twin!(
        SecretEvent::RelaySwapped {
            relay: "r".into(),
            host: "h".into(),
            method: "GET".into(),
            allowed: true,
            token_id: "tid".into(),
            client_uid: 1000,
            client_label: "lbl".into()
        },
        Kind::RelaySwapped(_)
    );
    has_twin!(
        SecretEvent::GuardRefused {
            subject: "s".into(),
            reason: "r".into()
        },
        Kind::GuardRefused(_)
    );
    has_twin!(
        SecretEvent::CaIssued {
            serial: "s".into(),
            cn: "cn".into(),
            not_after: "na".into()
        },
        Kind::CaIssued(_)
    );
    has_twin!(
        SecretEvent::LeafMinted {
            sni: "sni".into(),
            relay: "r".into(),
            not_after: "na".into()
        },
        Kind::LeafMinted(_)
    );
    has_twin!(
        SecretEvent::Log {
            source: "src".into(),
            stream: Stream::Stderr,
            line: "l".into()
        },
        Kind::Log(_)
    );
    has_twin!(SecretEvent::ChildExited { code: 0 }, Kind::ChildExited(_));
    has_twin!(
        SecretEvent::RunFinished {
            summary: RunSummary::default()
        },
        Kind::RunFinished(_)
    );

    // The 4 twin-less arms (cosmetic mirrors of durable audit rows) map to None and are filtered out
    // of the control stream — never panicking.
    let audit = AuditRecord {
        seq: 1,
        ts: "t".into(),
        actor_uid: Some(1000),
        event_type: "secret_read".into(),
        subject: Some("s".into()),
        detail: serde_json::json!({}),
        outcome: AuditOutcome::Ok,
        prev_hash: vec![0u8; 32],
        row_hash: vec![1u8; 32],
    };
    assert!(conv::event_to_proto(SecretEvent::Audit(audit)).is_none());
    assert!(conv::event_to_proto(SecretEvent::SecretRead {
        name: "n".into(),
        by_uid: 1000
    })
    .is_none());
    assert!(conv::event_to_proto(SecretEvent::RelayRotated {
        relay: "r".into(),
        expires_at: "t".into()
    })
    .is_none());
    assert!(conv::event_to_proto(SecretEvent::RelayRevoked {
        relay: "r".into(),
        reason: "x".into()
    })
    .is_none());

    // The Log stream discriminant must map Stdout->0 / Stderr->1.
    let p = conv::event_to_proto(SecretEvent::Log {
        source: "s".into(),
        stream: Stream::Stdout,
        line: "l".into(),
    })
    .unwrap();
    if let Some(Kind::Log(log)) = p.kind {
        assert_eq!(log.stream, 0, "Stdout must map to 0");
    } else {
        panic!("expected Log kind");
    }

    // mint_req_to_policy: the Phase-6 synthesis. Default-deny by construction (empty host_allow +
    // empty upstream_base), provider carried through, relay name used for both id + secret_name.
    let policy = conv::mint_req_to_policy(&v1::MintReq {
        relay: "eph".into(),
        ephemeral: true,
        provider: v1::ProviderKind::Anthropic as i32,
        ttl_secs: 3600,
        client_pid: 0,
    });
    assert_eq!(policy.relay_id, "eph");
    assert_eq!(policy.secret_name, "eph");
    assert!(matches!(policy.kind, RelayKind::Ephemeral));
    assert!(
        policy.host_allow.is_empty(),
        "Phase-6 minted policy is default-deny on host"
    );

    // add_req_to_meta: name/note/broker_only carried, provider decoded from the wire discriminant.
    let meta = conv::add_req_to_meta(&v1::AddSecretReq {
        name: "n".into(),
        provider: v1::ProviderKind::Openai as i32,
        value: b"v".to_vec(),
        note: "note".into(),
        overwrite: true,
        broker_only: true,
    });
    assert_eq!(meta.name, "n");
    assert_eq!(meta.note, "note");
    assert!(meta.broker_only);
}
