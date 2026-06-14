//! Integration tests for the engine agent-env subsystem (TASK-0013). All hermetic: local
//! `source:` paths (the committed `fixtures/agent/pack`), tempdir project roots, NO network.
//!
//! Isolation: a per-process temp `HOME` (with XDG_* unset → derived from HOME) keeps the
//! agent-env data/cache (the global lock + runtime memo) inside the test sandbox. Each test
//! uses a distinct project tempdir, so the per-project lock + runtime never collide. The two
//! cwd-dependent verbs (`list`, `clean`) serialize through `CWD_LOCK`.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use envctl_engine::event::{Event, EventSink};
use envctl_engine::{
    AgentCleanSpec, AgentListKind, AgentListSpec, AgentLockMode, AgentLockSpec, AgentRemoveSpec,
    AgentScope, AgentSectionSel, AgentSyncSpec, Engine,
};

// ---------------------------------------------------------------------------------------
// Sandbox helpers
// ---------------------------------------------------------------------------------------

/// One shared per-process temp HOME so the agent-env global data/cache dirs resolve inside the
/// sandbox. XDG_* are cleared so they derive from HOME.
fn sandbox_home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let base = unique_dir("envctl-agent-it-home");
        std::env::set_var("HOME", &base);
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_CACHE_HOME");
        std::env::remove_var("ENVCTL_AGENT_CONFIG");
        base
    })
}

/// Serializes the cwd-dependent verbs (list/clean read `std::env::current_dir`).
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

/// The committed local skill/MCP pack fixture.
fn pack_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent/pack")
}

/// Build a project tempdir with an `agent-env.yaml` and return (engine, project_dir, cfg_path).
fn project_with_config(yaml: &str) -> (Engine, PathBuf, String) {
    sandbox_home();
    let project = unique_dir("envctl-agent-it-proj");
    let cfg_path = project.join("agent-env.yaml");
    std::fs::write(&cfg_path, yaml).unwrap();
    // The agent verbs are manifest-independent; a detached engine is enough.
    let engine = Engine::detached();
    (engine, project, cfg_path.to_string_lossy().to_string())
}

/// A claude-code, project-scope config whose skills+mcps come from the local fixture pack.
fn full_config(pack: &Path) -> String {
    format!(
        "agent: claude-code\nscope: project\nskills:\n  - source: {pack}\n    skills: \"*\"\nmcps:\n  - source: {pack}\n    mcps: \"*\"\n",
        pack = pack.display()
    )
}

/// Drain a sink's events into a Vec for assertions.
fn drain(rx: std::sync::mpsc::Receiver<Event>) -> Vec<Event> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        out.push(ev);
    }
    out
}

fn sink() -> (EventSink, std::sync::mpsc::Receiver<Event>) {
    EventSink::channel()
}

// ---------------------------------------------------------------------------------------
// 1. sync preview (no writes, would_install) vs apply (installs + lock)
// ---------------------------------------------------------------------------------------

#[test]
fn sync_preview_writes_nothing_then_apply_installs() {
    let (engine, project, cfg) = project_with_config(&full_config(&pack_dir()));

    // Preview: zero writes.
    let (s, rx) = sink();
    let report = engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg.clone()),
                apply: false,
                ..Default::default()
            },
            &s,
        )
        .expect("preview sync");
    assert!(report.dry_run);
    assert!(report.summary.installed >= 2, "alpha+beta would install");
    assert!(report.summary.failed == 0);
    let events = drain(rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::AgentRunStarted { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::AgentRunFinished { .. })));
    // No skills dir, no lock on disk.
    assert!(
        !project.join(".claude/skills/alpha").exists(),
        "preview wrote nothing"
    );
    assert!(
        !project.join("agent-env.lock").exists(),
        "preview wrote no lock"
    );

    // Apply: installs + lock.
    let (s2, _rx2) = sink();
    let applied = engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg.clone()),
                apply: true,
                ..Default::default()
            },
            &s2,
        )
        .expect("apply sync");
    assert!(!applied.dry_run);
    assert_eq!(applied.summary.failed, 0);
    assert!(project.join(".claude/skills/alpha/SKILL.md").is_file());
    assert!(project.join(".claude/skills/beta/SKILL.md").is_file());
    assert!(
        project.join("agent-env.lock").is_file(),
        "apply wrote the lock"
    );
}

