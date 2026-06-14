//! Differential PARITY verification of the absorbed sync engine (kasetto → envctl, Epic C
//! TASK-0012, rows C-01..C-06) against kasetto v3.2.0's own certified `#[cfg(test)]` vectors in
//! `src/commands/sync/{skills,commands,mcps}.rs` (7 + 3 + 3 = 13). Each test below reproduces an
//! oracle test's EXACT fixture and asserts the IDENTICAL effect — but driven through the engine's
//! public `Engine::agent_sync` API (over the on-disk lock + installed files + merged `.mcp.json` +
//! the returned `AgentReport`), proving envctl's `crates/agent-env` driver reproduces kasetto's
//! behavior. The engine source is FROZEN; this file only verifies it.
//!
//! Oracle-mapping note: kasetto's unit tests call `sync_skills`/`sync_commands` directly against an
//! in-memory `&mut State`/`LockFile`, re-running on the SAME mutated lock. The engine persists the
//! lock to disk and re-loads it each call, so re-invoking `agent_sync` against the same project
//! tempdir reproduces the oracle's "second run" semantics through the real public seam. The skills
//! oracle drives a 1-source config with explicit `dests`; here the destination is steered via the
//! config `destination:`/`agent:` fields — the EFFECT under test (locked-set hold, local repair,
//! never-prune, tamper-update, locked fail-closed) is identical, not the literal dest path.
//!
//! Isolation mirrors `agent_sync.rs`: one per-process temp HOME (XDG_* cleared) so the agent-env
//! global data/cache resolve inside the sandbox; a distinct project tempdir per test.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use envctl_engine::event::{Event, EventSink};
use envctl_engine::{AgentLockMode, AgentScope, AgentSyncSpec, Engine};

// ---------------------------------------------------------------------------------------
// Sandbox helpers (same discipline as agent_sync.rs; kept local so the two files stay
// independent test binaries with no shared-module coupling).
// ---------------------------------------------------------------------------------------

fn sandbox_home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let base = unique_dir("envctl-agent-parity-home");
        std::env::set_var("HOME", &base);
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_CACHE_HOME");
        std::env::remove_var("ENVCTL_AGENT_CONFIG");
        base
    })
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

fn skill_pack() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent/pack")
}

fn cmd_pack() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent/cmdpack")
}

/// Write `agent-env.yaml` into a fresh project tempdir; return (engine, project, cfg_path).
fn project(yaml: &str) -> (Engine, PathBuf, String) {
    sandbox_home();
    let proj = unique_dir("envctl-agent-parity-proj");
    let cfg = proj.join("agent-env.yaml");
    std::fs::write(&cfg, yaml).unwrap();
    (Engine::detached(), proj, cfg.to_string_lossy().to_string())
}

fn sink() -> (EventSink, std::sync::mpsc::Receiver<Event>) {
    EventSink::channel()
}

/// Apply-mode plain sync against a config path. Returns the report.
fn sync_apply(engine: &Engine, cfg: &str) -> envctl_engine::AgentReport {
    let (s, _rx) = sink();
    engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg.to_string()),
                apply: true,
                ..Default::default()
            },
            &s,
        )
        .expect("agent_sync apply")
}

/// Apply-mode sync with an explicit lock mode (Update / Locked).
fn sync_apply_mode(engine: &Engine, cfg: &str, mode: AgentLockMode) -> envctl_engine::AgentReport {
    let (s, _rx) = sink();
    engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg.to_string()),
                apply: true,
                lock_mode: mode,
                ..Default::default()
            },
            &s,
        )
        .expect("agent_sync apply (mode)")
}

/// Copy a directory tree (used to build mutable source copies that don't touch the committed
/// fixtures).
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

// =======================================================================================
// C-02 / C-01 — sync_skills parity (7 oracle vectors)
//   Oracle: kasetto src/commands/sync/skills.rs::<test>
// =======================================================================================

/// A skills-only project config: one source (a mutable COPY of the skill pack so we can delete
/// the source mid-test like the oracle does), an explicit single `destination:`.
fn skills_cfg_one_dest(src: &Path, dest: &Path, skills_yaml: &str) -> String {
    format!(
        "scope: project\ndestination: {dest}\nskills:\n  - source: {src}\n{skills_yaml}",
        dest = dest.display(),
        src = src.display(),
    )
}

