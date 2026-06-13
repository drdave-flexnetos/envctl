//! Phase-8 data-plane (PR-1) acceptance tests for the auto-injection seam, driven SYNC through the
//! PUBLIC `Engine::run_child` API (no tokio — `run_child` is a pure-sync primitive). Mirrors the
//! `engine()` construction pattern in `tests/relay.rs`.
//!
//! Proves end to end:
//! - the relay BEARER (never a real key) reaches the child env: `echo $ANTHROPIC_API_KEY` ⇒ Log==bearer;
//! - the child's true exit code is returned (`sh -c 'exit 7'` ⇒ 7) with a `ChildExited{7}` event;
//! - `run_child` refuses an empty argv and an unresolvable program (durable `Refused` + `GuardRefused`).
use std::sync::mpsc::Receiver;

use envctl_secrets::broker::Provider;
use envctl_secrets::inject::{injection_template, ChildEnvPlan, DataPlaneMode};
use envctl_secrets::paths::Paths;
use envctl_secrets::seam::{Clock, NoMint, UpstreamError, UsbProbe};
use envctl_secrets::vault::{InMemStore, Store};
use envctl_secrets::{EgressReq, EgressResp, Engine, EventSink, SecretEvent, Upstream};
use zeroize::Zeroizing;

// ---- minimal fakes ---------------------------------------------------------------------------

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(1_700_000_000_000).unwrap()
    }
    fn boottime_ms(&self) -> i64 {
        1_700_000_000_000
    }
}

struct AbsentUsb;
impl UsbProbe for AbsentUsb {
    fn keyfile_for(&self, _uuid: &str) -> Option<Zeroizing<Vec<u8>>> {
        None
    }
}

#[derive(Clone)]
struct NoopUpstream;
#[async_trait::async_trait]
impl Upstream for NoopUpstream {
    async fn send(
        &self,
        _req: EgressReq,
        _real_key: &Zeroizing<Vec<u8>>,
    ) -> Result<EgressResp, UpstreamError> {
        Ok(EgressResp {
            status: 200,
            headers: Vec::new(),
            allowed: true,
        })
    }
}

fn engine() -> Engine {
    Engine::with_seams(
        Paths::under(std::path::PathBuf::from("/tmp/env-ctl-test-inject")),
        Box::new(InMemStore::new()) as Box<dyn Store>,
        Box::new(FixedClock),
        Box::new(AbsentUsb),
        Box::new(NoMint),
        Box::new(NoopUpstream),
    )
    .expect("with_seams must construct")
}

fn drain(rx: &Receiver<SecretEvent>) -> Vec<SecretEvent> {
    rx.try_iter().collect()
}

fn plan(provider: Provider, bearer: &str, mode: DataPlaneMode) -> ChildEnvPlan {
    let injection = injection_template(
        provider,
        bearer,
        "http://127.0.0.1:9443",
        "/tmp/ca.pem",
        mode,
    );
    ChildEnvPlan {
        injection,
        child_pid_hint: None,
    }
}

const BEARER: &str = "relay-bearer-DO-NOT-LEAK";
const REAL_KEY: &str = "sk-ant-REAL-VAULT-KEY-0000";

// ---- tests -----------------------------------------------------------------------------------

#[test]
fn run_child_overlays_bearer_into_child_env_and_logs_it() {
    let eng = engine();
    let (sink, rx) = EventSink::channel();
    let p = plan(Provider::Anthropic, BEARER, DataPlaneMode::BaseUrlRepoint);
    // base_url is the proxy for repoint.
    assert_eq!(
        p.injection.base_url.as_deref(),
        Some("http://127.0.0.1:9443")
    );

    let argv = vec![
        "sh".to_string(),
        "-c".to_string(),
        "echo $ANTHROPIC_API_KEY".to_string(),
    ];
    let code = eng.run_child(p, argv, &sink).expect("run_child ok");
    assert_eq!(code, 0);

    let events = drain(&rx);
    let logged: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            SecretEvent::Log { line, .. } => Some(line.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        logged.contains(&BEARER),
        "child must see the bearer on stdout; got {logged:?}"
    );
    // The real key must NEVER appear anywhere in the emitted events.
    for e in &events {
        if let SecretEvent::Log { line, .. } = e {
            assert_ne!(line, REAL_KEY);
        }
    }
    assert!(events
        .iter()
        .any(|e| matches!(e, SecretEvent::ChildExited { code: 0 })));
    assert!(events
        .iter()
        .any(|e| matches!(e, SecretEvent::RunFinished { .. })));
}

#[test]
fn run_child_returns_true_exit_code() {
    let eng = engine();
    let (sink, rx) = EventSink::channel();
    let p = plan(Provider::Generic, BEARER, DataPlaneMode::HttpsProxyMitm);
    let argv = vec!["sh".to_string(), "-c".to_string(), "exit 7".to_string()];
    let code = eng.run_child(p, argv, &sink).expect("run_child ok");
    assert_eq!(code, 7);
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, SecretEvent::ChildExited { code: 7 })));
}

#[test]
fn run_child_refuses_empty_argv() {
    let eng = engine();
    let (sink, rx) = EventSink::channel();
    let p = plan(Provider::Anthropic, BEARER, DataPlaneMode::BaseUrlRepoint);
    let res = eng.run_child(p, vec![], &sink);
    assert!(res.is_err(), "empty argv must be refused");
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SecretEvent::GuardRefused { .. })),
        "must emit GuardRefused"
    );
}

#[test]
fn run_child_refuses_unresolvable_program() {
    let eng = engine();
    let (sink, rx) = EventSink::channel();
    let p = plan(Provider::Anthropic, BEARER, DataPlaneMode::BaseUrlRepoint);
    let argv = vec!["envctl-definitely-not-a-real-binary-xyz".to_string()];
    let res = eng.run_child(p, argv, &sink);
    assert!(res.is_err(), "unresolvable program must be refused");
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, SecretEvent::GuardRefused { .. })));
}
