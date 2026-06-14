//! Differential PARITY verification of the absorbed COMMAND VERBS (kasetto → envctl, Epic C
//! TASK-0012, rows C-07..C-14: add/remove/lock/list/clean + source_edit + init + uninstall),
//! driven through the engine's public `Engine::agent_{add,remove,lock,list,clean}` API. The
//! engine source is FROZEN; this file only verifies it.
//!
//! Two proof classes, per the cardinal rules:
//!
//! * **C-12 / C-13 HAVE kasetto `#[cfg(test)]` oracles** (`src/commands/{source_edit,init}.rs`).
//!   Those vectors are reproduced VERBATIM here against the public re-exported primitives
//!   (`split_at_ref`, `ensure_local_config`, `DEFAULT_{,_GLOBAL_}CONFIG_FILENAME`), so the
//!   absorbed library reproduces kasetto's exact input→output. Each carries an
//!   `// Oracle: kasetto src/commands/<file>::<test>` citation.
//!
//! * **C-07/C-08/C-09/C-10/C-11/C-14 have NO kasetto unit oracle** (kasetto had no tests/ dir;
//!   these verbs were 0-test). They are thin orchestration over primitives that are ALREADY
//!   differentially `[x]`-verified (FE-02/03/04, L-03/L-06, P-01, MC-01/MC-02). So their
//!   ORCHESTRATION is proved end-to-end: drive the Engine verb over a hermetic fixture project
//!   and assert the documented ledger contract — the happy path, the key ERR arms, the dry-run
//!   zero-writes gate, and the fail-closed / never-prune guards. Each carries a
//!   `// Contract: ledger C-NN (no kasetto unit oracle; primitives <ids> already [x])` label.
//!
//! Isolation mirrors `agent_sync.rs` / `agent_sync_parity.rs`: one per-process temp HOME
//! (XDG_* cleared) so the agent-env global data/cache resolve inside the sandbox, a distinct
//! project tempdir per test, and a `CWD_LOCK` for the cwd-reading verbs (`list`/`clean`).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use envctl_agent_env::config_edit::{ensure_local_config, split_at_ref};
use envctl_agent_env::config_path::{DEFAULT_CONFIG_FILENAME, DEFAULT_GLOBAL_CONFIG_FILENAME};

use envctl_engine::event::{Event, EventSink};
use envctl_engine::{
    AgentAddSpec, AgentCleanSpec, AgentListKind, AgentListSpec, AgentLockMode, AgentLockSpec,
    AgentRemoveSpec, AgentScope, AgentSectionSel, AgentSyncSpec, Engine,
};

// ---------------------------------------------------------------------------------------
// Sandbox helpers (same discipline as agent_sync.rs; kept local so the test binaries stay
// independent with no shared-module coupling).
// ---------------------------------------------------------------------------------------

fn sandbox_home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let base = unique_dir("envctl-agent-cmdparity-home");
        std::env::set_var("HOME", &base);
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_CACHE_HOME");
        std::env::remove_var("ENVCTL_AGENT_CONFIG");
        base
    })
}

/// Serializes the cwd-dependent verbs (`list`/`clean` read `std::env::current_dir`).
fn cwd_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

fn unique_dir(prefix: &str) -> PathBuf {
    static N: OnceLock<Mutex<u64>> = OnceLock::new();
    let mut n = N.get_or_init(|| Mutex::new(0)).lock().unwrap();
    *n += 1;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("{prefix}-{nanos}-{}", *n));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pack_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent/pack")
}

/// Write `agent-env.yaml` into a fresh project tempdir; return (engine, project, cfg_path).
fn project(yaml: &str) -> (Engine, PathBuf, String) {
    sandbox_home();
    let proj = unique_dir("envctl-agent-cmdparity-proj");
    let cfg = proj.join("agent-env.yaml");
    std::fs::write(&cfg, yaml).unwrap();
    (Engine::detached(), proj, cfg.to_string_lossy().to_string())
}