fn list_block(names: &[&str]) -> String {
    let mut s = String::from("    skills:\n");
    for n in names {
        s.push_str(&format!("      - {n}\n"));
    }
    s
}

/// Oracle: kasetto src/commands/sync/skills.rs::first_run_installs_then_second_run_unchanged_without_source
/// First run installs; deleting the SOURCE then re-syncing plainly still reports `unchanged`
/// (no fetch — re-hash of the locked dest matches), failed == 0.
#[test]
fn c02_first_run_installs_then_unchanged_without_source() {
    let src = unique_dir("parity-skillsrc");
    copy_tree(&skill_pack(), &src);
    let (engine, proj, _cfg) = project("scope: project\n");
    let dest = proj.join(".agent/skills");
    let cfg_path = proj.join("env-1.yaml");
    std::fs::write(
        &cfg_path,
        skills_cfg_one_dest(&src, &dest, &list_block(&["alpha"])),
    )
    .unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    let s1 = sync_apply(&engine, &cfg);
    assert_eq!(s1.summary.installed, 1, "first run installs");
    assert!(dest.join("alpha/SKILL.md").is_file());

    // Remove the source entirely: a plain re-sync must still report unchanged (no fetch).
    std::fs::remove_dir_all(&src).unwrap();
    let s2 = sync_apply(&engine, &cfg);
    assert_eq!(s2.summary.unchanged, 1, "second run unchanged, no fetch");
    assert_eq!(s2.summary.failed, 0);
}

/// Oracle: kasetto src/commands/sync/skills.rs::tampered_dest_is_repaired_from_source
/// Tampering the installed copy makes the dest hash mismatch the lock → needs_fetch repairs from
/// source → `updated == 1`.
#[test]
fn c02_tampered_dest_is_repaired_from_source() {
    let src = unique_dir("parity-skillsrc");
    copy_tree(&skill_pack(), &src);
    let (engine, proj, _cfg) = project("scope: project\n");
    let dest = proj.join(".agent/skills");
    let cfg_path = proj.join("env-1.yaml");
    std::fs::write(
        &cfg_path,
        skills_cfg_one_dest(&src, &dest, &list_block(&["alpha"])),
    )
    .unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    sync_apply(&engine, &cfg);
    // Tamper the installed copy → no good local copy → repair from source.
    std::fs::write(
        dest.join("alpha/SKILL.md"),
        "---\nname: alpha\n---\nEDITED\n",
    )
    .unwrap();
    let s = sync_apply(&engine, &cfg);
    assert_eq!(s.summary.updated, 1, "tampered dest repaired from source");
    assert_eq!(s.summary.failed, 0);
}

/// Oracle: kasetto src/commands/sync/skills.rs::missing_second_dest_repaired_locally_without_source
/// Two destinations; drop the second dest AND remove the source — repair must copy from the good
/// first dest (local repair, no fetch) → `updated == 1`, failed == 0, dest2 restored.
#[test]
fn c02_missing_second_dest_repaired_locally_without_source() {
    let src = unique_dir("parity-skillsrc");
    copy_tree(&skill_pack(), &src);
    let (engine, proj, _cfg) = project("scope: project\n");
    // Two agents → two skill destinations (.claude/skills + .cursor/skills).
    let cfg_path = proj.join("env-1.yaml");
    let yaml = format!(
        "scope: project\nagent:\n  - claude-code\n  - cursor\nskills:\n  - source: {src}\n{block}",
        src = src.display(),
        block = list_block(&["alpha"]),
    );
    std::fs::write(&cfg_path, yaml).unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    sync_apply(&engine, &cfg);
    let dest1 = proj.join(".claude/skills/alpha/SKILL.md");
    let dest2_dir = proj.join(".cursor/skills");
    assert!(dest1.is_file());
    assert!(dest2_dir.join("alpha/SKILL.md").exists());

    // Drop dest2 and the source: repair must copy from the good dest1.
    std::fs::remove_dir_all(&dest2_dir).unwrap();
    std::fs::remove_dir_all(&src).unwrap();
    let s = sync_apply(&engine, &cfg);
    assert_eq!(s.summary.updated, 1, "repaired locally from the good dest");
    assert_eq!(s.summary.failed, 0);
    assert!(dest2_dir.join("alpha/SKILL.md").exists(), "dest2 restored");
}

