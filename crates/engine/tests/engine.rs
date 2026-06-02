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

/// Regression for the audit BLOCKER: a REFUSED targeted reset must NOT delete the
/// refused target's now-orphaned prerequisites. Chain capp -> cweb -> clib; capp
/// is "live" (detected). `reset cweb` is refused (live capp depends on it) — and
/// clib (cweb's prereq) must be left untouched, not removed.
#[test]
fn reset_refusal_does_not_orphan_remove_prereqs() {
    use envctl_engine::{Engine, EventSink, HookRunner, OpResult, OpStatus, Phase, RunPlan};
    use std::collections::HashSet;

    // a runner where only `capp` detects as present (live).
    struct LiveRunner {
        live: HashSet<String>,
    }
    impl HookRunner for LiveRunner {
        fn run(&self, comp: &str, phase: Phase, _h: &envctl_engine::Hook, _d: bool, _s: &EventSink) -> OpResult {
            let status = if phase == Phase::Detect && self.live.contains(comp) {
                OpStatus::Ok
            } else {
                OpStatus::DryRun
            };
            OpResult { component: comp.into(), phase, status, exit_code: None, duration_ms: 0, message: String::new(), dry_run: false }
        }
    }

    // temp manifest with the 3-component chain.
    let dir = std::env::temp_dir().join(format!(
        "envctl-reset-test-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("chain.toml"),
        r#"
[[component]]
id = "clib"
name = "clib"
[component.detect]
kind = "command"
command = "true"
[component.remove]
kind = "command"
command = "true"

[[component]]
id = "cweb"
name = "cweb"
requires = ["clib"]
[component.detect]
kind = "command"
command = "true"
[component.remove]
kind = "command"
command = "true"

[[component]]
id = "capp"
name = "capp"
requires = ["cweb"]
[component.detect]
kind = "command"
command = "true"
"#,
    )
    .unwrap();

    let runner = LiveRunner { live: HashSet::from(["capp".to_string()]) };
    let eng = Engine::with_runner(dir.clone(), Box::new(runner)).unwrap();
    let sink = EventSink::null();
    let summary = eng
        .run(RunPlan::new(Phase::Remove, vec!["cweb".to_string()], false), &sink)
        .unwrap();

    // cweb refused (live capp depends on it)…
    assert!(summary.refused.iter().any(|x| x == "cweb"), "cweb must be refused: {:?}", summary.refused);
    // …and clib (cweb's orphaned prereq) must NOT have been processed for removal.
    let clib_removed = summary
        .results
        .iter()
        .any(|r| r.component == "clib" && r.phase == Phase::Remove && r.status != OpStatus::NoHook);
    assert!(!clib_removed, "BLOCKER REGRESSION: clib (orphaned prereq) was removed: {:?}", summary.results);
    assert!(!summary.ok());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dropin_filters_injection_in_relinks() {
    use envctl_engine::register::{synth_dropin, RegisterSpec};
    let spec = RegisterSpec {
        id: "x".into(),
        slug: "x".into(),
        display_name: "x".into(),
        source: "https://x/y".into(),
        git_ref: None,
        resolved_sha: "abc123".into(),
        strategy_tag: "as-is".into(),
        build_system: "cargo".into(),
        build_cmd: "cargo build --release".into(),
        transform: None,
        primary_bin: Some("x".into()),
        verify_cmd: None,
        // an unsafe install-name and an unsafe clone-rel path must be filtered out
        relinks: vec![
            ("evil$(touch /tmp/pwn)".into(), "target/release/x".into()),
            ("ok".into(), "bad\"$(x)".into()),
            ("good".into(), "target/release/good".into()),
        ],
        installed_targets: vec![],
    };
    let toml = synth_dropin(&spec);
    assert!(!toml.contains("touch /tmp/pwn"), "unsafe relink NAME must be filtered");
    assert!(!toml.contains("bad\"$(x)"), "unsafe relink REL must be filtered");
    assert!(toml.contains("ln -sfn \"$SRC/target/release/good\" \"$HOME/.local/bin/good\""), "safe relink kept");
    // and the generated TOML still parses (one component with the expected hooks)
    let reg_ok = toml.contains("[[component]]") && toml.contains("[component.install]");
    assert!(reg_ok);
}

#[test]
fn guard_verify_path_uuid_fail_closed() {
    let ctx = RunContext::default();
    // missing path -> refuse
    assert!(envctl_engine::guard::verify_path_uuid("/no/such/envctl-x", "1111", &ctx).is_some());
    // existing path with a bogus UUID that can't resolve -> refuse (never deletes on doubt)
    assert!(envctl_engine::guard::verify_path_uuid("/", "deadbeef-0000-0000", &ctx).is_some());
}

#[test]
fn refused_target_that_is_a_survivors_prereq_is_not_removed() {
    // FOCUS #0 regression: targets {clib, capp} with capp->clib (capp requires
    // clib) and an external LIVE cdep->clib. clib is refused (cdep depends on it,
    // outside the set, no --cascade). Because clib is ALSO in capp's closure, the
    // old code re-added it to `keep` and removed it anyway — orphaning cdep. The
    // fix subtracts refused ids from `keep`: clib must survive, only capp removed.
    use envctl_engine::{Engine, EventSink, HookRunner, OpResult, OpStatus, Phase, RunPlan};
    use std::collections::HashSet;

    struct LiveRunner {
        live: HashSet<String>,
    }
    impl HookRunner for LiveRunner {
        fn run(&self, comp: &str, phase: Phase, _h: &envctl_engine::Hook, _d: bool, _s: &EventSink) -> OpResult {
            let status = if phase == Phase::Detect && self.live.contains(comp) { OpStatus::Ok } else { OpStatus::DryRun };
            OpResult { component: comp.into(), phase, status, exit_code: None, duration_ms: 0, message: String::new(), dry_run: false }
        }
    }

    let dir = std::env::temp_dir().join(format!("envctl-reset-prereq-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("chain.toml"),
        r#"
[[component]]
id = "clib"
name = "clib"
[component.detect]
kind = "command"
command = "true"
[component.remove]
kind = "command"
command = "true"

[[component]]
id = "capp"
name = "capp"
requires = ["clib"]
[component.detect]
kind = "command"
command = "true"
[component.remove]
kind = "command"
command = "true"

[[component]]
id = "cdep"
name = "cdep"
requires = ["clib"]
[component.detect]
kind = "command"
command = "true"
[component.remove]
kind = "command"
command = "true"
"#,
    )
    .unwrap();

    // cdep is the live external reverse-dependent of clib.
    let runner = LiveRunner { live: HashSet::from(["cdep".to_string()]) };
    let eng = Engine::with_runner(dir.clone(), Box::new(runner)).unwrap();
    let sink = EventSink::null();
    let summary = eng
        .run(RunPlan::new(Phase::Remove, vec!["clib".to_string(), "capp".to_string()], false), &sink)
        .unwrap();

    assert!(summary.refused.iter().any(|x| x == "clib"), "clib must be refused: {:?}", summary.refused);
    // a component is "removed" iff its Remove hook actually ran (DryRun/Ok/etc.);
    // a Refused marker is NOT a removal.
    let was_removed = |id: &str| {
        summary.results.iter().any(|r| {
            r.component == id
                && r.phase == Phase::Remove
                && matches!(r.status, OpStatus::DryRun | OpStatus::Ok | OpStatus::Incomplete | OpStatus::Failed)
        })
    };
    assert!(!was_removed("clib"), "BLOCKER REGRESSION: refused clib removed via capp's closure: {:?}", summary.results);
    assert!(was_removed("capp"), "capp (a surviving target) should still be removed: {:?}", summary.results);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn add_repo_paths_reject_traversal_and_option_injection() {
    // Both the build path (Engine::add_repo) and the interactive path
    // (Engine::connect_repo) must run the SAME validation gate BEFORE any path
    // join or git call — AUDIT-FIX for the --connect bypass.
    use envctl_engine::{AddRepoSpec, Engine, EventSink};
    let eng = Engine::with_runner(manifest_dir(), Box::new(envctl_engine::DryRunRunner)).unwrap();
    let sink = EventSink::null();

    // 1. id path-traversal is rejected on the build path.
    let bad_id = AddRepoSpec { id: "../../etc/evil".into(), git_url: "https://example.com/x".into(), ..Default::default() };
    let e = eng.add_repo(bad_id.clone(), true, &sink).unwrap_err().to_string();
    assert!(e.contains("invalid component id"), "build path must reject traversal id: {e}");

    // 2. …and on the interactive connect path (the bug: it used to skip this).
    let e = eng.connect_repo(&bad_id).unwrap_err().to_string();
    assert!(e.contains("invalid component id"), "connect path must reject traversal id: {e}");

    // 3. leading-dash git_ref (git option-injection) is rejected.
    let bad_ref = AddRepoSpec {
        id: "okname".into(),
        git_url: "https://example.com/x".into(),
        git_ref: Some("--upload-pack=evil".into()),
        ..Default::default()
    };
    let e = eng.connect_repo(&bad_ref).unwrap_err().to_string();
    assert!(e.contains("--git-ref") || e.contains("git-ref"), "connect path must reject dash ref: {e}");
}

// --- graph intelligence over the real manifest DAG ---------------------------
#[test]
fn graph_analyze_real_manifest() {
    use envctl_engine::graph;
    let reg = Registry::load(&manifest_dir()).expect("manifest loads");
    let g = graph::analyze(&reg);
    assert!(g.nodes > 0 && g.edges > 0, "non-empty DAG: {}n/{}e", g.nodes, g.edges);
    // rustup has no prerequisites -> it is a root.
    assert!(g.roots.contains(&"rustup".to_string()), "rustup is a root: {:?}", g.roots);
    assert!(!g.critical_path.is_empty(), "a longest chain exists");

    // impact("bun"): its install pulls bun in, and a --cascade reset folds node-via-bun.
    let imp = graph::impact(&reg, "bun").expect("bun exists");
    assert!(imp.install_closure.contains(&"bun".to_string()));
    assert!(imp.cascade_removes.contains(&"node-via-bun".to_string()), "cascade: {:?}", imp.cascade_removes);
    assert!(graph::impact(&reg, "no-such-component").is_none());

    // every root->bun path starts at a root and ends at bun.
    let paths = graph::dependency_paths(&reg, "bun");
    assert!(!paths.is_empty());
    for p in &paths {
        assert_eq!(p.last().map(String::as_str), Some("bun"), "path ends at target: {p:?}");
    }
    // DOT + JSON renderers produce something well-formed.
    assert!(graph::to_dot(&reg, None).contains("digraph"));
    assert!(graph::to_json(&reg, None).get("nodes").is_some());
}

// --- envctl.lock generate -> save -> load -> diff is clean -------------------
#[test]
fn lock_roundtrip_no_drift_against_self() {
    use envctl_engine::lock;
    let reg = Registry::load(&manifest_dir()).expect("manifest loads");
    let tmp = std::env::temp_dir().join(format!("envctl-lock-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let mut lf = lock::generate(&reg);
    lf.save(&tmp).expect("save lock");
    let loaded = lock::LockFile::load(&tmp).expect("load lock");
    // a lock generated from a registry has zero drift against that same registry.
    assert!(lock::diff(&reg, &loaded).is_empty(), "freshly-generated lock must not drift");

    let _ = std::fs::remove_dir_all(&tmp);
}

// --- machine-local runtime last-run state round-trips through XDG cache ------
#[test]
fn runtime_record_and_load_roundtrip() {
    use envctl_engine::{runtime, Phase, RunSummary};
    let base = std::env::temp_dir().join(format!("envctl-rt-test-{}", std::process::id()));
    let cache = base.join("cache");
    let manifest = base.join("manifest");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(&manifest).unwrap();
    std::env::set_var("XDG_CACHE_HOME", &cache);

    // a half-failed install: one failure recorded as not-ok.
    let mut s = RunSummary::default();
    s.failed.push("bun".into());
    runtime::record_run(&manifest, Phase::Install, &s);

    let st = runtime::load(&manifest).last_run.expect("a run was recorded");
    assert_eq!(st.verb, "install");
    assert_eq!(st.failed, 1);
    assert!(!st.ok, "a run with a failure is not ok");
    assert!(!st.at.is_empty(), "timestamp recorded");

    std::env::remove_var("XDG_CACHE_HOME");
    let _ = std::fs::remove_dir_all(&base);
}