// ---------------------------------------------------------------------------------------
// 2. MCP never-clobber: pre-existing broker/repowire/weave survive a sync
// ---------------------------------------------------------------------------------------

#[test]
fn mcp_sync_is_additive_never_clobbers_existing_servers() {
    let (engine, project, cfg) = project_with_config(&full_config(&pack_dir()));

    // Seed a pre-existing .mcp.json with three global servers NOT tracked by the agent lock.
    let pre = serde_json::json!({
        "mcpServers": {
            "broker": { "command": "broker" },
            "repowire": { "command": "repowire" },
            "weave": { "command": "weave" }
        }
    });
    std::fs::write(
        project.join(".mcp.json"),
        serde_json::to_string_pretty(&pre).unwrap(),
    )
    .unwrap();

    let (s, _rx) = sink();
    let report = engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg),
                apply: true,
                ..Default::default()
            },
            &s,
        )
        .expect("apply sync");
    assert_eq!(report.summary.failed, 0);

    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(project.join(".mcp.json")).unwrap()).unwrap();
    let servers = merged["mcpServers"].as_object().unwrap();
    // 3 pre-existing + 2 from the fixture pack (github, context7) = 5 present.
    for name in ["broker", "repowire", "weave", "github", "context7"] {
        assert!(
            servers.contains_key(name),
            "{name} must be present after sync"
        );
    }
}

// ---------------------------------------------------------------------------------------
// 3. lock --check drift (mutate content -> drift; clean -> empty)
// ---------------------------------------------------------------------------------------

#[test]
fn lock_check_reports_drift_then_clean() {
    let (engine, project, cfg) = project_with_config(&full_config(&pack_dir()));

    // Write the lock first.
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
    assert!(project.join("agent-env.lock").is_file());

    // Clean check: no drift.
    let (s2, _rx2) = sink();
    let clean = engine
        .agent_lock(
            AgentLockSpec {
                config_path: Some(cfg.clone()),
                scope_override: None,
                check: true,
                upgrade_only: Vec::new(),
                lock_mode: AgentLockMode::Plain,
            },
            &s2,
        )
        .expect("lock check clean");
    assert!(clean.check && !clean.saved);
    assert!(clean.drift.is_empty(), "no drift right after writing");

    // Mutate a skill's content -> the re-resolved hash differs -> drift.
    let copied = pack_dir(); // mutate a COPY so the committed fixture stays pristine.
    let mutated_pack = project.join("pack");
    copy_tree(&copied, &mutated_pack);
    std::fs::write(
        mutated_pack.join("alpha/SKILL.md"),
        "---\nname: alpha\n---\nMUTATED\n",
    )
    .unwrap();
    let cfg2_path = project.join("agent-env-2.yaml");
    std::fs::write(&cfg2_path, full_config(&mutated_pack)).unwrap();

    let (s3, _rx3) = sink();
    let drifted = engine
        .agent_lock(
            AgentLockSpec {
                config_path: Some(cfg2_path.to_string_lossy().to_string()),
                scope_override: None,
                check: true,
                upgrade_only: Vec::new(),
                lock_mode: AgentLockMode::Plain,
            },
            &s3,
        )
        .expect("lock check drifted");
    assert!(!drifted.drift.is_empty(), "mutated content drifts");
    assert!(drifted.drift.iter().any(|d| d.id.contains("alpha")));
}

// ---------------------------------------------------------------------------------------
// 4. --locked zero-network (unlocked source -> locked_error + failed, no fetch)
// ---------------------------------------------------------------------------------------