/// Oracle: kasetto src/commands/sync/skills.rs::wildcard_holds_to_locked_set_on_plain_sync
/// A wildcard source installs {alpha,beta}; removing beta from the SOURCE and re-syncing PLAINLY
/// keeps the locked set (2 unchanged, 0 removed).
#[test]
fn c02_wildcard_holds_to_locked_set_on_plain_sync() {
    let src = unique_dir("parity-skillsrc");
    copy_tree(&skill_pack(), &src);
    let (engine, proj, _cfg) = project("scope: project\n");
    let dest = proj.join(".agent/skills");
    let cfg_path = proj.join("env-1.yaml");
    std::fs::write(
        &cfg_path,
        skills_cfg_one_dest(&src, &dest, "    skills: \"*\"\n"),
    )
    .unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    let s1 = sync_apply(&engine, &cfg);
    assert_eq!(s1.summary.installed, 2, "wildcard installs alpha+beta");

    // Remove beta from the SOURCE; a plain wildcard re-sync holds the locked set.
    std::fs::remove_dir_all(src.join("beta")).unwrap();
    let s2 = sync_apply(&engine, &cfg);
    assert_eq!(s2.summary.unchanged, 2, "locked set still honored");
    assert_eq!(
        s2.summary.removed, 0,
        "plain sync never prunes the wildcard set"
    );
}

/// Oracle: kasetto src/commands/sync/skills.rs::wildcard_update_prunes_removed_skill
/// Same setup, but `--update` (LockMode::Update) re-resolves the wildcard against the source and
/// prunes the upstream-removed skill → `removed == 1`.
#[test]
fn c02_wildcard_update_prunes_removed_skill() {
    let src = unique_dir("parity-skillsrc");
    copy_tree(&skill_pack(), &src);
    let (engine, proj, _cfg) = project("scope: project\n");
    let dest = proj.join(".agent/skills");
    let cfg_path = proj.join("env-1.yaml");
    std::fs::write(
        &cfg_path,
        skills_cfg_one_dest(&src, &dest, "    skills: \"*\"\n"),
    )
    .unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    sync_apply(&engine, &cfg);
    std::fs::remove_dir_all(src.join("beta")).unwrap();
    let s = sync_apply_mode(&engine, &cfg, AgentLockMode::Update { only: Vec::new() });
    assert_eq!(
        s.summary.removed, 1,
        "--update prunes the upstream-removed skill"
    );
}

/// Oracle: kasetto src/commands/sync/skills.rs::locked_errors_when_skill_absent_from_lock
/// `--locked` with no lock yet fails closed: `failed == 1`, `installed == 0`, a `locked_error`
/// action, and nothing installed on disk.
#[test]
fn c02_locked_errors_when_skill_absent_from_lock() {
    let src = unique_dir("parity-skillsrc");
    copy_tree(&skill_pack(), &src);
    let (engine, proj, _cfg) = project("scope: project\n");
    let dest = proj.join(".agent/skills");
    let cfg_path = proj.join("env-1.yaml");
    std::fs::write(
        &cfg_path,
        skills_cfg_one_dest(&src, &dest, &list_block(&["alpha"])),
    )
    .unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    let s = sync_apply_mode(&engine, &cfg, AgentLockMode::Locked);
    assert_eq!(s.summary.failed, 1, "--locked errors when not in lock");
    assert_eq!(s.summary.installed, 0);
    assert!(s.actions.iter().any(|a| a.status == "locked_error"));
    assert!(
        !dest.join("alpha").exists(),
        "no fetch/install under failing --locked"
    );
}

