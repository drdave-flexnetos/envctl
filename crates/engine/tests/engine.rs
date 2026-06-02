//! Phase-0/1 contract tests: the manifest loads + topo-sorts, the internally-tagged
//! Hook/Guard enums round-trip through toml, the guard engine fails CLOSED, and
//! drift flags missing/unhealthy components with the right suggested verbs.
use envctl_engine::{
    Component, ComponentState, DriftKind, EnvReport, Guard, Hook, Registry, RunContext,
};
use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../manifest")
}

#[derive(serde::Deserialize)]
struct Wrap {
    #[serde(default)]
    component: Vec<Component>,
}

#[test]
fn manifest_loads_and_topo_sorts() {
    let reg = Registry::load(&manifest_dir()).expect("manifest dir loads");
    let ids: Vec<String> = reg.ids().cloned().collect();
    for want in ["rustup", "bun", "cuda-toolkit", "yazelix-shell", "boot-repair-dev"] {
        assert!(ids.contains(&want.to_string()), "missing component {want}: {ids:?}");
    }
    let pos = |id: &str| ids.iter().position(|x| x == id).unwrap();
    assert!(pos("rustup") < pos("cuda-toolkit"), "rustup must precede cuda-toolkit");
    assert!(pos("bun") < pos("yazelix-shell"), "bun must precede yazelix-shell");
}

#[test]
fn tagged_enums_round_trip_through_toml() {
    // Exercises every Hook kind and every Guard kind (the brittle serde+toml seam).
    let src = r#"
        [[component]]
        id = "demo"
        name = "demo"
        destructive = true

        [component.detect]
        kind = "command"
        command = "bash"
        args = ["-lc", "true"]

        [component.install]
        kind = "script"
        login_shell = true
        script = "echo hi"

        [component.fix]
        kind = "script"
        path = "/tmp/demo.sh"

        [component.verify]
        kind = "shipped_script"
        path = "/usr/local/bin/demo.sh"
        args = ["check"]

        [[component.guards]]
        kind = "uuid_resolves"
        uuid = "1111"
        [[component.guards]]
        kind = "not_live_device"
        uuid = "1111"
        [[component.guards]]
        kind = "not_mounted"
        uuid = "2222"
        [[component.guards]]
        kind = "path_exists"
        path = "/tmp"
        [[component.guards]]
        kind = "hook_succeeds"
        [component.guards.hook]
        kind = "command"
        command = "true"
    "#;
    let w: Wrap = toml::from_str(src).expect("tagged enums deserialize from toml");
    let c = &w.component[0];
    assert!(matches!(c.detect, Some(Hook::Command { .. })));
    assert!(matches!(c.install, Some(Hook::Script { .. })));
    assert!(matches!(c.verify, Some(Hook::ShippedScript { .. })));
    assert_eq!(c.guards.len(), 5);
    assert!(matches!(c.guards[4], Guard::HookSucceeds { .. }));
}

#[test]
fn guards_fail_closed() {
    let runner = envctl_engine::DryRunRunner;
    let ctx = RunContext::default();

    // A UUID that cannot resolve must REFUSE (never silent-pass).
    let bogus = vec![Guard::UuidResolves {
        uuid: "deadbeef-0000-0000-0000-000000000000".into(),
    }];
    assert!(
        envctl_engine::guard::check_guards(&bogus, &runner, &ctx).is_some(),
        "an unresolvable UUID guard must refuse"
    );

    // A missing required path must REFUSE.
    let missing = vec![Guard::PathExists {
        path: "/no/such/path/envctl-test".into(),
    }];
    assert!(
        envctl_engine::guard::check_guards(&missing, &runner, &ctx).is_some(),
        "a missing PathExists guard must refuse"
    );

    // An empty guard set passes (nothing to refuse).
    assert!(envctl_engine::guard::check_guards(&[], &runner, &ctx).is_none());
}

#[test]
fn drift_flags_missing_and_unhealthy() {
    let reg = Registry::load(&manifest_dir()).expect("manifest loads");
    let st = |id: &str, detected: bool, healthy: Option<bool>| ComponentState {
        id: id.into(),
        name: id.into(),
        detected,
        healthy,
        wiring_present: false,
        note: String::new(),
    };
    let report = EnvReport {
        gpu_present: true,
        driver_loaded: false, // → DriverInactive drift
        components: vec![
            st("bun", false, None),          // → Missing (has install hook)
            st("rustup", true, Some(false)), // → Unhealthy
            st("uv", true, Some(true)),      // → no drift
        ],
        ..Default::default()
    };
    let drift = envctl_engine::drift::compute(&report, &reg);

    let kind_for = |c: &str| drift.iter().find(|d| d.component == c).map(|d| d.kind);
    assert_eq!(kind_for("bun"), Some(DriftKind::Missing));
    assert_eq!(kind_for("rustup"), Some(DriftKind::Unhealthy));
    assert!(drift.iter().any(|d| d.kind == DriftKind::DriverInactive));
    assert!(!drift.iter().any(|d| d.component == "uv"), "healthy uv must not drift");
    // suggested verb points at the right command
    let bun = drift.iter().find(|d| d.component == "bun").unwrap();
    assert!(bun.suggested_verb.contains("install bun"));
}