#[test]
fn locked_mode_fails_closed_without_lock_then_passes_when_locked() {
    let (engine, project, cfg) = project_with_config(&full_config(&pack_dir()));

    // No lock yet: --locked must fail-closed (locked_error) without installing anything.
    let (s, _rx) = sink();
    let report = engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg.clone()),
                apply: true,
                lock_mode: AgentLockMode::Locked,
                ..Default::default()
            },
            &s,
        )
        .expect("locked sync (no lock)");
    assert!(
        report.summary.failed > 0,
        "unlocked source under --locked fails closed"
    );
    assert!(
        report.summary.installed == 0,
        "no install under failing --locked"
    );
    assert!(
        report.actions.iter().any(|a| a.status == "locked_error"),
        "a locked_error is recorded"
    );
    assert!(
        !project.join(".claude/skills/alpha").exists(),
        "no fetch/install happened"
    );

    // Now write the lock + install plainly, then --locked is satisfied (unchanged, no fetch).
    let (s2, _rx2) = sink();
    engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg.clone()),
                apply: true,
                lock_mode: AgentLockMode::Plain,
                ..Default::default()
            },
            &s2,
        )
        .unwrap();

    let (s3, _rx3) = sink();
    let locked_ok = engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg),
                apply: true,
                lock_mode: AgentLockMode::Locked,
                ..Default::default()
            },
            &s3,
        )
        .expect("locked sync (satisfied)");
    assert_eq!(
        locked_ok.summary.failed, 0,
        "satisfied lock passes --locked"
    );
    assert!(locked_ok.summary.unchanged >= 2);
}

// ---------------------------------------------------------------------------------------
// 5. remove + sync-after prune
// ---------------------------------------------------------------------------------------

#[test]
fn remove_then_sync_after_prunes_skill() {
    // Skills-only config so we can drop a named skill list.
    let pack = pack_dir();
    let yaml = format!(
        "agent: claude-code\nscope: project\nskills:\n  - source: {p}\n    skills:\n      - alpha\n      - beta\n",
        p = pack.display()
    );
    let (engine, project, cfg) = project_with_config(&yaml);

    // Install both.
    let (s, _rx) = sink();
    engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg.clone()),
                apply: true,
                ..Default::default()
            },
            &s,
        )
        .unwrap();
    assert!(project.join(".claude/skills/alpha/SKILL.md").is_file());
    assert!(project.join(".claude/skills/beta/SKILL.md").is_file());

    // Remove `beta` from the skills list, with sync-after (apply).
    let (s2, _rx2) = sink();
    let outcome = engine
        .agent_remove(
            AgentRemoveSpec {
                source: pack.display().to_string(),
                section: AgentSectionSel {
                    skills: vec!["beta".into()],
                    ..Default::default()
                },
                git_ref: None,
                branch: None,
                sub_dir: None,
                config_path: Some(cfg.clone()),
                scope_override: None,
                apply: true,
                no_sync: false,
                lock_mode: AgentLockMode::Plain,
            },
            &s2,
        )
        .expect("remove beta");
    assert_eq!(outcome.action, "removed");
    assert!(outcome.sync.is_some(), "sync-after ran");
    // alpha kept, beta pruned.
    assert!(project.join(".claude/skills/alpha/SKILL.md").is_file());
    assert!(
        !project.join(".claude/skills/beta").exists(),
        "beta pruned by sync-after"
    );
}

// ---------------------------------------------------------------------------------------
// 6. clean preview vs apply (untracked MCP survives)
// ---------------------------------------------------------------------------------------