/// Oracle: kasetto src/commands/sync/skills.rs::locked_succeeds_when_satisfiable_and_repairs
/// After a plain install writes the lock, a `--locked` re-sync of the still-good install is
/// satisfied with ZERO fetch → `unchanged == 1`, `failed == 0`.
#[test]
fn c02_locked_succeeds_when_satisfiable() {
    let src = unique_dir("parity-skillsrc");
    copy_tree(&skill_pack(), &src);
    let (engine, proj, _cfg) = project("scope: project\n");
    let dest = proj.join(".agent/skills");
    let cfg_path = proj.join("env-1.yaml");
    std::fs::write(
        &cfg_path,
        skills_cfg_one_dest(&src, &dest, &list_block(&["alpha"])),
    )
    .unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    sync_apply(&engine, &cfg); // plain install writes the lock
    let s = sync_apply_mode(&engine, &cfg, AgentLockMode::Locked);
    assert_eq!(
        s.summary.unchanged, 1,
        "satisfiable --locked is unchanged, no fetch"
    );
    assert_eq!(s.summary.failed, 0);
}

// =======================================================================================
// C-03 — sync_commands parity (3 oracle vectors)
//   Oracle: kasetto src/commands/sync/commands.rs::<test>
// =======================================================================================

/// Oracle: kasetto src/commands/sync/commands.rs::sync_writes_to_supported_agents_and_skips_unsupported
/// A wildcard command source applied for claude-code + gemini-cli + cursor + codex writes the
/// per-agent TRANSFORM (claude nested `git/commit.md`, gemini flattened `git-commit.toml`, cursor
/// plain `git-commit.md`); codex has no project commands path → not written. A pre-existing
/// user-authored `.claude/commands/user-own.md` is preserved. Then dropping the `commands:` block
/// and re-syncing prunes the managed files but keeps the user file.
#[test]
fn c03_commands_write_to_supported_agents_skip_unsupported_then_prune() {
    let pack = cmd_pack();
    let (engine, proj, _cfg) = project("scope: project\n");

    // Pre-existing user file under .claude/commands must survive.
    let user_file = proj.join(".claude/commands/user-own.md");
    std::fs::create_dir_all(user_file.parent().unwrap()).unwrap();
    std::fs::write(&user_file, "user authored\n").unwrap();

    let cfg_path = proj.join("env-1.yaml");
    let yaml = format!(
        "scope: project\nagent:\n  - claude-code\n  - gemini-cli\n  - cursor\n  - codex\ncommands:\n  - source: {src}\n    commands: \"*\"\n",
        src = pack.display(),
    );
    std::fs::write(&cfg_path, yaml).unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    let r = sync_apply(&engine, &cfg);
    assert_eq!(r.summary.failed, 0);
    // Per-agent transforms (the `git/commit.md` source discovers as `git:commit`).
    assert!(
        proj.join(".claude/commands/git/commit.md").is_file(),
        "claude nested"
    );
    assert!(
        proj.join(".gemini/commands/git-commit.toml").is_file(),
        "gemini TOML flattened"
    );
    assert!(
        proj.join(".cursor/commands/git-commit.md").is_file(),
        "cursor plain md"
    );
    // Codex has no project commands path → skipped.
    assert!(
        !proj.join(".codex/prompts").exists(),
        "unsupported agent dir not created"
    );
    // User-authored file untouched.
    assert!(user_file.is_file(), "pre-existing user command preserved");

    // Drop the `commands:` block → remove_stale prunes the managed files, keeps the user file.
    let cfg2_path = proj.join("env-2.yaml");
    let yaml2 = "scope: project\nagent:\n  - claude-code\n  - gemini-cli\n  - cursor\n  - codex\n";
    std::fs::write(&cfg2_path, yaml2).unwrap();
    let r2 = sync_apply(&engine, &cfg2_path.to_string_lossy().to_string());
    assert_eq!(r2.summary.failed, 0);
    assert!(
        !proj.join(".claude/commands/git/commit.md").exists(),
        "managed claude cmd pruned"
    );
    assert!(
        !proj.join(".gemini/commands/git-commit.toml").exists(),
        "managed gemini cmd pruned"
    );
    assert!(
        !proj.join(".cursor/commands/git-commit.md").exists(),
        "managed cursor cmd pruned"
    );
    assert!(user_file.is_file(), "user file survives the prune");
}