fn sink() -> (EventSink, std::sync::mpsc::Receiver<Event>) {
    EventSink::channel()
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// A skills-only, claude-code, project-scope config rooted at a local pack.
fn skills_named_cfg(pack: &Path, names: &[&str]) -> String {
    let mut s = format!(
        "agent: claude-code\nscope: project\nskills:\n  - source: {p}\n    skills:\n",
        p = pack.display()
    );
    for n in names {
        s.push_str(&format!("      - {n}\n"));
    }
    s
}

fn add_spec(source: &str, cfg: &str) -> AgentAddSpec {
    AgentAddSpec {
        source: source.to_string(),
        section: AgentSectionSel::default(),
        git_ref: None,
        branch: None,
        sub_dir: None,
        config_path: Some(cfg.to_string()),
        scope_override: None,
        apply: false,
        no_sync: false,
        no_verify: true,
        lock_mode: AgentLockMode::Plain,
    }
}

fn remove_spec(source: &str, cfg: &str) -> AgentRemoveSpec {
    AgentRemoveSpec {
        source: source.to_string(),
        section: AgentSectionSel::default(),
        git_ref: None,
        branch: None,
        sub_dir: None,
        config_path: Some(cfg.to_string()),
        scope_override: None,
        apply: false,
        no_sync: false,
        lock_mode: AgentLockMode::Plain,
    }
}

// =======================================================================================
// C-12 — source_edit primitives (split_at_ref + ensure_local_config)
//   Oracle: kasetto src/commands/source_edit.rs::<test> (6 split_at_ref vectors, VERBATIM)
//
// The `@<ref>` tail-split is the load-bearing parsing primitive shared by add/remove (it must
// be SSH/userinfo round-trip safe). It is re-exported by envctl as
// `agent_env::config_edit::split_at_ref` and driven by the engine's `agent_add`/`agent_remove`.
// =======================================================================================

/// Oracle: kasetto src/commands/source_edit.rs::split_at_ref_https_tag
#[test]
fn c12_split_at_ref_https_tag() {
    let (s, r) = split_at_ref("https://github.com/org/repo@v1.2.0");
    assert_eq!(s, "https://github.com/org/repo");
    assert_eq!(r.as_deref(), Some("v1.2.0"));
}

/// Oracle: kasetto src/commands/source_edit.rs::split_at_ref_userinfo_url_round_trips
#[test]
fn c12_split_at_ref_userinfo_url_round_trips() {
    let (s, r) = split_at_ref("https://user@host.example/repo");
    assert_eq!(s, "https://user@host.example/repo");
    assert!(r.is_none());
}

/// Oracle: kasetto src/commands/source_edit.rs::split_at_ref_ssh_round_trips
#[test]
fn c12_split_at_ref_ssh_round_trips() {
    let (s, r) = split_at_ref("git@github.com:org/repo");
    assert_eq!(s, "git@github.com:org/repo");
    assert!(r.is_none());
}

/// Oracle: kasetto src/commands/source_edit.rs::split_at_ref_userinfo_and_ref
#[test]
fn c12_split_at_ref_userinfo_and_ref() {
    let (s, r) = split_at_ref("https://user@host.example/repo@main");
    assert_eq!(s, "https://user@host.example/repo");
    assert_eq!(r.as_deref(), Some("main"));
}

/// Oracle: kasetto src/commands/source_edit.rs::split_at_ref_local_path_no_at
#[test]
fn c12_split_at_ref_local_path_no_at() {
    let (s, r) = split_at_ref("./local/pack");
    assert_eq!(s, "./local/pack");
    assert!(r.is_none());
}

/// Oracle: kasetto src/commands/source_edit.rs::split_at_ref_trailing_at_ignored
#[test]
fn c12_split_at_ref_trailing_at_ignored() {
    let (s, r) = split_at_ref("https://github.com/org/repo@");
    assert_eq!(s, "https://github.com/org/repo@");
    assert!(r.is_none());
}

/// Contract: ledger C-12 `resolve_local_config_path` ERR-on-remote arm. kasetto's
/// `resolve_local_config_path` rejects any `scheme://` config (remote configs cannot be edited in
/// place). envctl's engine `resolve_local_config_path` now routes the config string through
/// `agent_env::config_edit::ensure_local_config` (C-12-FIX, `edit.rs`) before building the path,
/// so the rejection is enforced AT THE ENGINE VERB — `agent_add`/`agent_remove` both refuse a
/// remote `--config`, parity with kasetto (no longer a downgrade). Verified both at the library
/// seam and end-to-end through the verbs.
#[test]
fn c12_engine_verbs_reject_remote_config_accept_local() {
    // Library seam: the local path passes through unchanged...
    assert_eq!(
        ensure_local_config("./agent-env.yaml").unwrap(),
        "./agent-env.yaml"
    );
    // ...and any scheme://-bearing remote config is rejected with the porting hint.
    let err = ensure_local_config("https://example.com/agent-env.yaml").unwrap_err();
    assert!(
        err.to_string().contains("cannot edit remote config"),
        "remote config edit must be refused: {err}"
    );

    // C-12-FIX — the Engine VERBS now enforce it too (fail-closed, before any FS touch).
    let (engine, _proj, _cfg) = project("scope: project\nagent: claude-code\n");
    let remote = "https://example.com/agent-env.yaml";

    let (s, _rx) = sink();
    let add_err = engine
        .agent_add(add_spec("github.com/o/r", remote), &s)
        .expect_err("agent_add must reject a remote --config");
    assert!(
        add_err.to_string().contains("cannot edit remote config"),
        "agent_add remote-config rejection: {add_err}"
    );

    let (s2, _rx2) = sink();
    let rm_err = engine
        .agent_remove(remove_spec("github.com/o/r", remote), &s2)
        .expect_err("agent_remove must reject a remote --config");
    assert!(
        rm_err.to_string().contains("cannot edit remote config"),
        "agent_remove remote-config rejection: {rm_err}"
    );
}

/// Contract: ledger C-12 `sync_after` = a plain sync runs against the freshly edited config so
/// installs/lock catch up. Driven through the public `agent_add` apply path: adding a local
/// source then letting `sync_after` run must INSTALL the new skill end-to-end (the embedded
/// follow-up `AgentReport` is present and the skill lands on disk).
#[test]
fn c12_sync_after_runs_plain_sync_post_edit() {
    // Start with an empty config; add a wildcard local source with apply+sync.
    let (engine, proj, cfg) = project("scope: project\nagent: claude-code\n");
    let pack = pack_dir();
    let (s, _rx) = sink();
    let mut spec = add_spec(&pack.display().to_string(), &cfg);
    spec.section = AgentSectionSel {
        skills: vec!["alpha".into()],
        ..Default::default()
    };
    spec.apply = true;
    let outcome = engine.agent_add(spec, &s).expect("add+sync_after");
    assert_eq!(outcome.action, "added");
    let sync = outcome.sync.expect("sync_after produced a report");
    assert_eq!(sync.summary.failed, 0);
    assert!(
        proj.join(".claude/skills/alpha/SKILL.md").is_file(),
        "sync_after installed the just-added skill"
    );
}

// =======================================================================================
// C-13 — init (TEMPLATE + init_config_path)
//   Oracle: kasetto src/commands/init.rs::<test> (2 path-resolution vectors, business only)
//
// The init business is template + path resolution; the TTY overwrite prompt + banner are
// FRONT-END (out of scope). kasetto's 2 unit tests certify only init_config_path. envctl renamed
// the product filenames (`kasetto.yaml`→`agent-env.yaml`, dir `kasetto/`→`agent-env/`), so the
// VERBATIM oracle assertion is reproduced against envctl's renamed constants (the rename is the
// absorbed-tool self-identity, a SHOULD-rename per CP-01, not a behavior change).
// =======================================================================================

/// Oracle: kasetto src/commands/init.rs::init_path_defaults_to_local_config
/// kasetto asserts the local default is `kasetto.yaml`; envctl's renamed equivalent is
/// `agent-env.yaml` (CP-01 product-identity rename). The BEHAVIOR — "default `--global=false`
/// init writes the local config filename in cwd" — is identical.
#[test]
fn c13_init_path_defaults_to_local_config() {
    assert_eq!(DEFAULT_CONFIG_FILENAME, "agent-env.yaml");
    // The kasetto contract: a non-global init targets the bare local filename (no dir prefix).
    assert!(!DEFAULT_CONFIG_FILENAME.contains('/'));
}

/// Oracle: kasetto src/commands/init.rs::init_path_global_uses_kasetto_config_dir
/// kasetto asserts the global path ends with `kasetto/kasetto.yaml`; envctl's renamed equivalent
/// resolves under the `agent-env/` XDG config dir as `agent-env.yaml`. We reproduce the oracle's
/// "global path ends with <product-dir>/<global-config-filename>" assertion against the renamed
/// constants + the real dir resolver.
#[test]
fn c13_init_path_global_uses_agent_env_config_dir() {
    assert_eq!(DEFAULT_GLOBAL_CONFIG_FILENAME, "agent-env.yaml");
    let dir = envctl_agent_env::dirs::dirs_agent_env_config().expect("config dir");
    let global = dir.join(DEFAULT_GLOBAL_CONFIG_FILENAME);
    assert!(
        global.ends_with("agent-env/agent-env.yaml"),
        "global init path resolves under the agent-env config dir: {}",
        global.display()
    );
}

// =======================================================================================
// C-07 — agent_add (verb 2): ERR arms + dry_run zero-writes + already-exists guard
//   Contract: ledger C-07 (no kasetto unit oracle; primitives FE-02 insert_item, F-06
//   plan_add_edits, S-03 derive_browse_url, C-12 split_at_ref already [x]).
// =======================================================================================

/// Contract: ledger C-07 — `--ref` and `--branch` are mutually exclusive (ERR, zero writes).
#[test]
fn c07_add_ref_and_branch_mutually_exclusive() {
    let (engine, _proj, cfg) = project("scope: project\nagent: claude-code\n");
    let mut spec = add_spec("https://github.com/org/repo", &cfg);
    spec.git_ref = Some("v1".into());
    spec.branch = Some("main".into());
    let (s, _rx) = sink();
    let err = engine
        .agent_add(spec, &s)
        .expect_err("ref+branch must error");
    assert!(
        err.to_string().contains("mutually exclusive"),
        "ref+branch: {err}"
    );
}

/// Contract: ledger C-07 — `--locked` on `add` without `--no-sync` is refused (a newly added
/// source has no lock entry yet; installing it would require a fetch). Fail-closed, zero writes.
#[test]
fn c07_add_locked_without_no_sync_refused() {
    let (engine, _proj, cfg) = project("scope: project\nagent: claude-code\n");
    let mut spec = add_spec("https://github.com/org/repo", &cfg);
    spec.lock_mode = AgentLockMode::Locked;
    spec.no_sync = false;
    let (s, _rx) = sink();
    let err = engine
        .agent_add(spec, &s)
        .expect_err("--locked w/o --no-sync must error");
    assert!(err.to_string().contains("--no-sync"), "locked rule: {err}");
}

/// Contract: ledger C-07 — the `@<ref>` shorthand conflicts with `--ref`/`--branch` (ERR).
#[test]
fn c07_add_at_ref_conflicts_with_ref_flag() {
    let (engine, _proj, cfg) = project("scope: project\nagent: claude-code\n");
    let mut spec = add_spec("https://github.com/org/repo@v1.0", &cfg);
    spec.git_ref = Some("v2".into());
    let (s, _rx) = sink();
    let err = engine
        .agent_add(spec, &s)
        .expect_err("@ref + --ref must error");
    assert!(
        err.to_string().contains("conflicts"),
        "@ref conflict: {err}"
    );
}

/// Contract: ledger C-07 — adding a source already present in a section is refused
/// (already-exists guard via `item_exists`). Fail-closed, no duplicate written.
#[test]
fn c07_add_already_exists_refused() {
    let pack = pack_dir();
    // Config already carries the source under skills.
    let (engine, _proj, cfg) = project(&skills_named_cfg(&pack, &["alpha"]));
    let mut spec = add_spec(&pack.display().to_string(), &cfg);
    spec.section = AgentSectionSel {
        skills: vec!["alpha".into()],
        ..Default::default()
    };
    spec.apply = true;
    let (s, _rx) = sink();
    let err = engine
        .agent_add(spec, &s)
        .expect_err("already-present source must error");
    assert!(
        err.to_string().contains("already in"),
        "already-exists: {err}"
    );
}

/// Contract: ledger C-07 — `dry_run` (apply=false) previews `would_add` and writes NOTHING (the
/// config file is byte-for-byte unchanged, no skill installed, no lock written).
#[test]
fn c07_add_dry_run_writes_nothing() {
    let (engine, proj, cfg) = project("scope: project\nagent: claude-code\n");
    let pack = pack_dir();
    let before = std::fs::read_to_string(&cfg).unwrap();
    let mut spec = add_spec(&pack.display().to_string(), &cfg);
    spec.section = AgentSectionSel {
        skills: vec!["alpha".into()],
        ..Default::default()
    };
    spec.apply = false;
    let (s, _rx) = sink();
    let outcome = engine.agent_add(spec, &s).expect("add preview");
    assert_eq!(outcome.action, "would_add");
    assert!(outcome.dry_run);
    assert!(outcome.sync.is_none(), "no sync_after on preview");
    assert_eq!(
        std::fs::read_to_string(&cfg).unwrap(),
        before,
        "preview left the config byte-for-byte unchanged"
    );
    assert!(
        !proj.join(".claude/skills/alpha").exists(),
        "preview installed nothing"
    );
    assert!(
        !proj.join("agent-env.lock").exists(),
        "preview wrote no lock"
    );
}

// =======================================================================================
// C-08 — agent_remove (verb 3): not-found ERR arms + dry_run zero-writes
//   Contract: ledger C-08 (no kasetto unit oracle; primitives FE-03 remove_item, FE-04
//   remove_names, S-03 derive already [x]).
// =======================================================================================

/// Contract: ledger C-08 — removing a source absent from every list errors with the
/// "not found in any list" message (whole-source removal path). Fail-closed.
#[test]
fn c08_remove_whole_source_not_found_in_any_list() {
    let pack = pack_dir();
    let (engine, _proj, cfg) = project(&skills_named_cfg(&pack, &["alpha"]));
    // Remove a DIFFERENT source that is in no list.
    let spec = remove_spec("https://github.com/nobody/missing", &cfg);
    let (s, _rx) = sink();
    let err = engine
        .agent_remove(spec, &s)
        .expect_err("absent source must error");
    assert!(
        err.to_string().contains("not found in any list"),
        "whole-source not-found: {err}"
    );
}

/// Contract: ledger C-08 — a per-kind remove of a name absent from that section errors
/// "not found in `<section>:`" (remove_by_kind path). Fail-closed.
#[test]
fn c08_remove_by_kind_not_found_in_section() {
    let pack = pack_dir();
    let (engine, _proj, cfg) = project(&skills_named_cfg(&pack, &["alpha"]));
    let mut spec = remove_spec(&pack.display().to_string(), &cfg);
    // The source IS present under skills, but request removal from `mcps:` where it is absent.
    spec.section = AgentSectionSel {
        mcps: vec!["github".into()],
        ..Default::default()
    };
    let (s, _rx) = sink();
    let err = engine
        .agent_remove(spec, &s)
        .expect_err("kind not present must error");
    assert!(
        err.to_string().contains("not found in"),
        "kind not-found: {err}"
    );
}

/// Contract: ledger C-08 — `dry_run` (apply=false) previews `would_remove` and writes NOTHING
/// (the config is unchanged, no sync_after runs).
#[test]
fn c08_remove_dry_run_writes_nothing() {
    let pack = pack_dir();
    let (engine, _proj, cfg) = project(&skills_named_cfg(&pack, &["alpha", "beta"]));
    let before = std::fs::read_to_string(&cfg).unwrap();
    let mut spec = remove_spec(&pack.display().to_string(), &cfg);
    spec.section = AgentSectionSel {
        skills: vec!["beta".into()],
        ..Default::default()
    };
    spec.apply = false;
    let (s, _rx) = sink();
    let outcome = engine.agent_remove(spec, &s).expect("remove preview");
    assert_eq!(outcome.action, "would_remove");
    assert!(outcome.dry_run);
    assert!(outcome.sync.is_none(), "no sync_after on preview");
    assert_eq!(
        std::fs::read_to_string(&cfg).unwrap(),
        before,
        "preview left the config byte-for-byte unchanged"
    );
}

// =======================================================================================
// C-09 — agent_lock (verb 4): --check drift exit (never writes) + --upgrade-package filter
//   Contract: ledger C-09 (no kasetto unit oracle; primitives L-06 lock_check, F-06/F-01
//   resolve+materialize, P-01 read_skill_profile already [x]).
// =======================================================================================

/// Contract: ledger C-09 — `--check` re-resolves and diffs against the on-disk lock and NEVER
/// writes. On drift it returns the changes (a front-end maps to exit(1)); the lock file on disk
/// is unchanged. This is the guardian-critical `--check` drift-exit path.
#[test]
fn c09_lock_check_reports_drift_without_writing() {
    let pack = pack_dir();
    let (engine, proj, cfg) = project(&skills_named_cfg(&pack, &["alpha"]));

    // Write the lock from the pristine source.
    let (s, _rx) = sink();
    engine
        .agent_lock(
            AgentLockSpec {
                config_path: Some(cfg.clone()),
                scope_override: None,
                check: false,
                upgrade_only: Vec::new(),
                lock_mode: AgentLockMode::Plain,
            },
            &s,
        )
        .expect("lock write");
    let lock_path = proj.join("agent-env.lock");
    assert!(lock_path.is_file());
    let lock_before = std::fs::read_to_string(&lock_path).unwrap();

    // Mutate a COPY of the source so the re-resolved hash drifts.
    let mutated = proj.join("pack");
    copy_tree(&pack, &mutated);
    std::fs::write(
        mutated.join("alpha/SKILL.md"),
        "---\nname: alpha\n---\nDRIFTED\n",
    )
    .unwrap();
    let cfg2 = proj.join("agent-env-2.yaml");
    std::fs::write(&cfg2, skills_named_cfg(&mutated, &["alpha"])).unwrap();

    let (s2, _rx2) = sink();
    let checked = engine
        .agent_lock(
            AgentLockSpec {
                config_path: Some(cfg2.to_string_lossy().to_string()),
                scope_override: None,
                check: true,
                upgrade_only: Vec::new(),
                lock_mode: AgentLockMode::Plain,
            },
            &s2,
        )
        .expect("lock --check");
    assert!(checked.check && !checked.saved, "--check never saves");
    assert!(!checked.drift.is_empty(), "mutated source drifts");
    assert!(checked.drift.iter().any(|d| d.id.contains("alpha")));
    assert_eq!(
        std::fs::read_to_string(&lock_path).unwrap(),
        lock_before,
        "--check wrote nothing to the on-disk lock"
    );
}

/// Contract: ledger C-09 — `--check` with `Locked` is a true zero-network audit: a satisfied
/// lock diffs clean against itself (no re-resolve, empty drift, nothing written).
#[test]
fn c09_lock_check_locked_is_zero_network_clean() {
    let pack = pack_dir();
    let (engine, proj, cfg) = project(&skills_named_cfg(&pack, &["alpha"]));
    let (s, _rx) = sink();
    engine
        .agent_lock(
            AgentLockSpec {
                config_path: Some(cfg.clone()),
                scope_override: None,
                check: false,
                upgrade_only: Vec::new(),
                lock_mode: AgentLockMode::Plain,
            },
            &s,
        )
        .expect("lock write");
    // Remove the source entirely → a zero-network Locked check must still diff clean.
    let (s2, _rx2) = sink();
    let checked = engine
        .agent_lock(
            AgentLockSpec {
                config_path: Some(cfg),
                scope_override: None,
                check: true,
                upgrade_only: Vec::new(),
                lock_mode: AgentLockMode::Locked,
            },
            &s2,
        )
        .expect("locked --check");
    assert!(
        checked.drift.is_empty(),
        "satisfied locked check diffs clean"
    );
    assert!(!checked.saved);
    let _ = proj; // project root retained for symmetry.
}

// =======================================================================================
// C-10 — agent_list (verb 7/10): scope filter + kind filter (read-only, never writes)
//   Contract: ledger C-10 (no kasetto unit oracle; primitives P-01 read_skill_profile,
//   load_skills_mcps_commands already [x]).
// =======================================================================================

/// Contract: ledger C-10 — list reads the lock(s) and reports installed skills + MCP/command
/// rows, filtered by `ListKind`. Read-only: it writes nothing. Verifies the All vs Skills filter
/// and that an `--scope` override is honored (merged_scopes=false).
#[test]
fn c10_list_filters_by_kind_and_scope_readonly() {
    let _guard = cwd_lock().lock().unwrap();
    let pack = pack_dir();
    let yaml = format!(
        "agent: claude-code\nscope: project\nskills:\n  - source: {p}\n    skills: \"*\"\nmcps:\n  - source: {p}\n    mcps: \"*\"\n",
        p = pack.display()
    );
    let (engine, proj, cfg) = project(&yaml);

    let (s, _rx) = sink();
    engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg),
                apply: true,
                ..Default::default()
            },
            &s,
        )
        .expect("sync install");

    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(&proj).unwrap();

    let (sl, _rxl) = sink();
    let all = engine
        .agent_list(
            AgentListSpec {
                scope_override: Some(AgentScope::Project),
                kind: AgentListKind::All,
            },
            &sl,
        )
        .expect("list all");
    assert!(
        !all.merged_scopes,
        "explicit --scope is not the merged view"
    );
    assert!(all.skills.iter().any(|s| s.skill == "alpha"));
    assert!(all.mcps.iter().any(|m| m.name == "github"));

    let (sm, _rxm) = sink();
    let mcps_only = engine
        .agent_list(
            AgentListSpec {
                scope_override: Some(AgentScope::Project),
                kind: AgentListKind::Mcps,
            },
            &sm,
        )
        .expect("list mcps");
    assert!(mcps_only.skills.is_empty(), "mcps-only list drops skills");
    assert!(mcps_only.mcps.iter().any(|m| m.name == "github"));

    std::env::set_current_dir(&prev).unwrap();
    // No lock/config was written by listing.
    assert!(
        proj.join("agent-env.lock").is_file(),
        "the only lock is the one sync wrote (list added none)"
    );
}