#[test]
fn clean_preview_keeps_then_apply_removes_tracked_only() {
    let _guard = cwd_lock().lock().unwrap();
    let (engine, project, cfg) = project_with_config(&full_config(&pack_dir()));

    // Seed an untracked global server alongside what the sync will add.
    std::fs::write(
        project.join(".mcp.json"),
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
        .unwrap();
    assert!(project.join(".claude/skills/alpha").exists());

    // clean + list are cwd-based.
    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&project).unwrap();

    // list (read-only) sees the installed skills + the synced MCP servers.
    let (sl, _rxl) = sink();
    let listed = engine
        .agent_list(
            AgentListSpec {
                scope_override: Some(AgentScope::Project),
                kind: AgentListKind::All,
            },
            &sl,
        )
        .expect("list");
    assert!(listed.skills.iter().any(|s| s.skill == "alpha"));
    assert!(listed.mcps.iter().any(|m| m.name == "github"));

    // list filtered to skills only drops the MCP rows.
    let (sl2, _rxl2) = sink();
    let skills_only = engine
        .agent_list(
            AgentListSpec {
                scope_override: Some(AgentScope::Project),
                kind: AgentListKind::Skills,
            },
            &sl2,
        )
        .expect("list skills");
    assert!(skills_only.mcps.is_empty(), "skills-only list has no mcps");

    // Preview: nothing removed.
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
    assert!(preview.summary.removed >= 1);
    assert!(
        project.join(".claude/skills/alpha").exists(),
        "preview removed nothing"
    );

    // Apply: tracked assets removed, untracked `weave` survives.
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
        !project.join(".claude/skills/alpha").exists(),
        "tracked skill removed"
    );

    let servers: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(project.join(".mcp.json")).unwrap()).unwrap();
    let map = servers["mcpServers"].as_object().unwrap();
    assert!(
        map.contains_key("weave"),
        "untracked global MCP survives clean"
    );
    assert!(!map.contains_key("github"), "tracked MCP removed by clean");

    std::env::set_current_dir(prev_cwd).unwrap();
}

// ---------------------------------------------------------------------------------------
// 7. M-22 fallback (config_path: None -> scope from default-config file)
// ---------------------------------------------------------------------------------------

#[test]
fn m22_fallback_resolves_default_config_from_cwd() {
    let _guard = cwd_lock().lock().unwrap();
    let (engine, project, _cfg) = project_with_config(&full_config(&pack_dir()));

    // No explicit config_path -> default_config_path() resolves the local `agent-env.yaml`.
    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&project).unwrap();

    let (s, _rx) = sink();
    let report = engine
        .agent_sync(
            AgentSyncSpec {
                config_path: None,
                apply: false,
                ..Default::default()
            },
            &s,
        )
        .expect("m22 fallback sync");
    // The default config was found and its project scope resolved.
    assert_eq!(report.scope, AgentScope::Project);
    assert!(
        report.summary.installed >= 2,
        "skills discovered via default config"
    );

    std::env::set_current_dir(prev_cwd).unwrap();
}

// ---------------------------------------------------------------------------------------
// 8. never-prune-on-failure (good + failing source -> good assets kept)
// ---------------------------------------------------------------------------------------

#[test]
fn never_prune_when_a_source_fails() {
    // Install a good skill first, then add a failing (nonexistent) source and re-sync:
    // the failing source bumps `failed`, so the good locked skill must NOT be pruned.
    let pack = pack_dir();
    let good_only = format!(
        "agent: claude-code\nscope: project\nskills:\n  - source: {p}\n    skills:\n      - alpha\n",
        p = pack.display()
    );
    let (engine, project, cfg) = project_with_config(&good_only);

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
        .unwrap();
    assert!(project.join(".claude/skills/alpha/SKILL.md").is_file());

    // Now a config that keeps alpha but adds a source that ERRORS at materialize time
    // (a nonexistent `sub-dir` of the real pack -> source_error -> summary.failed++,
    // distinct from a `broken` skill which would NOT trip the never-prune guard).
    let with_broken = format!(
        "agent: claude-code\nscope: project\nskills:\n  - source: {p}\n    skills:\n      - alpha\n  - source: {p}\n    sub-dir: no-such-subdir\n    skills:\n      - ghost\n",
        p = pack.display()
    );
    let cfg2 = project.join("agent-env-2.yaml");
    std::fs::write(&cfg2, with_broken).unwrap();

    let (s2, _rx2) = sink();
    let report = engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg2.to_string_lossy().to_string()),
                apply: true,
                ..Default::default()
            },
            &s2,
        )
        .expect("sync with broken source");
    assert!(report.summary.failed > 0, "broken source records a failure");
    assert_eq!(report.summary.removed, 0, "never prune when failed > 0");
    assert!(
        project.join(".claude/skills/alpha/SKILL.md").is_file(),
        "good locked skill survives a failing sibling source"
    );
}

// ---------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------

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