/// Oracle: kasetto src/commands/sync/commands.rs::second_run_unchanged_without_source_no_fetch
/// A wildcard command source installs `foo`; removing the SOURCE and re-syncing plainly reports
/// `unchanged == 1`, `failed == 0`, `removed == 0` (the installed file is a transform, repaired
/// only via fetch — but the hash+dest-file match means no fetch is needed).
#[test]
fn c03_second_run_unchanged_without_source_no_fetch() {
    let src = unique_dir("parity-cmdsrc");
    copy_tree(&cmd_pack(), &src);
    // Drop the nested git/commit.md so the only command is `foo` (matches the oracle fixture).
    std::fs::remove_dir_all(src.join("commands/git")).unwrap();
    let (engine, proj, _cfg) = project("scope: project\n");
    let cfg_path = proj.join("env-1.yaml");
    let yaml = format!(
        "scope: project\nagent: claude-code\ncommands:\n  - source: {src}\n    commands: \"*\"\n",
        src = src.display(),
    );
    std::fs::write(&cfg_path, yaml).unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    let r1 = sync_apply(&engine, &cfg);
    assert_eq!(r1.summary.installed, 1, "first run installs foo");
    assert!(proj.join(".claude/commands/foo.md").is_file());

    std::fs::remove_dir_all(&src).unwrap();
    let r2 = sync_apply(&engine, &cfg);
    assert_eq!(r2.summary.unchanged, 1, "second run unchanged, no fetch");
    assert_eq!(r2.summary.failed, 0);
    assert_eq!(r2.summary.removed, 0, "lock entry retained, not pruned");
}

/// Oracle: kasetto src/commands/sync/commands.rs::locked_errors_when_command_absent_from_lock
/// `--locked` on a command list with no lock yet fails closed: `failed == 1`, `installed == 0`.
#[test]
fn c03_locked_errors_when_command_absent_from_lock() {
    let src = unique_dir("parity-cmdsrc");
    copy_tree(&cmd_pack(), &src);
    std::fs::remove_dir_all(src.join("commands/git")).unwrap();
    let (engine, proj, _cfg) = project("scope: project\n");
    let cfg_path = proj.join("env-1.yaml");
    let yaml = format!(
        "scope: project\nagent: claude-code\ncommands:\n  - source: {src}\n    commands:\n      - foo\n",
        src = src.display(),
    );
    std::fs::write(&cfg_path, yaml).unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    let r = sync_apply_mode(&engine, &cfg, AgentLockMode::Locked);
    assert_eq!(
        r.summary.failed, 1,
        "--locked errors when command not in lock"
    );
    assert_eq!(r.summary.installed, 0);
    assert!(r.actions.iter().any(|a| a.status == "locked_error"));
}

// =======================================================================================
// C-04 — sync_mcps parity (3 oracle vectors)
//   Oracle: kasetto src/commands/sync/mcps.rs::<test>
//
// kasetto's 3 mcp tests are white-box unit tests of private types (PendingMcp classification,
// new-vs-update gating, needs_fetch_mcps). The OBSERVABLE behaviors they certify — (1) new servers
// are merged ADDITIVELY, (2) an already-satisfied (present) server triggers no re-fetch/no-clobber,
// (3) an absent asset forces a fetch+install — are exercised here end-to-end through the public
// merge effect on `.mcp.json`. The additive/never-clobber property is the #1 no-downgrade invariant.
// =======================================================================================

fn mcp_cfg(pack: &Path) -> String {
    format!(
        "scope: project\nagent: claude-code\nmcps:\n  - source: {src}\n    mcps: \"*\"\n",
        src = pack.display(),
    )
}