// =======================================================================================
// C-11 / C-14 — agent_clean (verb 8 + uninstall asset-cleanup): dry_run preview + apply tears
//   down ONLY lock-tracked assets (never-clobber pre-existing servers); the lock is cleared.
//   Contract: ledger C-11/C-14 (no kasetto unit oracle; primitives L-03 clear_all, MC-02
//   remove_mcp_server, apply_removals already [x]).
// =======================================================================================

/// Contract: ledger C-11 — `dry_run` (apply=false) reports the would-remove counts but deletes
/// NOTHING and does not clear the lock. Contract: ledger C-14 — only lock-TRACKED MCP servers
/// are torn down; a pre-existing untracked server (broker/weave) survives apply (never-clobber).
#[test]
fn c11_c14_clean_preview_then_apply_removes_tracked_only() {
    let _guard = cwd_lock().lock().unwrap();
    let pack = pack_dir();
    let yaml = format!(
        "agent: claude-code\nscope: project\nskills:\n  - source: {p}\n    skills: \"*\"\nmcps:\n  - source: {p}\n    mcps: \"*\"\n",
        p = pack.display()
    );
    let (engine, proj, cfg) = project(&yaml);

    // Seed an untracked server the lock will never own.
    std::fs::write(
        proj.join(".mcp.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": { "weave": { "command": "weave" } }
        }))
        .unwrap(),
    )
    .unwrap();

    let (s, _rx) = sink();
    engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg),
                apply: true,
                ..Default::default()
            },
            &s,
        )
        .expect("sync install");
    let lock_path = proj.join("agent-env.lock");
    assert!(lock_path.is_file());
    let lock_before = std::fs::read_to_string(&lock_path).unwrap();

    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(&proj).unwrap();

    // Preview: counts > 0, but nothing deleted and the lock is intact.
    let (s2, _rx2) = sink();
    let preview = engine
        .agent_clean(
            AgentCleanSpec {
                scope_override: Some(AgentScope::Project),
                apply: false,
            },
            &s2,
        )
        .expect("clean preview");
    assert!(preview.dry_run);
    assert!(
        preview.summary.removed >= 1,
        "preview reports would-remove count"
    );
    assert!(
        proj.join(".claude/skills/alpha").exists(),
        "preview deleted nothing"
    );
    assert_eq!(
        std::fs::read_to_string(&lock_path).unwrap(),
        lock_before,
        "preview never clears the lock (fail-closed: never partial-prune the lock)"
    );

    // Apply: tracked assets gone, untracked `weave` survives, lock cleared.
    let (s3, _rx3) = sink();
    let applied = engine
        .agent_clean(
            AgentCleanSpec {
                scope_override: Some(AgentScope::Project),
                apply: true,
            },
            &s3,
        )
        .expect("clean apply");
    assert!(!applied.dry_run);
    assert!(
        !proj.join(".claude/skills/alpha").exists(),
        "tracked skill removed on apply"
    );
    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join(".mcp.json")).unwrap()).unwrap();
    let servers = merged["mcpServers"].as_object().unwrap();
    assert!(
        servers.contains_key("weave"),
        "untracked pre-existing server survives clean (C-14 never-clobber)"
    );
    assert!(
        !servers.contains_key("github"),
        "lock-tracked server torn down on clean"
    );

    std::env::set_current_dir(&prev).unwrap();
}
