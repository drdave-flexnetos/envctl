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

// ---- Phase 3 ----

#[test]
fn runsummary_ok_counts_incomplete() {
    let mut s = envctl_engine::RunSummary::default();
    assert!(s.ok());
    s.incomplete.push("x".into());
    assert!(!s.ok(), "an incomplete (acted-but-post-state-wrong) run is NOT ok");
}

#[test]
fn untargeted_reset_refused_without_all_confirm() {
    let eng = envctl_engine::Engine::with_runner(manifest_dir(), Box::new(envctl_engine::DryRunRunner))
        .expect("engine loads");
    let sink = envctl_engine::EventSink::null();
    // Whole-roster reset (no targets) with no gates -> one synthetic Refused, early return.
    let s = eng
        .run(envctl_engine::RunPlan::new(envctl_engine::Phase::Remove, vec![], false), &sink)
        .expect("run ok");
    assert!(s.refused.iter().any(|x| x == "<reset>"), "must refuse: {:?}", s.refused);
    assert!(!s.ok());
}

#[test]
fn reverse_dependents_transitive() {
    let reg = Registry::load(&manifest_dir()).expect("manifest loads");
    let rdeps: Vec<String> = reg.reverse_dependents("bun").iter().map(|c| c.id.clone()).collect();
    assert!(rdeps.contains(&"node-via-bun".to_string()), "direct: {rdeps:?}");
    assert!(rdeps.contains(&"group-ai-clis".to_string()), "transitive (via node-via-bun): {rdeps:?}");
}

#[test]
fn manifest_phase3_wiring_loads() {
    let reg = Registry::load(&manifest_dir()).expect("manifest loads");
    assert!(!reg.get("gh").unwrap().wiring.apt_repos.is_empty());
    assert!(!reg.get("nix-yazelix-cache").unwrap().wiring.nix_conf_lines.is_empty());
    assert!(!reg.get("ghostty-default-terminal").unwrap().wiring.alternatives.is_empty());
    let nct = reg.get("nvidia-container-toolkit").unwrap();
    assert!(!nct.wiring.apt_repos.is_empty() && !nct.wiring.cdi_specs.is_empty());
}

#[test]
fn guard_verify_path_uuid_fail_closed() {
    let ctx = RunContext::default();
    // missing path -> refuse
    assert!(envctl_engine::guard::verify_path_uuid("/no/such/envctl-x", "1111", &ctx).is_some());
    // existing path with a bogus UUID that can't resolve -> refuse (never deletes on doubt)
    assert!(envctl_engine::guard::verify_path_uuid("/", "deadbeef-0000-0000", &ctx).is_some());
}