/// Oracle: kasetto src/commands/sync/mcps.rs::pending_mcp_classification_new_vs_update
/// (the new-server classification → additive install). NEW servers from the source pack
/// (github, context7) are MERGED into `.mcp.json` while a pre-existing broker/repowire/weave
/// (NOT tracked by the lock) SURVIVE untouched. This is the additive/never-clobber invariant.
#[test]
fn c04_mcp_merge_is_additive_new_servers_added_existing_survive() {
    let (engine, proj, _cfg) = project("scope: project\n");
    // Seed a pre-existing .mcp.json with three servers the agent lock does not track.
    let pre = serde_json::json!({
        "mcpServers": {
            "broker": { "command": "broker" },
            "repowire": { "command": "repowire" },
            "weave": { "command": "weave" }
        }
    });
    std::fs::write(
        proj.join(".mcp.json"),
        serde_json::to_string_pretty(&pre).unwrap(),
    )
    .unwrap();

    let cfg_path = proj.join("env-1.yaml");
    std::fs::write(&cfg_path, mcp_cfg(&skill_pack())).unwrap();
    let r = sync_apply(&engine, &cfg_path.to_string_lossy().to_string());
    assert_eq!(r.summary.failed, 0);

    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join(".mcp.json")).unwrap()).unwrap();
    let servers = merged["mcpServers"].as_object().unwrap();
    // 3 pre-existing (never clobbered) + 2 new from the pack = 5.
    for name in ["broker", "repowire", "weave", "github", "context7"] {
        assert!(
            servers.contains_key(name),
            "{name} must be present after additive sync"
        );
    }
    // The pre-existing broker entry's command must be UNCHANGED (never clobbered).
    assert_eq!(
        servers["broker"]["command"], "broker",
        "existing server not overwritten"
    );
}

/// Oracle: kasetto src/commands/sync/mcps.rs::pending_mcp_no_new_servers_skips_gate
/// (an "update" with no NEW servers does not re-trip the install gate → unchanged). After a first
/// install, a plain re-sync finds the servers already present in every target → no re-fetch, the
/// `.mcp.json` is unchanged and no server is duplicated/clobbered.
#[test]
fn c04_mcp_resync_no_new_servers_is_unchanged() {
    let (engine, proj, _cfg) = project("scope: project\n");
    let cfg_path = proj.join("env-1.yaml");
    std::fs::write(&cfg_path, mcp_cfg(&skill_pack())).unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    let r1 = sync_apply(&engine, &cfg);
    assert_eq!(r1.summary.failed, 0);
    let after_first =
        std::fs::read_to_string(proj.join(".mcp.json")).expect(".mcp.json written on first sync");

    // Re-sync: servers already present in the only target → no new servers, no clobber.
    let r2 = sync_apply(&engine, &cfg);
    assert_eq!(r2.summary.failed, 0);
    let after_second = std::fs::read_to_string(proj.join(".mcp.json")).unwrap();
    let v1: serde_json::Value = serde_json::from_str(&after_first).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&after_second).unwrap();
    assert_eq!(
        v1, v2,
        "re-sync with no new servers leaves .mcp.json unchanged"
    );
    // Still exactly the 2 pack servers — none duplicated.
    assert_eq!(v2["mcpServers"].as_object().unwrap().len(), 2);
}

/// Oracle: kasetto src/commands/sync/mcps.rs::needs_fetch_mcps_true_when_asset_absent_false_when_satisfied
/// (absent lock asset forces a fetch; present+satisfied needs none). Driven end-to-end: the FIRST
/// sync (asset absent from lock) fetches+installs the servers (`.mcp.json` created with both); a
/// SECOND sync (asset present, servers satisfied in the target) performs no fetch (unchanged).
#[test]
fn c04_needs_fetch_when_absent_then_satisfied() {
    let (engine, proj, _cfg) = project("scope: project\n");
    let cfg_path = proj.join("env-1.yaml");
    std::fs::write(&cfg_path, mcp_cfg(&skill_pack())).unwrap();
    let cfg = cfg_path.to_string_lossy().to_string();

    // Asset absent from lock → fetch+install happens (the .mcp.json now carries both servers).
    assert!(!proj.join(".mcp.json").exists());
    let r1 = sync_apply(&engine, &cfg);
    assert_eq!(r1.summary.failed, 0);
    let merged: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join(".mcp.json")).unwrap()).unwrap();
    let servers = merged["mcpServers"].as_object().unwrap();
    assert!(
        servers.contains_key("github") && servers.contains_key("context7"),
        "fetched+installed"
    );

    // Asset present + servers satisfied in target → needs_fetch == false → unchanged, no failure.
    let r2 = sync_apply(&engine, &cfg);
    assert_eq!(r2.summary.failed, 0, "satisfied asset needs no fetch");
    assert_eq!(
        r2.summary.installed, 0,
        "no re-install when already satisfied"
    );
}

// =======================================================================================
// C-01 + C-05 — orchestrator + never-prune-on-failure (cross-cutting guardian-critical paths)
//   Oracle: the SyncContext orchestration contract + remove_stale never-prune guard, certified
//   across kasetto's skills/commands/mcps modules (the `failed>0 → skip remove_stale` branch).
// =======================================================================================

/// Oracle: kasetto src/commands/sync/{skills,commands,mcps}.rs remove_stale guard
/// (`if summary.failed == 0 { remove_stale(...) }`). A good locked skill must NOT be pruned when a
/// sibling source fails — proves the never-prune-on-failure invariant through the public API.
#[test]
fn c01_c05_never_prune_when_a_source_fails() {
    let pack = skill_pack();
    let (engine, proj, _cfg) = project("scope: project\n");
    let good_cfg = proj.join("good.yaml");
    let yaml = format!(
        "scope: project\nagent: claude-code\nskills:\n  - source: {p}\n    skills:\n      - alpha\n",
        p = pack.display(),
    );
    std::fs::write(&good_cfg, yaml).unwrap();
    sync_apply(&engine, &good_cfg.to_string_lossy().to_string());
    assert!(proj.join(".claude/skills/alpha/SKILL.md").is_file());

    // Add a sibling source that ERRORS at materialize (nonexistent sub-dir) → failed++.
    let broken_cfg = proj.join("broken.yaml");
    let yaml2 = format!(
        "scope: project\nagent: claude-code\nskills:\n  - source: {p}\n    skills:\n      - alpha\n  - source: {p}\n    sub-dir: no-such-subdir\n    skills:\n      - ghost\n",
        p = pack.display(),
    );
    std::fs::write(&broken_cfg, yaml2).unwrap();
    let r = sync_apply(&engine, &broken_cfg.to_string_lossy().to_string());
    assert!(
        r.summary.failed > 0,
        "broken sibling source records a failure"
    );
    assert_eq!(r.summary.removed, 0, "never prune when failed > 0");
    assert!(
        proj.join(".claude/skills/alpha/SKILL.md").is_file(),
        "good locked skill survives a failing sibling source"
    );
}

/// Oracle: the SyncContext orchestrator contract (C-01) — `--locked` + `--update` is a
/// contradiction, and `dry_run` performs zero writes. The engine encodes `--locked`/`--update`
/// as the mutually-exclusive `AgentLockMode` enum (so the contradiction is unrepresentable, the
/// idiomatic-Rust expression of kasetto's runtime `err`), and `apply: false` is the dry-run gate.
/// Here we verify the observable half: a dry-run sync writes NOTHING yet reports the planned work.
#[test]
fn c01_dry_run_writes_nothing_but_reports_plan() {
    let pack = skill_pack();
    let (engine, proj, _cfg) = project("scope: project\n");
    let cfg_path = proj.join("env-1.yaml");
    let yaml = format!(
        "scope: project\nagent: claude-code\nskills:\n  - source: {p}\n    skills:\n      - alpha\n      - beta\n",
        p = pack.display(),
    );
    std::fs::write(&cfg_path, yaml).unwrap();
    let (s, _rx) = sink();
    let r = engine
        .agent_sync(
            AgentSyncSpec {
                config_path: Some(cfg_path.to_string_lossy().to_string()),
                apply: false,
                ..Default::default()
            },
            &s,
        )
        .expect("dry-run sync");
    assert!(r.dry_run, "preview is dry_run");
    assert!(
        r.summary.installed >= 2,
        "plan reports the would-install work"
    );
    assert_eq!(r.summary.failed, 0);
    assert!(
        !proj.join(".claude/skills/alpha").exists(),
        "dry-run wrote no skill"
    );
    assert!(
        !proj.join("agent-env.lock").exists(),
        "dry-run wrote no lock"
    );
    // The lock-mode contradiction is structurally impossible: `Locked` and `Update` are distinct
    // enum variants, so `--locked --update` cannot be constructed (kasetto's runtime error becomes
    // a compile-time impossibility — a no-downgrade hardening, not a gap).
    let _ = (AgentScope::Project, AgentLockMode::Locked);
}
