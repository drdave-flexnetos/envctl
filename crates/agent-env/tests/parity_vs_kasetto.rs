//! Differential parity harness: `envctl-agent-env` vs kasetto v3.2.0 (the source).
//!
//! Each assertion encodes a GOLDEN VECTOR taken verbatim from kasetto's own
//! `#[cfg(test)]` modules (cited `kasetto src/<file>:<test>`). Those vectors are the
//! source's CERTIFIED behavior: `cargo test` in `meta/kasetto` @ v3.2.0 (`ec01cca`)
//! passes all 216 of them. This harness feeds the SAME inputs through agent-env's
//! PUBLIC API and asserts agent-env reproduces kasetto's expected outputs. A mismatch
//! here is a genuine port defect (parity FAIL), independent of agent-env's own tests.
//!
//! This is the parity-verifier pass that flips `[~]` → `[x]` in
//! `.handoff/loop/rust-port/parity-ledger.md` (and merge-ledger.md).

use envctl_agent_env::{
    all_command_global_targets, all_mcp_project_targets, all_mcp_settings_targets, archive_url,
    clear_runtime_state, command_global_targets, command_project_targets, default_config_path,
    derive_browse_url, dirs_agent_env_cache, dirs_agent_env_config, dirs_agent_env_data,
    dirs_xdg_cache_home, dirs_xdg_config_home, dirs_xdg_data_home, discover, discover_commands,
    discover_mcps, discover_with_root_name, extract_extends, format_updated_ago, git_pin_of,
    hash_dir, hash_file, hash_str, load_config_any, load_config_recursive, load_runtime_state,
    materialize_source, merge_mcp_config, merge_yaml, now_unix, now_unix_str, parse,
    parse_repo_url, read_skill_profile, read_skill_profile_from_dir, render, resolve_command_entry,
    resolve_config_path, resolve_mcp_entry, runtime_state_path, save_runtime_state, Action, Agent,
    AgentField, AgentLockEntry, AgentLockFile, AssetEntry, CommandEntry, CommandFormat,
    CommandSourceSpec, Config, GitPin, InstalledSkill, LockMode, McpEntry, McpSettingsFormat,
    McpSettingsTarget, McpSourceSpec, McpsField, RepoUrl, Report, RuntimeState, Scope, SkillTarget,
    SkillsField, SourceSpec, Summary, SyncFailure, AGENT_PRESETS, CONFIG_ENV_VAR,
    DEFAULT_CONFIG_FILENAME, DEFAULT_GLOBAL_CONFIG_FILENAME, LOCK_VERSION, PREFERENCES_FILENAME,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

// A process-wide lock so the GITHUB_TOKEN-sensitive archive tests don't race each
// other (kasetto guards these with the same pattern — src/source/remote.rs:ENV_LOCK).
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn tmp(tag: &str) -> PathBuf {
    let mut d = std::env::temp_dir();
    d.push(format!("agentenv-parity-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

// ───────────────────────────── S-01/S-02 · parse_repo_url ─────────────────────────────
// Oracle: kasetto src/source/parse.rs::tests
#[test]
fn parity_parse_repo_url() {
    // parse_repo_url_github
    assert!(
        matches!(parse_repo_url("https://github.com/openai/skills").unwrap(),
        RepoUrl::GitHub{host,owner,repo} if host=="github.com" && owner=="openai" && repo=="skills")
    );
    // parse_repo_url_github_enterprise_two_segment_path
    assert!(
        matches!(parse_repo_url("https://ghe.example.com/acme/skill-pack").unwrap(),
        RepoUrl::GitHub{host,owner,repo} if host=="ghe.example.com" && owner=="acme" && repo=="skill-pack")
    );
    // parse_repo_url_github_trims_git_and_trailing_slash
    assert!(
        matches!(parse_repo_url("https://github.com/pivoshenko/kasetto.git/").unwrap(),
        RepoUrl::GitHub{host,owner,repo} if host=="github.com" && owner=="pivoshenko" && repo=="kasetto")
    );
    // parse_repo_url_gitlab (subgroup)
    assert!(
        matches!(parse_repo_url("https://gitlab.example.com/group/subgroup/repo").unwrap(),
        RepoUrl::GitLab{host,project_path} if host=="gitlab.example.com" && project_path=="group/subgroup/repo")
    );
    // parse_repo_url_gitlab_com_two_segments
    assert!(
        matches!(parse_repo_url("https://gitlab.com/group/project").unwrap(),
        RepoUrl::GitLab{host,project_path} if host=="gitlab.com" && project_path=="group/project")
    );
    // parse_repo_url_bitbucket_cloud
    assert!(
        matches!(parse_repo_url("https://bitbucket.org/workspace/skill-repo").unwrap(),
        RepoUrl::Bitbucket{workspace,repo_slug} if workspace=="workspace" && repo_slug=="skill-repo")
    );
    // parse_repo_url_codeberg
    assert!(
        matches!(parse_repo_url("https://codeberg.org/someone/skills").unwrap(),
        RepoUrl::Gitea{host,owner,repo} if host=="codeberg.org" && owner=="someone" && repo=="skills")
    );
}

// ───────────────────────────── S-03 · derive_browse_url ─────────────────────────────
// Oracle: kasetto src/source/parse.rs::tests (derive_*)
#[test]
fn parity_derive_browse_url() {
    let d = derive_browse_url(
        "https://github.com/mattpocock/skills/blob/main/skills/personal/edit-article/SKILL.md",
    )
    .unwrap();
    assert_eq!(d.source, "https://github.com/mattpocock/skills");
    assert_eq!(d.branch.as_deref(), Some("main"));
    assert_eq!(d.git_ref, None);
    assert_eq!(d.sub_dir.as_deref(), Some("skills/personal"));
    assert_eq!(d.skill_name.as_deref(), Some("edit-article"));

    let d = derive_browse_url("https://github.com/mattpocock/skills/tree/main/skills/personal")
        .unwrap();
    assert_eq!(d.source, "https://github.com/mattpocock/skills");
    assert_eq!(d.branch.as_deref(), Some("main"));
    assert_eq!(d.sub_dir.as_deref(), Some("skills/personal"));
    assert_eq!(d.skill_name, None);

    // derive_sha_ref_is_pinned_not_branch
    let sha = "a".repeat(40);
    let d = derive_browse_url(&format!("https://github.com/o/r/tree/{sha}/pack")).unwrap();
    assert_eq!(d.git_ref.as_deref(), Some(sha.as_str()));
    assert_eq!(d.branch, None);

    // derive_gitlab_dash_separator
    let d = derive_browse_url("https://gitlab.com/group/proj/-/tree/main/skills/a").unwrap();
    assert_eq!(d.source, "https://gitlab.com/group/proj");
    assert_eq!(d.branch.as_deref(), Some("main"));
    assert_eq!(d.sub_dir.as_deref(), Some("skills/a"));

    // derive_plain_repo_url_is_none
    assert_eq!(derive_browse_url("https://github.com/owner/repo"), None);
    assert_eq!(derive_browse_url("./local/pack"), None);
}

// ───────────────────────────── S-08 · rewrite_browse_to_raw_url ─────────────────────────────
// Oracle: kasetto src/source/remote.rs::tests (rewrite_*)
#[test]
fn parity_rewrite_browse_to_raw_url() {
    use envctl_agent_env::rewrite_browse_to_raw_url as rw;
    assert_eq!(
        rw("https://github.com/pivoshenko/kasetto/blob/main/kasetto.yml").unwrap(),
        "https://raw.githubusercontent.com/pivoshenko/kasetto/main/kasetto.yml"
    );
    assert_eq!(
        rw("https://github.com/owner/repo/blob/v1.2.3/configs/kasetto.yml").unwrap(),
        "https://raw.githubusercontent.com/owner/repo/v1.2.3/configs/kasetto.yml"
    );
    assert_eq!(
        rw("https://github.com/owner/repo/raw/main/kasetto.yml").unwrap(),
        "https://raw.githubusercontent.com/owner/repo/main/kasetto.yml"
    );
    assert!(rw("https://github.com/owner/repo").is_none());
    assert_eq!(
        rw("https://codeberg.org/owner/repo/src/branch/main/kasetto.yml").unwrap(),
        "https://codeberg.org/owner/repo/raw/branch/main/kasetto.yml"
    );
    assert_eq!(
        rw("https://codeberg.org/owner/repo/src/tag/v1.0.0/configs/kasetto.yml").unwrap(),
        "https://codeberg.org/owner/repo/raw/tag/v1.0.0/configs/kasetto.yml"
    );
    assert_eq!(rw("https://gitlab.com/group/sub/repo/-/blob/main/kasetto.yml").unwrap(),
        "https://gitlab.com/api/v4/projects/group%2Fsub%2Frepo/repository/files/kasetto.yml/raw?ref=main");
    assert!(rw("https://example.com/some/path").is_none());
    assert!(rw("git@github.com:owner/repo.git").is_none());
}

// ───────────────────────────── S-04/S-05/S-06 · archive_url ─────────────────────────────
// Oracle: kasetto src/source/remote.rs::tests (github/bitbucket/gitea archive)
#[test]
fn parity_archive_url() {
    let _g = ENV_LOCK.lock().unwrap();
    let gh = RepoUrl::GitHub {
        host: "github.com".into(),
        owner: "o".into(),
        repo: "r".into(),
    };

    // No token → web archive (refs/heads for branch; short form for ref).
    std::env::remove_var("GITHUB_TOKEN");
    std::env::remove_var("GH_TOKEN");
    assert_eq!(
        archive_url(&gh, &GitPin::Branch("main".into())).0,
        "https://github.com/o/r/archive/refs/heads/main.tar.gz"
    );
    assert_eq!(
        archive_url(&gh, &GitPin::Ref("v2.0".into())).0,
        "https://github.com/o/r/archive/v2.0.tar.gz"
    );
    assert_eq!(
        archive_url(&gh, &GitPin::Ref("abc123def".into())).0,
        "https://github.com/o/r/archive/abc123def.tar.gz"
    );

    // With token → api.github.com tarball, ref %2F-encoded.
    std::env::set_var("GITHUB_TOKEN", "test-token");
    assert_eq!(
        archive_url(&gh, &GitPin::Branch("main".into())).0,
        "https://api.github.com/repos/o/r/tarball/main"
    );
    assert_eq!(
        archive_url(&gh, &GitPin::Branch("feature/foo".into())).0,
        "https://api.github.com/repos/o/r/tarball/feature%2Ffoo"
    );
    assert_eq!(
        archive_url(&gh, &GitPin::Ref("refs/tags/release/1.2".into())).0,
        "https://api.github.com/repos/o/r/tarball/refs%2Ftags%2Frelease%2F1.2"
    );
    std::env::remove_var("GITHUB_TOKEN");

    // Bitbucket + Gitea archive URLs (token-independent).
    let bb = RepoUrl::Bitbucket {
        workspace: "ws".into(),
        repo_slug: "myrepo".into(),
    };
    assert_eq!(
        archive_url(&bb, &GitPin::Branch("main".into())).0,
        "https://bitbucket.org/ws/myrepo/get/main.tar.gz"
    );
    let gt = RepoUrl::Gitea {
        host: "codeberg.org".into(),
        owner: "a".into(),
        repo: "b".into(),
    };
    assert_eq!(
        archive_url(&gt, &GitPin::Branch("main".into())).0,
        "https://codeberg.org/a/b/archive/main.tar.gz"
    );

    // git_pin_of precedence (ref > branch > default) — kasetto config.rs git_pin.
    assert!(matches!(git_pin_of(Some("v1"), Some("b")), GitPin::Ref(r) if r=="v1"));
    assert!(matches!(git_pin_of(None, Some("b")), GitPin::Branch(b) if b=="b"));
    assert!(matches!(git_pin_of(None, None), GitPin::Default));
}

// ───────────────────────────── F-01/F-02 · hashing (3rd-party oracle: sha256sum) ─────────────────────────────
// Oracle: independent SHA-256 (the standard), matching kasetto src/fsops/hash.rs framing.
#[test]
fn parity_hash_str_and_file_vs_sha256() {
    // SHA-256("hello") — canonical published digest, independent of either Rust impl.
    assert_eq!(
        hash_str("hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    // SHA-256("") empty.
    assert_eq!(
        hash_str(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    // hash_file == SHA-256 of file bytes.
    let dir = tmp("hashfile");
    let f = dir.join("x.txt");
    fs::write(&f, "hello").unwrap();
    assert_eq!(
        hash_file(&f).unwrap(),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    let _ = fs::remove_dir_all(&dir);
}

// hash_dir framing (rel + NUL + content + NUL, sorted, sep-invariant) reproduced by an
// independent SHA-256 over the SAME byte stream — a 3rd-party oracle for F-01.
#[test]
fn parity_hash_dir_vs_independent_framing() {
    use sha2::{Digest, Sha256};
    let root = tmp("hashdir");
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("SKILL.md"), "# Demo\n").unwrap();
    fs::write(root.join("sub/extra.md"), "body\n").unwrap();

    // Independent reconstruction of kasetto's documented framing (src/fsops/hash.rs:hash_dir):
    // collect files, sort by rel path, feed `rel\0 content \0` per file (rel '\\'→'/').
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("SKILL.md".to_string(), b"# Demo\n".to_vec()),
        ("sub/extra.md".to_string(), b"body\n".to_vec()),
    ];
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Sha256::new();
    for (rel, content) in &files {
        h.update(rel.replace('\\', "/").as_bytes());
        h.update([0u8]);
        h.update(content);
        h.update([0u8]);
    }
    let expected = format!("{:x}", h.finalize());
    assert_eq!(
        hash_dir(&root).unwrap(),
        expected,
        "hash_dir framing must match kasetto's"
    );
    // stability across runs (kasetto hash_dir_is_stable_across_runs)
    assert_eq!(hash_dir(&root).unwrap(), hash_dir(&root).unwrap());
    let _ = fs::remove_dir_all(&root);
}

// ───────────────────────────── X-01/X-02/X-03 · extends + merge_yaml ─────────────────────────────
// Oracle: kasetto src/model/extend.rs::tests
#[test]
fn parity_extract_extends_and_merge() {
    use serde_yaml::Value;
    let y = |s: &str| -> Value { serde_yaml::from_str(s).unwrap() };

    let mut v = y("extends: ../base.yaml\nskills: []\n");
    assert_eq!(extract_extends(&mut v), vec!["../base.yaml".to_string()]);
    assert!(matches!(&v, Value::Mapping(m) if !m.contains_key(Value::String("extends".into()))));

    let mut v = y("extends:\n  - a.yaml\n  - https://x/b.yaml\nskills: []\n");
    assert_eq!(extract_extends(&mut v), vec!["a.yaml", "https://x/b.yaml"]);

    let mut v = y("skills: []\n");
    assert!(extract_extends(&mut v).is_empty());

    // merge_replaces_scalars
    let m = merge_yaml(
        y("scope: global\nagent: cursor\nskills: []\n"),
        y("scope: project\nskills: []\n"),
    );
    assert_eq!(m.get("scope").and_then(Value::as_str), Some("project"));
    assert_eq!(m.get("agent").and_then(Value::as_str), Some("cursor"));

    let seq_len = |m: &Value, k: &str| -> usize {
        match m.get(k).unwrap() {
            Value::Sequence(s) => s.len(),
            _ => panic!("expected seq"),
        }
    };
    // append distinct sources
    let m = merge_yaml(
        y("skills:\n  - source: https://x/a\n    skills: \"*\"\n"),
        y("skills:\n  - source: https://x/b\n    skills: \"*\"\n"),
    );
    assert_eq!(seq_len(&m, "skills"), 2);
    // override same identity (source+ref) → replaced
    let m = merge_yaml(
        y("skills:\n  - source: https://x/a\n    ref: v1\n    skills: \"*\"\n"),
        y("skills:\n  - source: https://x/a\n    ref: v1\n    skills:\n      - one\n"),
    );
    assert_eq!(seq_len(&m, "skills"), 1);
    // distinct refs kept separate
    let m = merge_yaml(
        y("skills:\n  - source: https://x/a\n    ref: v1\n    skills: \"*\"\n"),
        y("skills:\n  - source: https://x/a\n    ref: v2\n    skills: \"*\"\n"),
    );
    assert_eq!(seq_len(&m, "skills"), 2);
    // distinct sub-dirs kept separate
    let m = merge_yaml(
        y("skills:\n  - source: https://x/a\n    sub-dir: pack-a\n    skills: \"*\"\n"),
        y("skills:\n  - source: https://x/a\n    sub-dir: pack-b\n    skills: \"*\"\n"),
    );
    assert_eq!(seq_len(&m, "skills"), 2);
    // mcps + commands use the same identity rules
    let m = merge_yaml(
        y("mcps:\n  - source: https://x/a\n    ref: v1\n    mcps: \"*\"\n"),
        y("mcps:\n  - source: https://x/a\n    ref: v1\n    mcps:\n      - github\n"),
    );
    assert_eq!(seq_len(&m, "mcps"), 1);
    let m = merge_yaml(
        y("commands:\n  - source: https://x/a\n    commands: \"*\"\n"),
        y("commands:\n  - source: https://x/b\n    commands: \"*\"\n"),
    );
    assert_eq!(seq_len(&m, "commands"), 2);
    // base-only keys preserved
    let m = merge_yaml(
        y("destination: ./skills\nskills: []\n"),
        y("scope: project\nskills: []\n"),
    );
    assert_eq!(
        m.get("destination").and_then(Value::as_str),
        Some("./skills")
    );
    assert_eq!(m.get("scope").and_then(Value::as_str), Some("project"));
}

// ───────────────────────────── M-02/M-04/M-07/M-08 · Config parse ─────────────────────────────
// Oracle: kasetto src/model/config.rs::tests
#[test]
fn parity_config_parse() {
    use envctl_agent_env::{CommandEntry, CommandsField};
    let cfg: Config = serde_yaml::from_str(
        "skills: []\ncommands:\n  - source: https://github.com/me/cmds\n    commands: \"*\"\n",
    )
    .unwrap();
    assert_eq!(cfg.commands.len(), 1);
    assert!(matches!(
        cfg.commands[0].commands,
        CommandsField::Wildcard(_)
    ));

    let cfg: Config = serde_yaml::from_str(
        "skills: []\ncommands:\n  - source: https://github.com/me/cmds\n    ref: v1.0\n    sub-dir: commands\n    commands:\n      - review-pr\n      - name: deploy\n        path: ops\n").unwrap();
    assert_eq!(cfg.commands[0].git_ref.as_deref(), Some("v1.0"));
    assert_eq!(cfg.commands[0].sub_dir.as_deref(), Some("commands"));
    let CommandsField::List(ref entries) = cfg.commands[0].commands else {
        panic!("list")
    };
    assert_eq!(entries.len(), 2);
    assert!(matches!(&entries[0], CommandEntry::Name(n) if n == "review-pr"));
    assert!(
        matches!(&entries[1], CommandEntry::Obj{name, path: Some(p)} if name=="deploy" && p=="ops")
    );

    // sub_dir alias
    let cfg: Config = serde_yaml::from_str(
        "skills: []\ncommands:\n  - source: https://github.com/me/cmds\n    sub_dir: nested/commands\n    commands: \"*\"\n").unwrap();
    assert_eq!(cfg.commands[0].sub_dir.as_deref(), Some("nested/commands"));
}

// ───────────────────────────── M-15/M-16/M-18 · 21-agent path table ─────────────────────────────
// Oracle: kasetto src/model/agent.rs::tests
#[test]
fn parity_agent_command_path_table() {
    let home = Path::new("/tmp/home");
    assert_eq!(
        Agent::ClaudeCode.commands_global_path(home).unwrap().path,
        home.join(".claude/commands")
    );
    assert_eq!(
        Agent::Windsurf.commands_global_path(home).unwrap().path,
        home.join(".codeium/windsurf/global_workflows")
    );
    assert_eq!(
        Agent::GeminiCli.commands_global_path(home).unwrap().format,
        CommandFormat::GeminiToml
    );
    assert!(Agent::Cursor.commands_global_path(home).is_none());
    assert!(Agent::Trae.commands_global_path(home).is_none());

    let pr = Path::new("/work");
    assert_eq!(
        Agent::Cursor.commands_project_path(pr).unwrap().path,
        pr.join(".cursor/commands")
    );
    assert_eq!(
        Agent::Cursor.commands_project_path(pr).unwrap().format,
        CommandFormat::MarkdownPlain
    );
    assert_eq!(
        Agent::GithubCopilot
            .commands_project_path(pr)
            .unwrap()
            .format,
        CommandFormat::PromptMd
    );
    assert!(Agent::Codex.commands_project_path(pr).is_none());
    assert!(Agent::Warp.commands_project_path(pr).is_none());

    // all_command_global_targets dedupes and sorts
    let all = all_command_global_targets(home);
    assert!(!all.is_empty());
    for w in all.windows(2) {
        assert!(w[0].path <= w[1].path);
    }
}

// ───────────────────────────── MC-01/MC-02 · additive MCP merge (no-clobber) ─────────────────────────────
// Oracle: kasetto src/mcps/mod.rs::tests — the #1 no-downgrade invariant.
#[test]
fn parity_mcp_merge_additive_no_clobber() {
    let mcp_target = |p: PathBuf| McpSettingsTarget {
        path: p,
        format: McpSettingsFormat::McpServers,
    };
    let dir = tmp("mcp");

    // create from scratch
    let src = dir.join("source.json");
    let tgt = dir.join("settings.json");
    fs::write(
        &src,
        r#"{"mcpServers":{"git-tools":{"command":"git-mcp"}}}"#,
    )
    .unwrap();
    merge_mcp_config(&src, &mcp_target(tgt.clone())).unwrap();
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&tgt).unwrap()).unwrap();
    assert_eq!(v["mcpServers"]["git-tools"]["command"], "git-mcp");

    // preserve existing servers
    fs::write(&tgt, r#"{"mcpServers":{"existing":{"command":"keep-me"}}}"#).unwrap();
    fs::write(
        &src,
        r#"{"mcpServers":{"new-server":{"command":"new-cmd"}}}"#,
    )
    .unwrap();
    merge_mcp_config(&src, &mcp_target(tgt.clone())).unwrap();
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&tgt).unwrap()).unwrap();
    assert_eq!(v["mcpServers"]["existing"]["command"], "keep-me");
    assert_eq!(v["mcpServers"]["new-server"]["command"], "new-cmd");

    // does NOT overwrite an existing server's key (real secret preserved)
    fs::write(
        &tgt,
        r#"{"mcpServers":{"airflow":{"command":"uvx","env":{"AIRFLOW_PASSWORD":"real-secret"}}}}"#,
    )
    .unwrap();
    fs::write(&src, r#"{"mcpServers":{"airflow":{"command":"uvx","env":{"AIRFLOW_PASSWORD":"__FROM_SOURCE_PACK__"}}}}"#).unwrap();
    merge_mcp_config(&src, &mcp_target(tgt.clone())).unwrap();
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&tgt).unwrap()).unwrap();
    assert_eq!(
        v["mcpServers"]["airflow"]["env"]["AIRFLOW_PASSWORD"],
        "real-secret"
    );

    // CodexToml format (MC-02) writes [mcp_servers] table
    let codex_src = dir.join("codex_source.json");
    let codex_tgt = dir.join("config.toml");
    fs::write(
        &codex_src,
        r#"{"mcpServers":{"demo":{"command":"uvx","args":["p"],"env":{"K":"v"}}}}"#,
    )
    .unwrap();
    merge_mcp_config(
        &codex_src,
        &McpSettingsTarget {
            path: codex_tgt.clone(),
            format: McpSettingsFormat::CodexToml,
        },
    )
    .unwrap();
    let parsed: toml::Value = fs::read_to_string(&codex_tgt).unwrap().parse().unwrap();
    let mcp = parsed.get("mcp_servers").unwrap().as_table().unwrap();
    assert_eq!(mcp["demo"]["command"].as_str().unwrap(), "uvx");
    assert_eq!(
        mcp["demo"]["args"].as_array().unwrap()[0].as_str().unwrap(),
        "p"
    );
    assert_eq!(mcp["demo"]["env"]["K"].as_str().unwrap(), "v");

    let _ = fs::remove_dir_all(&dir);
}

// ───────────────────────────── PR-01 · 5 command-format transforms ─────────────────────────────
// Oracle: kasetto src/prompts/transform.rs::tests + parse.rs::tests
#[test]
fn parity_command_transforms() {
    let sample =
        parse("---\ndescription: do thing\nargument-hint: <n>\n---\nUse $ARGUMENTS here.\n")
            .unwrap();

    let r = render(&sample, CommandFormat::MarkdownFrontmatter);
    assert!(
        r.starts_with("---\n")
            && r.contains("description: do thing")
            && r.contains("Use $ARGUMENTS here.")
    );

    let r = render(&sample, CommandFormat::MarkdownPlain);
    assert!(!r.contains("description:") && r.contains("Use $ARGUMENTS here."));

    let r = render(&sample, CommandFormat::PromptMd);
    assert!(r.starts_with("---\n") && r.contains("description: do thing"));

    let r = render(&sample, CommandFormat::PromptFile);
    assert!(
        r.contains("invokable: true") && r.contains("{{{ input }}}") && !r.contains("$ARGUMENTS")
    );

    // no double invokable
    let p = parse("---\ninvokable: false\n---\nx\n").unwrap();
    let r = render(&p, CommandFormat::PromptFile);
    assert_eq!(r.matches("invokable:").count(), 1);
    assert!(r.contains("invokable: false"));

    let r = render(&sample, CommandFormat::GeminiToml);
    assert!(
        r.contains("description = \"do thing\"")
            && r.contains("prompt = \"\"\"")
            && r.contains("Use $ARGUMENTS here.")
    );

    // parse: frontmatter/body split + description()
    let p = parse("---\ndescription: hi\nargument-hint: <n>\n---\nBody here.\n").unwrap();
    assert!(p
        .frontmatter
        .as_deref()
        .unwrap()
        .contains("description: hi"));
    assert_eq!(p.body, "Body here.\n");
    assert_eq!(p.description().as_deref(), Some("hi"));
}

// ───────────────────────────── PR-01 · destination_path relpath shapes ─────────────────────────────
// Oracle: kasetto src/prompts/transform.rs::{nested_paths_for_markdown_frontmatter, flat_names_for_other_formats}
#[test]
fn parity_command_destination_relpath() {
    use envctl_agent_env::{destination_path, CommandTarget};
    let base = Path::new("/b");
    let rel = |fmt, name| -> PathBuf {
        destination_path(
            &CommandTarget {
                path: base.to_path_buf(),
                format: fmt,
            },
            name,
        )
        .strip_prefix(base)
        .unwrap()
        .to_path_buf()
    };
    // MarkdownFrontmatter keeps `:`-namespaced names as nested dirs.
    assert_eq!(
        rel(CommandFormat::MarkdownFrontmatter, "git:commit"),
        PathBuf::from("git/commit.md")
    );
    assert_eq!(
        rel(CommandFormat::MarkdownFrontmatter, "commit"),
        PathBuf::from("commit.md")
    );
    // Other formats flatten with `-`.
    assert_eq!(
        rel(CommandFormat::MarkdownPlain, "git:commit"),
        PathBuf::from("git-commit.md")
    );
    assert_eq!(
        rel(CommandFormat::PromptMd, "git:commit"),
        PathBuf::from("git-commit.prompt.md")
    );
    assert_eq!(
        rel(CommandFormat::PromptFile, "git:commit"),
        PathBuf::from("git-commit.prompt")
    );
    assert_eq!(
        rel(CommandFormat::GeminiToml, "git:commit"),
        PathBuf::from("git-commit.toml")
    );
}

// ───────────────────────────── S-09/S-10/S-11 · env-only auth (no-clobber creds) ─────────────────────────────
// Oracle: kasetto src/source/remote.rs::tests (github_branch_archive_uses_{refs_heads,api_endpoint}*)
// + src/source/auth.rs (github/gitlab/bitbucket/gitea cred readers).
//
// The env-credential readers + UrlRequestAuth assembly (S-09/S-10) and host-classified
// auth selection (S-11) are `pub(crate)` in both kasetto and agent-env, so they are NOT
// directly reachable. They ARE observable through the PUBLIC `archive_url`, whose returned
// `UrlRequestAuth` carries the env-derived `headers`/`basic` (public fields). kasetto's own
// remote.rs tests assert the *same* env→header behavior through the same seam (the token
// flips the GitHub URL to the api.github.com endpoint, proving the Bearer header was built).
#[test]
fn parity_auth_env_credentials_via_archive_url() {
    let _g = ENV_LOCK.lock().unwrap();

    // Save + clear every env var these readers consult, restore at the end (no leak).
    let keys = [
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "GITLAB_TOKEN",
        "CI_JOB_TOKEN",
        "BITBUCKET_EMAIL",
        "BITBUCKET_TOKEN",
        "BITBUCKET_USERNAME",
        "BITBUCKET_APP_PASSWORD",
        "GITEA_TOKEN",
        "CODEBERG_TOKEN",
        "FORGEJO_TOKEN",
    ];
    let saved: Vec<(&str, Option<String>)> =
        keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();
    for k in keys {
        std::env::remove_var(k);
    }

    let gh = RepoUrl::GitHub {
        host: "github.com".into(),
        owner: "o".into(),
        repo: "r".into(),
    };
    let gl = RepoUrl::GitLab {
        host: "gitlab.com".into(),
        project_path: "g/p".into(),
    };
    let bb = RepoUrl::Bitbucket {
        workspace: "ws".into(),
        repo_slug: "r".into(),
    };
    let gt = RepoUrl::Gitea {
        host: "codeberg.org".into(),
        owner: "a".into(),
        repo: "b".into(),
    };

    // No creds anywhere → empty headers / no basic (S-09/S-10 absent arm).
    let (_, auth) = archive_url(&gh, &GitPin::Branch("main".into()));
    assert!(auth.headers.is_empty() && auth.basic.is_none());

    // GITHUB_TOKEN → Authorization: Bearer <token> (S-10 github_auth_headers).
    std::env::set_var("GITHUB_TOKEN", "ghtok");
    let (_, auth) = archive_url(&gh, &GitPin::Branch("main".into()));
    assert_eq!(
        auth.headers,
        vec![("Authorization".to_string(), "Bearer ghtok".to_string())]
    );
    std::env::remove_var("GITHUB_TOKEN");

    // GH_TOKEN is the fallback key (first_env_var order: GITHUB_TOKEN then GH_TOKEN).
    std::env::set_var("GH_TOKEN", "ghtok2");
    let (_, auth) = archive_url(&gh, &GitPin::Branch("main".into()));
    assert_eq!(
        auth.headers,
        vec![("Authorization".to_string(), "Bearer ghtok2".to_string())]
    );
    std::env::remove_var("GH_TOKEN");

    // GITLAB_TOKEN → PRIVATE-TOKEN; CI_JOB_TOKEN → JOB-TOKEN (S-10 gitlab_auth_headers).
    std::env::set_var("GITLAB_TOKEN", "gltok");
    let (_, auth) = archive_url(&gl, &GitPin::Branch("main".into()));
    assert_eq!(
        auth.headers,
        vec![("PRIVATE-TOKEN".to_string(), "gltok".to_string())]
    );
    std::env::remove_var("GITLAB_TOKEN");
    std::env::set_var("CI_JOB_TOKEN", "jobtok");
    let (_, auth) = archive_url(&gl, &GitPin::Branch("main".into()));
    assert_eq!(
        auth.headers,
        vec![("JOB-TOKEN".to_string(), "jobtok".to_string())]
    );
    std::env::remove_var("CI_JOB_TOKEN");

    // BITBUCKET_EMAIL+TOKEN → basic; USERNAME+APP_PASSWORD is the fallback (S-10).
    std::env::set_var("BITBUCKET_EMAIL", "me@x.io");
    std::env::set_var("BITBUCKET_TOKEN", "bbtok");
    let (_, auth) = archive_url(&bb, &GitPin::Branch("main".into()));
    assert_eq!(
        auth.basic,
        Some(("me@x.io".to_string(), "bbtok".to_string()))
    );
    assert!(auth.headers.is_empty());
    std::env::remove_var("BITBUCKET_EMAIL");
    std::env::remove_var("BITBUCKET_TOKEN");
    std::env::set_var("BITBUCKET_USERNAME", "user");
    std::env::set_var("BITBUCKET_APP_PASSWORD", "pass");
    let (_, auth) = archive_url(&bb, &GitPin::Branch("main".into()));
    assert_eq!(auth.basic, Some(("user".to_string(), "pass".to_string())));
    std::env::remove_var("BITBUCKET_USERNAME");
    std::env::remove_var("BITBUCKET_APP_PASSWORD");

    // GITEA/CODEBERG/FORGEJO_TOKEN → Authorization: token <t> (S-10 gitea_auth_headers).
    std::env::set_var("GITEA_TOKEN", "gttok");
    let (_, auth) = archive_url(&gt, &GitPin::Branch("main".into()));
    assert_eq!(
        auth.headers,
        vec![("Authorization".to_string(), "token gttok".to_string())]
    );
    std::env::remove_var("GITEA_TOKEN");

    // S-11: host classification routes auth — a GitLab repo never receives a GitHub
    // Bearer even when GITHUB_TOKEN is set (auth_for_request_url / for_*_archive split).
    std::env::set_var("GITHUB_TOKEN", "ghtok");
    let (_, auth_gl) = archive_url(&gl, &GitPin::Branch("main".into()));
    assert!(auth_gl.headers.is_empty() && auth_gl.basic.is_none());
    std::env::remove_var("GITHUB_TOKEN");

    // restore
    for (k, v) in saved {
        match v {
            Some(val) => std::env::set_var(k, val),
            None => std::env::remove_var(k),
        }
    }
}

// ───────────────────────────── S-14/S-16 · materialize_source (local) + sub-dir un-nest ─────────────────────────────
// Oracle: kasetto src/source/mod.rs::tests::{local_materialize_does_not_set_cleanup_dir,
// local_materialize_supports_sub_dir}. resolve_source_root (S-16) is `fn`-private in both;
// it is observed through the PUBLIC materialize_source sub-dir arm (the only public seam).
#[test]
fn parity_materialize_source_local() {
    let root = tmp("materialize-local");
    let skill_dir = root.join("demo-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "# Demo\n\nDesc\n").unwrap();

    let src = SourceSpec {
        source: root.to_string_lossy().to_string(),
        branch: None,
        git_ref: None,
        sub_dir: None,
        skills: SkillsField::Wildcard("*".to_string()),
    };
    let stage = tmp("materialize-stage");
    let m = materialize_source(&src, Path::new("/"), &stage).unwrap();
    // local source: revision "local", no cleanup, in-place, skill discovered, root preserved.
    assert_eq!(m.source_revision, "local");
    assert!(m.cleanup_dir.is_none());
    assert!(m.available.contains_key("demo-skill"));
    assert!(root.exists());
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&stage);

    // sub-dir un-nest (S-16 resolve_source_root via the public seam).
    let root = tmp("materialize-subdir");
    let nested = root.join("plugins/swift-apple-expert");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("SKILL.md"), "# Nested\n\nDesc\n").unwrap();
    let src = SourceSpec {
        source: root.to_string_lossy().to_string(),
        branch: None,
        git_ref: None,
        sub_dir: Some("plugins/swift-apple-expert".to_string()),
        skills: SkillsField::Wildcard("*".to_string()),
    };
    let stage = tmp("materialize-stage2");
    let m = materialize_source(&src, Path::new("/"), &stage).unwrap();
    assert!(m.available.contains_key("swift-apple-expert"));
    assert_eq!(m.available.get("swift-apple-expert").unwrap(), &nested);
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&stage);
}

// ───────────────────────────── S-17 · discover / discover_with_root_name ─────────────────────────────
// Oracle: kasetto src/source/mod.rs::tests::{discover_supports_root_level_skill_with_hint,
// discover_uses_local_directory_name_for_root_level_skill, discover_finds_skills_in_skills_subdir}.
#[test]
fn parity_discover_skills() {
    // root-level SKILL.md named by the hint.
    let root = tmp("discover-root-hint");
    fs::write(root.join("SKILL.md"), "# Root\n\nDesc\n").unwrap();
    let available = discover_with_root_name(&root, Some("raycast-script-creator")).unwrap();
    assert!(available.contains_key("raycast-script-creator"));
    assert_eq!(available.get("raycast-script-creator").unwrap(), &root);
    let _ = fs::remove_dir_all(&root);

    // discover() names the root skill by the directory's own name.
    let root = tmp("discover-root-local");
    fs::write(root.join("SKILL.md"), "# Root\n\nDesc\n").unwrap();
    let available = discover(&root).unwrap();
    let root_name = root.file_name().unwrap().to_string_lossy().to_string();
    assert!(available.contains_key(&root_name));
    assert_eq!(available.get(&root_name).unwrap(), &root);
    let _ = fs::remove_dir_all(&root);

    // scans both `<root>/` and `<root>/skills/`; a dir with no SKILL.md is not a skill.
    let root = tmp("discover-subdir");
    let a = root.join("skills/alpha");
    let b = root.join("beta");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(a.join("SKILL.md"), "x").unwrap();
    fs::write(b.join("SKILL.md"), "x").unwrap();
    fs::create_dir_all(root.join("skills/not-a-skill")).unwrap();
    let available = discover(&root).unwrap();
    assert!(available.contains_key("alpha"));
    assert!(available.contains_key("beta"));
    assert!(!available.contains_key("not-a-skill"));
    let _ = fs::remove_dir_all(&root);
}

// ───────────────────────────── S-18 · discover_mcps ─────────────────────────────
// Oracle: kasetto src/source/mod.rs::tests::discover_mcps_*.
#[test]
fn parity_discover_mcps() {
    let mcp_json = r#"{"mcpServers":{"tool":{"command":"x"}}}"#;

    // root .mcp.json
    let root = tmp("mcps-dot");
    fs::write(root.join(".mcp.json"), mcp_json).unwrap();
    let mcps = discover_mcps(&root).unwrap();
    assert_eq!(mcps.len(), 1);
    assert!(mcps[0].ends_with(".mcp.json"));
    let _ = fs::remove_dir_all(&root);

    // root mcp.json
    let root = tmp("mcps-plain");
    fs::write(root.join("mcp.json"), mcp_json).unwrap();
    let mcps = discover_mcps(&root).unwrap();
    assert_eq!(mcps.len(), 1);
    assert!(mcps[0].ends_with("mcp.json"));
    let _ = fs::remove_dir_all(&root);

    // root .mcp.json + mcps/extra.json → 2
    let root = tmp("mcps-both");
    fs::create_dir_all(root.join("mcps")).unwrap();
    fs::write(
        root.join(".mcp.json"),
        r#"{"mcpServers":{"a":{"command":"x"}}}"#,
    )
    .unwrap();
    fs::write(
        root.join("mcps/extra.json"),
        r#"{"mcpServers":{"b":{"command":"y"}}}"#,
    )
    .unwrap();
    let mcps = discover_mcps(&root).unwrap();
    assert_eq!(mcps.len(), 2);
    let _ = fs::remove_dir_all(&root);

    // nothing → empty
    let root = tmp("mcps-empty");
    assert!(discover_mcps(&root).unwrap().is_empty());
    let _ = fs::remove_dir_all(&root);
}

// ───────────────────────────── S-19 · resolve_mcp_entry ─────────────────────────────
// Oracle: kasetto src/source/mod.rs::tests::resolve_mcp_entry_*.
#[test]
fn parity_resolve_mcp_entry() {
    let payload = r#"{"mcpServers":{"s":{"command":"x"}}}"#;

    // Name → mcps/<name>.json
    let root = tmp("rme-name");
    fs::create_dir_all(root.join("mcps")).unwrap();
    fs::write(root.join("mcps/github.json"), payload).unwrap();
    let path = resolve_mcp_entry(&root, &McpEntry::Name("github".into())).unwrap();
    assert!(path.ends_with("mcps/github.json"));
    let _ = fs::remove_dir_all(&root);

    // auto-append .json, and explicit .json both resolve.
    let root = tmp("rme-ext");
    fs::create_dir_all(root.join("mcps")).unwrap();
    fs::write(root.join("mcps/linear.json"), payload).unwrap();
    assert!(resolve_mcp_entry(&root, &McpEntry::Name("linear".into()))
        .unwrap()
        .ends_with("linear.json"));
    assert!(
        resolve_mcp_entry(&root, &McpEntry::Name("linear.json".into()))
            .unwrap()
            .ends_with("linear.json")
    );
    let _ = fs::remove_dir_all(&root);

    // Obj{path} → <path>/<name>.json
    let root = tmp("rme-obj");
    fs::create_dir_all(root.join("tools")).unwrap();
    fs::write(root.join("tools/my-server.json"), payload).unwrap();
    let path = resolve_mcp_entry(
        &root,
        &McpEntry::Obj {
            name: "my-server".into(),
            path: Some("tools".into()),
        },
    )
    .unwrap();
    assert!(path.ends_with("tools/my-server.json"));
    let _ = fs::remove_dir_all(&root);

    // Obj{no path} → defaults to mcps/
    let root = tmp("rme-obj-default");
    fs::create_dir_all(root.join("mcps")).unwrap();
    fs::write(root.join("mcps/server.json"), payload).unwrap();
    let path = resolve_mcp_entry(
        &root,
        &McpEntry::Obj {
            name: "server".into(),
            path: None,
        },
    )
    .unwrap();
    assert!(path.ends_with("mcps/server.json"));
    let _ = fs::remove_dir_all(&root);

    // missing → err
    let root = tmp("rme-missing");
    assert!(resolve_mcp_entry(&root, &McpEntry::Name("nope".into())).is_err());
    let _ = fs::remove_dir_all(&root);
}

// ───────────────────────────── S-20/S-21 · discover_commands + resolve_command_entry ─────────────────────────────
// Oracle: kasetto src/source/mod.rs::tests::{discover_commands_walks_nested_subdirs,
// resolve_command_entry_name_uses_discovery, resolve_command_entry_obj_with_path}.
#[test]
fn parity_discover_and_resolve_commands() {
    // walk commands/**/*.md, `:`-namespacing nested dirs; non-.md ignored.
    let root = tmp("cmd-disc");
    fs::create_dir_all(root.join("commands/git/work")).unwrap();
    fs::write(root.join("commands/commit.md"), "---\n---\nbody\n").unwrap();
    fs::write(root.join("commands/git/commit.md"), "x").unwrap();
    fs::write(root.join("commands/git/work/status.md"), "x").unwrap();
    fs::write(root.join("commands/not-md.txt"), "ignored").unwrap();
    let map = discover_commands(&root).unwrap();
    assert_eq!(map.len(), 3);
    assert!(map.contains_key("commit"));
    assert!(map.contains_key("git:commit"));
    assert!(map.contains_key("git:work:status"));
    let _ = fs::remove_dir_all(&root);

    // Name → namespaced discovery lookup.
    let root = tmp("cmd-resolve");
    fs::create_dir_all(root.join("commands/git")).unwrap();
    fs::write(root.join("commands/git/commit.md"), "x").unwrap();
    let (name, path) =
        resolve_command_entry(&root, &CommandEntry::Name("git:commit".to_string())).unwrap();
    assert_eq!(name, "git:commit");
    assert!(path.ends_with("commands/git/commit.md"));
    let _ = fs::remove_dir_all(&root);

    // Obj{path} → <path>/<name>.md, name strips `.md`.
    let root = tmp("cmd-obj");
    fs::create_dir_all(root.join("ops")).unwrap();
    fs::write(root.join("ops/deploy.md"), "x").unwrap();
    let (name, path) = resolve_command_entry(
        &root,
        &CommandEntry::Obj {
            name: "deploy".to_string(),
            path: Some("ops".to_string()),
        },
    )
    .unwrap();
    assert_eq!(name, "deploy");
    assert!(path.ends_with("ops/deploy.md"));
    let _ = fs::remove_dir_all(&root);

    // missing → err.
    let root = tmp("cmd-missing");
    assert!(resolve_command_entry(&root, &CommandEntry::Name("nope".to_string())).is_err());
    let _ = fs::remove_dir_all(&root);
}

// ───────────────────────────── XC-04 · now_unix / now_unix_str ─────────────────────────────
// Oracle: kasetto src/fsops/mod.rs::tests::{now_unix_is_after_2020, now_unix_str_matches_now_unix}.
#[test]
fn parity_now_unix() {
    // 2020-01-01T00:00:00Z — guards against a zeroed/saturated clock regression.
    assert!(now_unix() > 1_577_836_800);
    let s = now_unix_str();
    let parsed: u64 = s.parse().expect("decimal");
    // The two calls straddle at most one second.
    assert!(now_unix() - parsed <= 1);
}

// ═════════════════════════════ PASS-2: model-schema + 21-preset path table ═════════════════════════════
// + config loader + SHA-256 asset lock. Vectors VERBATIM from kasetto v3.2.0 certified tests.

// ───────────────────────────── M-01 · Scope serde (global/project, #[default]=Global) ─────────────────────────────
// Oracle: kasetto src/model/config.rs (Scope enum) — serde renames `global`/`project`,
// #[default]=Global. Observed through Config::resolved_scope + a direct serde round-trip.
#[test]
fn parity_scope_serde_and_default() {
    // `scope: global` / `scope: project` deserialize to the renamed variants.
    let g: Config = serde_yaml::from_str("scope: global\nskills: []\n").unwrap();
    assert_eq!(g.resolved_scope(), Scope::Global);
    let p: Config = serde_yaml::from_str("scope: project\nskills: []\n").unwrap();
    assert_eq!(p.resolved_scope(), Scope::Project);
    // Absent scope → #[default] = Global (resolved_scope = scope.unwrap_or_default()).
    let none: Config = serde_yaml::from_str("skills: []\n").unwrap();
    assert_eq!(none.resolved_scope(), Scope::Global);
    assert_eq!(Scope::default(), Scope::Global);
    // serde rename round-trips: Global serializes to "global".
    assert_eq!(
        serde_yaml::to_string(&Scope::Global).unwrap().trim(),
        "global"
    );
    assert_eq!(
        serde_yaml::to_string(&Scope::Project).unwrap().trim(),
        "project"
    );
}

// ───────────────────────────── M-03 · Config::agents / resolved_scope ─────────────────────────────
// Oracle: kasetto src/model/config.rs::tests (agent_field_parses_one_and_many implied by
// AgentField) + config.rs resolved_scope. One→vec![a]; Many→clone; None→[]; resolved_scope =
// scope.unwrap_or_default() = Global.
#[test]
fn parity_config_agents_and_resolved_scope() {
    // One
    let one: Config = serde_yaml::from_str("agent: claude-code\nskills: []\n").unwrap();
    assert_eq!(one.agents(), vec![Agent::ClaudeCode]);
    assert!(matches!(
        one.agent,
        Some(AgentField::One(Agent::ClaudeCode))
    ));
    // Many (clone preserves order)
    let many: Config = serde_yaml::from_str("agent:\n  - codex\n  - cursor\nskills: []\n").unwrap();
    assert_eq!(many.agents(), vec![Agent::Codex, Agent::Cursor]);
    assert!(matches!(many.agent, Some(AgentField::Many(_))));
    // None → []
    let none: Config = serde_yaml::from_str("skills: []\n").unwrap();
    assert!(none.agents().is_empty());
    assert!(none.agent.is_none());
    // resolved_scope default
    assert_eq!(none.resolved_scope(), Scope::Global);
}

// ───────────────────────────── M-04 · SourceSpec schema (ref/sub-dir renames, alias) ─────────────────────────────
// Oracle: kasetto src/model/config.rs::tests (skills_parses_wildcard_list_and_objects +
// git_pin tests) — `ref` rename, `sub-dir`/`sub_dir` alias, SkillsField wildcard/list.
#[test]
fn parity_source_spec_schema() {
    let cfg: Config = serde_yaml::from_str(
        "skills:\n  - source: https://github.com/me/a\n    skills: \"*\"\n  - source: https://github.com/me/b\n    sub-dir: pack\n    skills:\n      - one\n      - name: two\n        path: nested\n",
    )
    .unwrap();
    assert_eq!(cfg.skills.len(), 2);
    assert!(matches!(cfg.skills[0].skills, SkillsField::Wildcard(_)));
    assert_eq!(cfg.skills[1].sub_dir.as_deref(), Some("pack"));
    let SkillsField::List(ref items) = cfg.skills[1].skills else {
        panic!("list");
    };
    assert!(matches!(&items[0], SkillTarget::Name(n) if n == "one"));
    assert!(
        matches!(&items[1], SkillTarget::Obj { name, path: Some(p) } if name == "two" && p == "nested")
    );
    // `ref` rename + `sub_dir` underscore alias both honored.
    let cfg2: Config = serde_yaml::from_str(
        "skills:\n  - source: https://x/a\n    ref: v9\n    sub_dir: deep/pack\n    skills: \"*\"\n",
    )
    .unwrap();
    assert_eq!(cfg2.skills[0].git_ref.as_deref(), Some("v9"));
    assert_eq!(cfg2.skills[0].sub_dir.as_deref(), Some("deep/pack"));
    assert_eq!(cfg2.skills[0].branch, None);
}

// ───────────────────────────── M-06 · SourceSpec::expected_revision ─────────────────────────────
// Oracle: kasetto src/model/config.rs::tests::{git_pin_precedence_ref_beats_branch,
// git_pin_branch_then_default, local_source_revision_is_local}.
#[test]
fn parity_expected_revision() {
    let mk = |yaml: &str| -> Config { serde_yaml::from_str(yaml).unwrap() };
    // ref wins → ref:<r>
    let c =
        mk("skills:\n  - source: https://x/a\n    branch: dev\n    ref: v9\n    skills: \"*\"\n");
    assert_eq!(c.skills[0].expected_revision(), "ref:v9");
    // branch → branch:<b>
    let c = mk("skills:\n  - source: https://x/a\n    branch: dev\n    skills: \"*\"\n");
    assert_eq!(c.skills[0].expected_revision(), "branch:dev");
    // default remote → branch:main
    let c = mk("skills:\n  - source: https://x/a\n    skills: \"*\"\n");
    assert_eq!(c.skills[0].expected_revision(), "branch:main");
    // local (no "://") → local
    let c = mk("skills:\n  - source: ./local/pack\n    skills: \"*\"\n");
    assert_eq!(c.skills[0].expected_revision(), "local");
}

// ───────────────────────────── M-07 · McpSourceSpec/CommandSourceSpec + as_source_spec ─────────────────────────────
// Oracle: kasetto src/model/config.rs (McpSourceSpec/CommandSourceSpec::as_source_spec).
// MCP source forces sub_dir=None; command source carries sub_dir; both project to a
// wildcard-skills SourceSpec.
#[test]
fn parity_source_spec_projections() {
    // McpSourceSpec: no sub_dir field, mcps: McpsField; as_source_spec forces sub_dir=None.
    let cfg: Config = serde_yaml::from_str(
        "skills: []\nmcps:\n  - source: https://x/m\n    ref: v2\n    branch: dev\n    mcps: \"*\"\n",
    )
    .unwrap();
    let m: &McpSourceSpec = &cfg.mcps[0];
    assert!(matches!(m.mcps, McpsField::Wildcard(_)));
    let proj = m.as_source_spec();
    assert_eq!(proj.source, "https://x/m");
    assert_eq!(proj.git_ref.as_deref(), Some("v2"));
    assert_eq!(proj.branch.as_deref(), Some("dev"));
    assert_eq!(proj.sub_dir, None, "MCP projection forces sub_dir=None");
    assert!(matches!(proj.skills, SkillsField::Wildcard(ref s) if s == "*"));

    // CommandSourceSpec: carries sub_dir, commands: CommandsField; projection keeps sub_dir.
    let cfg: Config = serde_yaml::from_str(
        "skills: []\ncommands:\n  - source: https://x/c\n    sub-dir: cmds\n    commands: \"*\"\n",
    )
    .unwrap();
    let c: &CommandSourceSpec = &cfg.commands[0];
    let proj = c.as_source_spec();
    assert_eq!(proj.source, "https://x/c");
    assert_eq!(proj.sub_dir.as_deref(), Some("cmds"));
    assert!(matches!(proj.skills, SkillsField::Wildcard(ref s) if s == "*"));
}

// ───────────────────────────── M-09 · Agent 21-preset enum + AGENT_PRESETS (serde renames) ─────────────────────────────
// Oracle: kasetto src/model/agent.rs (Agent enum serde renames) +
// src/model/config.rs::tests (agent_presets_count_is_twenty_one). All 21 kebab/lower renames
// must round-trip through the public AgentField deserializer.
#[test]
fn parity_agent_enum_serde_all_21() {
    assert_eq!(AGENT_PRESETS.len(), 21);
    // Each serde rename string → the matching Agent variant (kasetto's #[serde(rename=...)]).
    let cases: &[(&str, Agent)] = &[
        ("amp", Agent::Amp),
        ("antigravity", Agent::Antigravity),
        ("augment", Agent::Augment),
        ("claude-code", Agent::ClaudeCode),
        ("cline", Agent::Cline),
        ("codex", Agent::Codex),
        ("continue", Agent::Continue),
        ("cursor", Agent::Cursor),
        ("gemini-cli", Agent::GeminiCli),
        ("github-copilot", Agent::GithubCopilot),
        ("goose", Agent::Goose),
        ("junie", Agent::Junie),
        ("kiro-cli", Agent::KiroCli),
        ("openclaw", Agent::OpenClaw),
        ("opencode", Agent::OpenCode),
        ("openhands", Agent::OpenHands),
        ("replit", Agent::Replit),
        ("roo", Agent::Roo),
        ("trae", Agent::Trae),
        ("warp", Agent::Warp),
        ("windsurf", Agent::Windsurf),
    ];
    assert_eq!(cases.len(), 21);
    for (rename, expected) in cases {
        let cfg: Config = serde_yaml::from_str(&format!("agent: {rename}\nskills: []\n")).unwrap();
        assert_eq!(cfg.agents(), vec![*expected], "serde rename `{rename}`");
        // and it is present in AGENT_PRESETS
        assert!(AGENT_PRESETS.contains(expected), "preset {expected:?}");
    }
}

// ───────────────────────────── M-10 · AgentField untagged One|Many ─────────────────────────────
// Oracle: kasetto src/model/config.rs::tests::agent_field_parses_one_and_many.
#[test]
fn parity_agent_field_one_and_many() {
    let one: Config = serde_yaml::from_str("agent: claude-code\nskills: []\n").unwrap();
    assert_eq!(one.agents(), vec![Agent::ClaudeCode]);
    let many: Config = serde_yaml::from_str("agent:\n  - codex\n  - cursor\nskills: []\n").unwrap();
    assert_eq!(many.agents(), vec![Agent::Codex, Agent::Cursor]);
    let none: Config = serde_yaml::from_str("skills: []\n").unwrap();
    assert!(none.agents().is_empty());
}

// ───────────────────────────── M-11 · Agent::global_path (all 21 presets) ─────────────────────────────
// Oracle: kasetto src/model/agent.rs Agent::global_path — the per-preset global SKILLS dir.
// Every preset's exact path string ported verbatim (the no-downgrade path table).
#[test]
fn parity_global_path_all_21() {
    let h = Path::new("/h");
    let cases: &[(Agent, &str)] = &[
        (Agent::Amp, ".config/agents/skills"),
        (Agent::Antigravity, ".gemini/antigravity/skills"),
        (Agent::Augment, ".augment/skills"),
        (Agent::ClaudeCode, ".claude/skills"),
        (Agent::Cline, ".agents/skills"),
        (Agent::Codex, ".codex/skills"),
        (Agent::Continue, ".continue/skills"),
        (Agent::Cursor, ".cursor/skills"),
        (Agent::GeminiCli, ".gemini/skills"),
        (Agent::GithubCopilot, ".copilot/skills"),
        (Agent::Goose, ".config/goose/skills"),
        (Agent::Junie, ".junie/skills"),
        (Agent::KiroCli, ".kiro/skills"),
        (Agent::OpenClaw, ".openclaw/skills"),
        (Agent::OpenCode, ".config/opencode/skills"),
        (Agent::OpenHands, ".openhands/skills"),
        (Agent::Replit, ".config/agents/skills"),
        (Agent::Roo, ".roo/skills"),
        (Agent::Trae, ".trae/skills"),
        (Agent::Warp, ".agents/skills"),
        (Agent::Windsurf, ".codeium/windsurf/skills"),
    ];
    assert_eq!(cases.len(), AGENT_PRESETS.len());
    for (a, rel) in cases {
        assert_eq!(a.global_path(h), h.join(rel), "global_path {a:?}");
    }
}

// ───────────────────────────── M-12 · Agent::project_path (all 21 presets) ─────────────────────────────
// Oracle: kasetto src/model/agent.rs Agent::project_path — diverges from global for
// amp|replit (.agents/skills), goose (.goose/skills), opencode (.opencode/skills),
// windsurf (.windsurf/skills).
#[test]
fn parity_project_path_all_21() {
    let p = Path::new("/p");
    let cases: &[(Agent, &str)] = &[
        (Agent::Amp, ".agents/skills"),
        (Agent::Antigravity, ".gemini/antigravity/skills"),
        (Agent::Augment, ".augment/skills"),
        (Agent::ClaudeCode, ".claude/skills"),
        (Agent::Cline, ".agents/skills"),
        (Agent::Codex, ".codex/skills"),
        (Agent::Continue, ".continue/skills"),
        (Agent::Cursor, ".cursor/skills"),
        (Agent::GeminiCli, ".gemini/skills"),
        (Agent::GithubCopilot, ".copilot/skills"),
        (Agent::Goose, ".goose/skills"),
        (Agent::Junie, ".junie/skills"),
        (Agent::KiroCli, ".kiro/skills"),
        (Agent::OpenClaw, ".openclaw/skills"),
        (Agent::OpenCode, ".opencode/skills"),
        (Agent::OpenHands, ".openhands/skills"),
        (Agent::Replit, ".agents/skills"),
        (Agent::Roo, ".roo/skills"),
        (Agent::Trae, ".trae/skills"),
        (Agent::Warp, ".agents/skills"),
        (Agent::Windsurf, ".windsurf/skills"),
    ];
    assert_eq!(cases.len(), AGENT_PRESETS.len());
    for (a, rel) in cases {
        assert_eq!(a.project_path(p), p.join(rel), "project_path {a:?}");
    }
}

// ───────────────────────────── M-13 · Agent::mcp_settings_target (global, all 21) ─────────────────────────────
// Oracle: kasetto src/model/agent.rs Agent::mcp_settings_target — per-preset native global
// MCP config path + McpSettingsFormat. github-copilot OS-branches (Linux on CI).
#[test]
fn parity_mcp_settings_target_all_21() {
    let h = Path::new("/h");
    let kcfg = Path::new("/h/kasetto.yaml"); // reserved/unused arg, threaded verbatim.
    let cases: &[(Agent, &str, McpSettingsFormat)] = &[
        (
            Agent::ClaudeCode,
            ".claude.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Cursor,
            ".cursor/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::GeminiCli,
            ".gemini/settings.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Roo,
            ".roo/mcp_settings.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Windsurf,
            ".codeium/windsurf/mcp_config.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Cline,
            ".cline/data/settings/cline_mcp_settings.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Continue,
            ".continue/mcpServers/kasetto.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Amp,
            ".config/agents/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Replit,
            ".config/agents/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Antigravity,
            ".gemini/antigravity/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Augment,
            ".augment/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (Agent::Warp, ".warp/mcp.json", McpSettingsFormat::McpServers),
        (
            Agent::Codex,
            ".codex/config.toml",
            McpSettingsFormat::CodexToml,
        ),
        (
            Agent::Goose,
            ".config/goose/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Junie,
            ".junie/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::KiroCli,
            ".kiro/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::OpenClaw,
            ".openclaw/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::OpenCode,
            ".config/opencode/opencode.json",
            McpSettingsFormat::OpenCode,
        ),
        (
            Agent::OpenHands,
            ".openhands/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (Agent::Trae, ".trae/mcp.json", McpSettingsFormat::McpServers),
    ];
    for (a, rel, fmt) in cases {
        let t = a.mcp_settings_target(h, kcfg);
        assert_eq!(t.path, h.join(rel), "mcp_settings_target path {a:?}");
        assert_eq!(t.format, *fmt, "mcp_settings_target format {a:?}");
    }
    // github-copilot: VsCodeServers format, OS-branched user mcp.json (Linux on CI host).
    let copilot = Agent::GithubCopilot.mcp_settings_target(h, kcfg);
    assert_eq!(copilot.format, McpSettingsFormat::VsCodeServers);
    assert_eq!(copilot.path, h.join(".config/Code/User/mcp.json"));
}

// ───────────────────────────── M-14 · Agent::mcp_project_target (all 21) ─────────────────────────────
// Oracle: kasetto src/model/agent.rs Agent::mcp_project_target — per-preset PROJECT MCP path;
// 7 fallthrough presets collapse to ".mcp.json" McpServers.
#[test]
fn parity_mcp_project_target_all_21() {
    let p = Path::new("/p");
    let cases: &[(Agent, &str, McpSettingsFormat)] = &[
        (
            Agent::ClaudeCode,
            ".mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Cursor,
            ".cursor/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::GithubCopilot,
            ".vscode/mcp.json",
            McpSettingsFormat::VsCodeServers,
        ),
        (
            Agent::GeminiCli,
            ".gemini/settings.json",
            McpSettingsFormat::McpServers,
        ),
        (Agent::Roo, ".roo/mcp.json", McpSettingsFormat::McpServers),
        (
            Agent::Windsurf,
            ".windsurf/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Cline,
            ".cline_mcp_servers.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Continue,
            ".continue/mcpServers/kasetto.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::Codex,
            ".codex/config.toml",
            McpSettingsFormat::CodexToml,
        ),
        (Agent::Amp, ".amp/mcp.json", McpSettingsFormat::McpServers),
        (Agent::Trae, ".trae/mcp.json", McpSettingsFormat::McpServers),
        (
            Agent::Junie,
            ".junie/mcp/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::KiroCli,
            ".kiro/settings/mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (
            Agent::OpenCode,
            ".opencode/opencode.json",
            McpSettingsFormat::OpenCode,
        ),
        // 7 fallthrough presets → ".mcp.json" McpServers.
        (
            Agent::Antigravity,
            ".mcp.json",
            McpSettingsFormat::McpServers,
        ),
        (Agent::Augment, ".mcp.json", McpSettingsFormat::McpServers),
        (Agent::Goose, ".mcp.json", McpSettingsFormat::McpServers),
        (Agent::OpenClaw, ".mcp.json", McpSettingsFormat::McpServers),
        (Agent::OpenHands, ".mcp.json", McpSettingsFormat::McpServers),
        (Agent::Replit, ".mcp.json", McpSettingsFormat::McpServers),
        (Agent::Warp, ".mcp.json", McpSettingsFormat::McpServers),
    ];
    assert_eq!(cases.len(), AGENT_PRESETS.len());
    for (a, rel, fmt) in cases {
        let t = a.mcp_project_target(p);
        assert_eq!(t.path, p.join(rel), "mcp_project_target path {a:?}");
        assert_eq!(t.format, *fmt, "mcp_project_target format {a:?}");
    }
}

// ───────────────────────────── M-17 · all_mcp_{settings,project}_targets (dedup + sort) ─────────────────────────────
// Oracle: kasetto src/model/agent.rs::tests (all_mcp_*_targets dedupe/sort, the `clean`
// manifest wipe). amp|replit collapse to one settings path; 7 presets collapse to one
// project ".mcp.json".
#[test]
fn parity_all_mcp_targets_dedup_sort() {
    let h = Path::new("/h");
    let kcfg = Path::new("/h/kasetto.yaml");
    let settings = all_mcp_settings_targets(h, kcfg);
    assert!(!settings.is_empty());
    // strictly sorted + deduped by path
    for w in settings.windows(2) {
        assert!(w[0].path < w[1].path, "settings strictly sorted+deduped");
    }
    // amp & replit share ".config/agents/mcp.json" → exactly one entry.
    let amp = h.join(".config/agents/mcp.json");
    assert_eq!(settings.iter().filter(|t| t.path == amp).count(), 1);

    let p = Path::new("/p");
    let project = all_mcp_project_targets(p);
    assert!(!project.is_empty());
    for w in project.windows(2) {
        assert!(w[0].path < w[1].path);
    }
    // 7 fallthrough presets collapse to a single ".mcp.json".
    let mcp_json = p.join(".mcp.json");
    assert_eq!(project.iter().filter(|t| t.path == mcp_json).count(), 1);
}

// ───────────────────────────── M-19 · command_{global,project}_targets (scoped, doctor) ─────────────────────────────
// Oracle: kasetto src/model/agent.rs (command_*_targets over a SPECIFIC agent set) — dedup
// over the given agents only; empty set → empty.
#[test]
fn parity_scoped_command_targets() {
    let h = Path::new("/h");
    let p = Path::new("/p");
    // Cursor has NO global command surface but DOES have a project one → global yields only
    // the ClaudeCode dir; project yields both.
    let g = command_global_targets(h, &[Agent::Cursor, Agent::ClaudeCode]);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].path, h.join(".claude/commands"));
    let pr = command_project_targets(p, &[Agent::Cursor, Agent::ClaudeCode]);
    assert_eq!(pr.len(), 2);
    // empty agent set → empty scoped targets.
    assert!(command_global_targets(h, &[]).is_empty());
    assert!(command_project_targets(p, &[]).is_empty());
}

// ───────────────────────────── M-20 · vscode_user_mcp_json OS-branch + dedup helpers ─────────────────────────────
// Oracle: kasetto src/model/agent.rs (vscode_user_mcp_json OS-branch via github-copilot;
// dedup_targets/dedup_command_targets via all_* sorting). The private helpers are observed
// through the public seams (github-copilot target + all_command_global_targets ordering).
#[test]
fn parity_private_helpers_via_public_seams() {
    let h = Path::new("/h");
    let kcfg = Path::new("/h/kasetto.yaml");
    // vscode_user_mcp_json (Linux CI branch) surfaces via the github-copilot global target.
    let copilot = Agent::GithubCopilot.mcp_settings_target(h, kcfg);
    assert_eq!(copilot.path, h.join(".config/Code/User/mcp.json"));
    // mcp_servers_target ctor → McpServers format (e.g. cursor).
    assert_eq!(
        Agent::Cursor.mcp_settings_target(h, kcfg).format,
        McpSettingsFormat::McpServers
    );
    // dedup_command_targets: all_command_global_targets is strictly sorted + deduped.
    let all = all_command_global_targets(h);
    assert!(!all.is_empty());
    for w in all.windows(2) {
        assert!(w[0].path < w[1].path, "command targets strictly deduped");
    }
}

// ───────────────────────────── M-21 · resolve_scope (CLI > cfg > Global; non-fallback arms) ─────────────────────────────
// Oracle: kasetto src/model/config.rs::tests::resolve_scope_prefers_cli_override (+ the
// cfg-then-default arm). The file-read fallback (M-22) is DEFERRED — not asserted here.
#[test]
fn parity_resolve_scope_precedence() {
    use envctl_agent_env::config::resolve_scope;
    // CLI override wins regardless of cfg.
    assert_eq!(resolve_scope(Some(Scope::Project), None), Scope::Project);
    assert_eq!(resolve_scope(Some(Scope::Global), None), Scope::Global);
    let cfg_proj: Config = serde_yaml::from_str("scope: project\nskills: []\n").unwrap();
    assert_eq!(
        resolve_scope(Some(Scope::Global), Some(&cfg_proj)),
        Scope::Global,
        "CLI override beats cfg"
    );
    // No CLI override → cfg scope.
    assert_eq!(resolve_scope(None, Some(&cfg_proj)), Scope::Project);
    // No CLI, no cfg → Global default (the non-file-read arm).
    assert_eq!(resolve_scope(None, None), Scope::Global);
}

// ───────────────────────────── M-23/M-24 · AgentLockEntry fields + LOCK_VERSION ─────────────────────────────
// Oracle: kasetto src/model/types.rs (SkillEntry fields, LOCK_VERSION=2). envctl folds
// SkillEntry → AgentLockEntry; State{version,skills} is engine-folded (see BLOCKED note).
#[test]
fn parity_lock_entry_fields_and_version() {
    assert_eq!(LOCK_VERSION, 2);
    // SkillEntry's 7 fields, including the Option<Scope> skip-if-none + default description.
    let e = AgentLockEntry {
        destination: ".claude/skills/a".into(),
        hash: "abc".into(),
        skill: "a".into(),
        description: "desc".into(),
        source: "src".into(),
        source_revision: "branch:main".into(),
        scope: Some(Scope::Project),
    };
    // serde round-trip preserves every field; scope present serializes.
    let y = serde_yaml::to_string(&e).unwrap();
    let back: AgentLockEntry = serde_yaml::from_str(&y).unwrap();
    assert_eq!(back, e);
    // scope = None is skipped on serialize (skip_serializing_if = Option::is_none).
    let no_scope = AgentLockEntry {
        scope: None,
        ..e.clone()
    };
    let y2 = serde_yaml::to_string(&no_scope).unwrap();
    assert!(!y2.contains("scope"), "None scope must be skipped: {y2}");
    // description defaults to "" when absent (legacy-tolerant).
    let legacy: AgentLockEntry = serde_yaml::from_str(
        "destination: d\nhash: h\nskill: s\nsource: src\nsource_revision: local\n",
    )
    .unwrap();
    assert_eq!(legacy.description, "");
    assert_eq!(legacy.scope, None);
}

// ───────────────────────────── M-25 · Summary/Action/Report/InstalledSkill/SyncFailure ─────────────────────────────
// Oracle: kasetto src/model/types.rs (the sync-result value types + their serde field names).
#[test]
fn parity_report_value_types() {
    // Summary default = all zero.
    let s = Summary::default();
    assert_eq!(
        (
            s.installed,
            s.updated,
            s.removed,
            s.unchanged,
            s.broken,
            s.failed
        ),
        (0, 0, 0, 0, 0, 0)
    );
    // Report serializes with kasetto's exact field names; Action.error stays null (not skipped).
    let report = Report {
        run_id: "run-1".into(),
        config: "kasetto.yaml".into(),
        destination: "/dest".into(),
        dry_run: true,
        summary: Summary {
            installed: 2,
            ..Default::default()
        },
        actions: vec![Action {
            source: Some("github.com/a/b".into()),
            skill: Some("review".into()),
            status: "installed".into(),
            error: None,
        }],
    };
    let v: serde_json::Value = serde_json::to_value(&report).unwrap();
    assert_eq!(v["run_id"], "run-1");
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["summary"]["installed"], 2);
    assert_eq!(v["actions"][0]["status"], "installed");
    assert!(v["actions"][0]["error"].is_null());
    // InstalledSkill carries scope + updated_ago (list view).
    let is = InstalledSkill {
        id: "src::a".into(),
        scope: Scope::Global,
        name: "a".into(),
        description: "d".into(),
        source: "src".into(),
        skill: "a".into(),
        destination: ".claude/skills/a".into(),
        hash: "h".into(),
        source_revision: "local".into(),
        updated_at: "111".into(),
        updated_ago: "2h ago".into(),
    };
    let iv: serde_json::Value = serde_json::to_value(&is).unwrap();
    assert_eq!(iv["scope"], "global");
    assert_eq!(iv["updated_ago"], "2h ago");
    // SyncFailure 3-field record.
    let f = SyncFailure {
        name: "n".into(),
        source: "s".into(),
        reason: "boom".into(),
    };
    let fv: serde_json::Value = serde_json::to_value(&f).unwrap();
    assert_eq!(fv["name"], "n");
    assert_eq!(fv["reason"], "boom");
}

// ───────────────────────────── M-26 · McpSettingsFormat + McpSettingsTarget (4 formats) ─────────────────────────────
// Oracle: kasetto src/model/mod.rs (McpSettingsFormat 4 variants + McpSettingsTarget{path,format}).
// Each variant is reached via a representative preset.
#[test]
fn parity_mcp_settings_format_variants() {
    let h = Path::new("/h");
    let kcfg = Path::new("/h/kasetto.yaml");
    // McpServers (claude-code), CodexToml (codex), OpenCode (opencode), VsCodeServers (copilot).
    assert_eq!(
        Agent::ClaudeCode.mcp_settings_target(h, kcfg).format,
        McpSettingsFormat::McpServers
    );
    assert_eq!(
        Agent::Codex.mcp_settings_target(h, kcfg).format,
        McpSettingsFormat::CodexToml
    );
    assert_eq!(
        Agent::OpenCode.mcp_settings_target(h, kcfg).format,
        McpSettingsFormat::OpenCode
    );
    assert_eq!(
        Agent::GithubCopilot.mcp_settings_target(h, kcfg).format,
        McpSettingsFormat::VsCodeServers
    );
    // McpSettingsTarget{path, format} constructed directly round-trips its fields.
    let t = McpSettingsTarget {
        path: PathBuf::from("/x/mcp.json"),
        format: McpSettingsFormat::McpServers,
    };
    assert_eq!(t.path, PathBuf::from("/x/mcp.json"));
    assert_eq!(t.format, McpSettingsFormat::McpServers);
}

// ───────────────────────────── CFG-01 · load_config_any (top-level loader) ─────────────────────────────
// Oracle: kasetto src/fsops/config.rs::tests::load_config_any_resolves_extends_relative_to_parent.
// extends resolved relative to the PARENT config's dir; scalars override, lists append.
#[test]
fn parity_load_config_any_extends_relative() {
    let root = tmp("cfg-extends-rel");
    let base = root.join("base.yaml");
    let child = root.join("child.yaml");
    fs::write(
        &base,
        "agent: cursor\nscope: global\nskills:\n  - source: https://x/a\n    ref: v1\n    skills: \"*\"\n",
    )
    .unwrap();
    fs::write(
        &child,
        "extends: ./base.yaml\nscope: project\nskills:\n  - source: https://x/b\n    skills: \"*\"\n",
    )
    .unwrap();

    let (cfg, _, _) = load_config_any(child.to_str().unwrap()).unwrap();
    assert_eq!(cfg.scope, Some(Scope::Project)); // child scalar overrides base
    assert_eq!(cfg.skills.len(), 2); // base + child sources appended
    assert!(cfg
        .skills
        .iter()
        .any(|s| s.source == "https://x/a" && s.git_ref.as_deref() == Some("v1")));
    assert!(cfg.skills.iter().any(|s| s.source == "https://x/b"));
    let _ = fs::remove_dir_all(&root);
}

// ───────────────────────────── CFG-02 · load_config_recursive (depth/cycle, chain) ─────────────────────────────
// Oracle: kasetto src/fsops/config.rs::tests::{load_config_any_chains_extends,
// load_config_any_detects_cycles, load_config_any_overrides_same_identity_in_extends}.
#[test]
fn parity_load_config_recursive_chain_and_cycle() {
    // 3-deep extends chain a←b←c; deepest child wins scope.
    let root = tmp("cfg-extends-chain");
    let a = root.join("a.yaml");
    let b = root.join("b.yaml");
    let c = root.join("c.yaml");
    fs::write(&a, "agent: cursor\nscope: global\nskills: []\n").unwrap();
    fs::write(&b, "extends: ./a.yaml\nskills: []\n").unwrap();
    fs::write(&c, "extends: ./b.yaml\nscope: project\nskills: []\n").unwrap();
    let (cfg, _, _) = load_config_any(c.to_str().unwrap()).unwrap();
    assert_eq!(cfg.scope, Some(Scope::Project));
    assert_eq!(cfg.agents().len(), 1); // agent: cursor inherited from a
    let _ = fs::remove_dir_all(&root);

    // circular extends a↔b → fail-closed with a "circular" error.
    let root = tmp("cfg-extends-cycle");
    let a = root.join("a.yaml");
    let b = root.join("b.yaml");
    fs::write(&a, "extends: ./b.yaml\nskills: []\n").unwrap();
    fs::write(&b, "extends: ./a.yaml\nskills: []\n").unwrap();
    let res = load_config_any(a.to_str().unwrap());
    assert!(res.is_err(), "cycle must error");
    assert!(
        format!("{}", res.err().unwrap()).contains("circular"),
        "cycle error must mention 'circular'"
    );
    let _ = fs::remove_dir_all(&root);

    // override same identity (source+ref) in extends → replaced, not appended.
    let root = tmp("cfg-extends-override");
    let base = root.join("base.yaml");
    let child = root.join("child.yaml");
    fs::write(
        &base,
        "agent: cursor\nskills:\n  - source: https://x/a\n    ref: v1\n    skills: \"*\"\n",
    )
    .unwrap();
    fs::write(
        &child,
        "extends: ./base.yaml\nskills:\n  - source: https://x/a\n    ref: v1\n    skills:\n      - one\n",
    )
    .unwrap();
    let (cfg, _, _) = load_config_any(child.to_str().unwrap()).unwrap();
    assert_eq!(cfg.skills.len(), 1);
    assert!(matches!(&cfg.skills[0].skills, SkillsField::List(items) if items.len() == 1));
    let _ = fs::remove_dir_all(&root);

    // load_config_recursive public seam: depth guard fires past MAX_EXTENDS_DEPTH.
    // (Reachable directly; a non-existent ref past the limit errors on the depth check.)
    let mut visited = HashSet::new();
    let deep = load_config_recursive("/nonexistent/over.yaml", None, &mut visited, 9);
    assert!(deep.is_err(), "depth > MAX_EXTENDS_DEPTH must error");
    assert!(format!("{}", deep.err().unwrap()).contains("depth limit"));
}

// ───────────────────────────── CFG-03 · fetch_config_text (local arm via load_config_any) ─────────────────────────────
// Oracle: kasetto src/fsops/config.rs (fetch_config_text local arm — canonicalize, "config
// not found" on miss). `fetch_config_text` is private in BOTH kasetto and envctl; its local
// arm is observed through the public load_config_any (the only public seam). The http(s) arm
// is network/private — see BLOCKED note for the remote sub-path.
#[test]
fn parity_fetch_config_text_local_arm() {
    // canonicalize + read a real local file (parent becomes base_dir).
    let root = tmp("cfg-fetch-local");
    let f = root.join("kasetto.yaml");
    fs::write(&f, "agent: codex\nscope: project\nskills: []\n").unwrap();
    let (cfg, base_dir, label) = load_config_any(f.to_str().unwrap()).unwrap();
    assert_eq!(cfg.agents(), vec![Agent::Codex]);
    assert!(label.ends_with("kasetto.yaml"));
    // base_dir is the canonicalized parent dir of the config file.
    assert_eq!(base_dir, fs::canonicalize(&root).unwrap());
    let _ = fs::remove_dir_all(&root);

    // missing local config → "config not found" error (fail-closed).
    let missing = load_config_any("/no/such/dir/kasetto.yaml");
    assert!(missing.is_err());
    assert!(
        format!("{}", missing.err().unwrap()).contains("config not found"),
        "missing config must report 'config not found'"
    );
}

// ───────────────────────────── L-01 · AssetEntry (CSV destination, skip-empty revision) ─────────────────────────────
// Oracle: kasetto src/lock.rs (AssetEntry fields; source_revision default + skip_if_empty).
#[test]
fn parity_asset_entry_fields() {
    let a = AssetEntry {
        kind: "mcp".into(),
        name: "pack.json".into(),
        hash: "h1".into(),
        source: "src".into(),
        destination: "srv1,srv2".into(), // CSV: MCP server names
        source_revision: "branch:main".into(),
    };
    let back: AssetEntry = serde_yaml::from_str(&serde_yaml::to_string(&a).unwrap()).unwrap();
    assert_eq!(back, a);
    // empty source_revision is skipped on serialize (skip_serializing_if = String::is_empty).
    let empty_rev = AssetEntry {
        source_revision: String::new(),
        ..a.clone()
    };
    let y = serde_yaml::to_string(&empty_rev).unwrap();
    assert!(
        !y.contains("source_revision"),
        "empty revision must be skipped: {y}"
    );
    // default-tolerant: a lock written before the revision field omits it.
    let legacy: AssetEntry = serde_yaml::from_str(
        "kind: command\nname: deploy\nhash: h\nsource: s\ndestination: .claude/commands/deploy.md\n",
    )
    .unwrap();
    assert_eq!(legacy.source_revision, "");
}

// ───────────────────────────── L-02 · AgentLockFile + Default + default_version ─────────────────────────────
// Oracle: kasetto src/lock.rs::tests::round_trip_empty_lock_file + LockFile::Default.
#[test]
fn parity_lock_file_default_and_version() {
    let lf = AgentLockFile::default();
    assert_eq!(lf.version, 2); // default_version = LOCK_VERSION = 2
    assert!(lf.skills.is_empty());
    assert!(lf.assets.is_empty());
    // version field defaults to 2 when absent (unknown fields ignored / legacy-tolerant).
    let parsed: AgentLockFile = serde_yaml::from_str("skills: {}\nassets: {}\n").unwrap();
    assert_eq!(parsed.version, 2);
    // unknown top-level fields are tolerated (legacy v1 carried extra keys).
    let legacy: AgentLockFile =
        serde_yaml::from_str("version: 1\nlast_run: '111'\nskills: {}\nassets: {}\n").unwrap();
    assert_eq!(legacy.version, 1);
}

// ───────────────────────────── L-03 · AgentLockFile state methods (asset CRUD) ─────────────────────────────
// Oracle: kasetto src/lock.rs::tests::{list_tracked_asset_ids_filters_by_kind,
// remove_tracked_asset_deletes_entry, list_installed_mcps_deduplicates, clear_all_empties_everything}.
// NOTE: kasetto's list_installed_{commands,mcps} + state/apply_state are NOT on envctl's
// public lock API (engine-folded) — see BLOCKED note. The asset-CRUD subset is verified here.
#[test]
fn parity_lock_state_asset_crud() {
    let mk = |kind: &str, name: &str, dest: &str| AssetEntry {
        kind: kind.into(),
        name: name.into(),
        hash: "h".into(),
        source: "s".into(),
        destination: dest.into(),
        source_revision: "rev".into(),
    };
    let mut lock = AgentLockFile::default();
    lock.save_tracked_asset("mcp::a", mk("mcp", "a", "srv1,srv2"));
    lock.save_tracked_asset("other::b", mk("other", "b", "d2"));

    // get_tracked_asset returns (hash, destination) only when the kind matches.
    assert_eq!(
        lock.get_tracked_asset("mcp", "mcp::a"),
        Some(("h".into(), "srv1,srv2".into()))
    );
    assert_eq!(
        lock.get_tracked_asset("command", "mcp::a"),
        None,
        "kind filter"
    );

    // list_tracked_asset_ids filters by kind.
    let mcps = lock.list_tracked_asset_ids("mcp");
    assert_eq!(mcps, vec![("mcp::a", "srv1,srv2")]);

    // remove deletes the entry.
    lock.remove_tracked_asset("mcp::a");
    assert!(lock.get_tracked_asset("mcp", "mcp::a").is_none());

    // clear_all empties skills + assets.
    lock.skills.insert("k".into(), AgentLockEntry::default());
    lock.clear_all();
    assert!(lock.skills.is_empty() && lock.assets.is_empty());
}

// ───────────────────────────── L-04 · lock_path (scope-keyed) ─────────────────────────────
// Oracle: kasetto src/lock.rs::lock_path — Project→project_root/<lock>, Global→data/<lock>.
// envctl threads the global data dir explicitly (kasetto reads dirs_kasetto_data()).
#[test]
fn parity_lock_path_by_scope() {
    use envctl_agent_env::lock::lock_path;
    let proj = Path::new("/proj");
    let data = Path::new("/data");
    // Project → <project_root>/<LOCK_FILENAME>
    let p = lock_path(Scope::Project, proj, data);
    assert_eq!(p, proj.join("agent-env.lock"));
    // Global → <global_data_dir>/<LOCK_FILENAME>
    let g = lock_path(Scope::Global, proj, data);
    assert_eq!(g, data.join("agent-env.lock"));
}

// ───────────────────────────── L-05 · load_lock / save_lock (round-trip, legacy restamp) ─────────────────────────────
// Oracle: kasetto src/lock.rs::tests::{round_trip_with_skills_and_assets,
// load_returns_default_when_missing, legacy_v1_lock_loads_and_restamps_on_save}.
#[test]
fn parity_load_save_lock() {
    use envctl_agent_env::lock::{load, save};
    let dir = tmp("lock-roundtrip");
    let path = dir.join("agent-env.lock");

    // round-trip skills + assets; scope-relative destination preserved verbatim.
    let mut lock = AgentLockFile::default();
    lock.skills.insert(
        "src::skill-a".into(),
        AgentLockEntry {
            destination: ".claude/skills/skill-a".into(),
            hash: "abc".into(),
            skill: "skill-a".into(),
            description: "desc".into(),
            source: "src".into(),
            source_revision: "rev1".into(),
            scope: Some(Scope::Project),
        },
    );
    lock.save_tracked_asset(
        "mcp::src::pack.json",
        AssetEntry {
            kind: "mcp".into(),
            name: "pack.json".into(),
            hash: "h1".into(),
            source: "src".into(),
            destination: "srv1,srv2".into(),
            source_revision: "rev1".into(),
        },
    );
    save(&mut lock, &path).unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.version, 2);
    assert_eq!(loaded.skills.len(), 1);
    assert_eq!(loaded.skills["src::skill-a"].hash, "abc");
    assert_eq!(
        loaded.skills["src::skill-a"].destination,
        ".claude/skills/skill-a"
    );
    assert_eq!(
        loaded.get_tracked_asset("mcp", "mcp::src::pack.json"),
        Some(("h1".into(), "srv1,srv2".into()))
    );

    // missing file → default lock.
    let missing = load(&dir.join("nope.lock")).unwrap();
    assert_eq!(missing.version, 2);
    assert!(missing.skills.is_empty());

    // legacy v1 lock: unknown fields ignored, absolute dest honored, restamped to v2 on save.
    let legacy_path = dir.join("legacy.lock");
    let legacy = "version: 1\n\
last_run: '111'\n\
skills:\n\
\x20 src::a:\n\
\x20\x20\x20 destination: /abs/path/.claude/skills/a\n\
\x20\x20\x20 hash: h\n\
\x20\x20\x20 skill: a\n\
\x20\x20\x20 source: src\n\
\x20\x20\x20 source_revision: local\n\
\x20\x20\x20 updated_at: '111'\n\
assets: {}\n";
    fs::write(&legacy_path, legacy).unwrap();
    let mut loaded = load(&legacy_path).unwrap();
    assert_eq!(loaded.version, 1);
    assert_eq!(
        loaded.skills["src::a"].destination,
        "/abs/path/.claude/skills/a" // legacy absolute dest honored
    );
    save(&mut loaded, &legacy_path).unwrap();
    let resaved = fs::read_to_string(&legacy_path).unwrap();
    assert!(resaved.starts_with("version: 2"), "restamped to v2");
    assert!(!resaved.contains("last_run"));
    assert!(!resaved.contains("updated_at"));

    let _ = fs::remove_dir_all(&dir);
}

// ───────────────────────────── L-06 · lock_check + LockMode diff ─────────────────────────────
// Oracle: kasetto commands/lock.rs diff semantics (added/removed/updated by hash|rev) +
// the 3-mode allows_fetch/should_resolve logic. (envctl folds these into lock.rs.)
#[test]
fn parity_lock_check_and_modes() {
    use envctl_agent_env::lock::DriftStatus;
    let mk = |dest: &str, hash: &str, rev: &str| AgentLockEntry {
        destination: dest.into(),
        hash: hash.into(),
        skill: "skill-a".into(),
        description: "d".into(),
        source: "src".into(),
        source_revision: rev.into(),
        scope: Some(Scope::Project),
    };

    // added / removed / unchanged.
    let mut prev = AgentLockFile::default();
    prev.skills.insert("k::keep".into(), mk("d", "h1", "r1"));
    prev.skills.insert("k::gone".into(), mk("d", "h2", "r1"));
    let mut next = AgentLockFile::default();
    next.skills.insert("k::keep".into(), mk("d", "h1", "r1"));
    next.skills.insert("k::new".into(), mk("d", "h3", "r1"));
    let drift = prev.lock_check(&next);
    assert!(drift
        .iter()
        .any(|d| d.status == DriftStatus::Removed && d.id == "k::gone"));
    assert!(drift
        .iter()
        .any(|d| d.status == DriftStatus::Added && d.id == "k::new"));
    assert!(!drift.iter().any(|d| d.id == "k::keep"), "no spurious keep");

    // updated by hash, updated by revision, and clean-when-identical.
    let mut p = AgentLockFile::default();
    p.skills.insert("k::a".into(), mk("d", "h1", "r1"));
    let mut by_hash = AgentLockFile::default();
    by_hash.skills.insert("k::a".into(), mk("d", "h2", "r1"));
    assert_eq!(p.lock_check(&by_hash)[0].status, DriftStatus::Updated);
    let mut by_rev = AgentLockFile::default();
    by_rev.skills.insert("k::a".into(), mk("d", "h1", "r2"));
    assert_eq!(p.lock_check(&by_rev)[0].status, DriftStatus::Updated);
    let mut same = AgentLockFile::default();
    same.skills.insert("k::a".into(), mk("d", "h1", "r1"));
    assert!(p.lock_check(&same).is_empty());

    // LockMode: Locked is zero-network + never resolves; Plain fetches + resolves all;
    // Update(names) re-resolves only sources providing a named skill.
    assert!(!LockMode::Locked.allows_fetch());
    assert!(!LockMode::Locked.should_resolve("src", &p));
    assert!(LockMode::Plain.allows_fetch());
    assert!(LockMode::Plain.should_resolve("anything", &p));
    let upd = LockMode::Update(vec!["skill-a".into()]);
    assert!(upd.allows_fetch());
    assert!(upd.should_resolve("src", &p)); // src provides skill-a
    assert!(!upd.should_resolve("unrelated", &p));
    assert!(LockMode::Update(vec![]).should_resolve("anything", &p)); // empty = all
}

// ═══════════════════════════════════════════════════════════════════════════
// fsops + config_edit mutation-engine cluster (F-03..F-10, FE-01..FE-06).
// Golden vectors taken VERBATIM from kasetto v3.2.0's certified #[cfg(test)]
// modules in src/fsops/{copy,settings,mod,config_edit}.rs.
// ═══════════════════════════════════════════════════════════════════════════

use envctl_agent_env::{
    copy_dir, dirs_home, insert_item, item_exists, relativize_dest, remove_item, remove_names,
    resolve_command_targets, resolve_dest, resolve_destinations, resolve_mcp_settings_targets,
    resolve_path, scope_root, select_targets, BrokenSkill, Pin, RemoveOutcome, Section, Selector,
    SettingsFile, SourceItem,
};

// ───────────────────────────── F-03 · copy_dir ─────────────────────────────
// Oracle: kasetto src/fsops/copy.rs::tests::{copy_dir_preserves_executable_bit,
// copy_dir_follows_symlinked_directories}. Verbatim recursive copy; fs::copy
// preserves +x; symlinked dirs are canonicalized and recursed.
#[cfg(unix)]
#[test]
fn parity_copy_dir_preserves_executable_bit() {
    use std::os::unix::fs::PermissionsExt;
    let root = tmp("copy-perm");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("create src");
    let script = src.join("run.sh");
    fs::write(&script, "#!/bin/sh\n").expect("write script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");

    let dst = root.join("dst");
    copy_dir(&src, &dst).expect("copy dir");

    let mode = fs::metadata(dst.join("run.sh"))
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "executable bit must survive the copy");
}

// Oracle: kasetto src/fsops/copy.rs::tests::copy_dir_follows_symlinked_directories.
#[cfg(unix)]
#[test]
fn parity_copy_dir_follows_symlinked_directories() {
    use std::os::unix::fs::symlink;
    let root = tmp("copy-symlink");
    let src = root.join("src");
    let refs_dir = src.join("references");
    fs::create_dir_all(&refs_dir).expect("create refs");
    fs::write(refs_dir.join("guide.md"), "hello").expect("write file");
    symlink("references", src.join("linked-references")).expect("create symlink");

    let dst = root.join("dst");
    copy_dir(&src, &dst).expect("copy dir");

    assert!(dst.join("linked-references/guide.md").is_file());
    assert!(dst.join("references/guide.md").is_file());
}

// Oracle: kasetto src/fsops/copy.rs (copy_dir contract: dst removed first).
// Portable arm of the recursive-copy behavior — exercises remove-then-recreate.
#[test]
fn parity_copy_dir_removes_existing_destination_first() {
    let root = tmp("copy-replace");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("keep.txt"), "new").expect("write");

    let dst = root.join("dst");
    fs::create_dir_all(&dst).expect("create dst");
    // A stale file that must NOT survive the copy (dst is wiped first).
    fs::write(dst.join("stale.txt"), "old").expect("write stale");

    copy_dir(&src, &dst).expect("copy dir");
    assert!(dst.join("keep.txt").is_file());
    assert!(
        !dst.join("stale.txt").exists(),
        "destination must be removed before copy"
    );
}

// ───────────────────────────── F-04 · SettingsFile ─────────────────────────────
// Oracle: kasetto src/fsops/mod.rs::tests::{settings_file_load_creates_empty_for_missing_file,
// settings_file_load_parses_existing_json, settings_file_save_creates_parent_dirs,
// settings_file_load_rejects_invalid_json}.
#[test]
fn parity_settings_file_load_save() {
    let root = tmp("settings");

    // load(missing) → empty {}.
    let missing = root.join("nonexistent.json");
    let sf = SettingsFile::load(&missing).expect("load");
    assert_eq!(sf.data, serde_json::json!({}));

    // load(existing) → parsed.
    let existing = root.join("settings.json");
    fs::write(&existing, r#"{"mcpServers":{}}"#).unwrap();
    let sf = SettingsFile::load(&existing).expect("load");
    assert!(sf.data["mcpServers"].is_object());

    // save() pretty-prints and creates parent dirs.
    let nested = root.join("deep").join("path").join("settings.json");
    let mut sf = SettingsFile::load(&nested).expect("load");
    sf.data["key"] = serde_json::json!("value");
    sf.save().expect("save");
    let text = fs::read_to_string(&nested).unwrap();
    let val: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(val["key"], "value");

    // load(invalid JSON) → err.
    let bad = root.join("bad.json");
    fs::write(&bad, "not valid json {{{").unwrap();
    let result = SettingsFile::load(&bad);
    assert!(result.is_err(), "invalid settings JSON must be rejected");
}

// ───────────────────────────── F-05 · resolve_path ─────────────────────────────
// Oracle: kasetto src/fsops/mod.rs::tests::resolve_path_expands_only_leading_tilde.
#[test]
fn parity_resolve_path_expands_only_leading_tilde() {
    let base = Path::new("/base");
    let home = dirs_home().expect("home");
    assert_eq!(resolve_path(base, "~/skills"), home.join("skills"));
    assert_eq!(resolve_path(base, "~"), home);
    // A `~` that is not the home prefix is an ordinary path character.
    assert_eq!(
        resolve_path(base, "backup~old/skills"),
        Path::new("/base/backup~old/skills")
    );
    // Relative paths join onto base; absolute kept.
    assert_eq!(resolve_path(base, "rel/dir"), Path::new("/base/rel/dir"));
    assert_eq!(resolve_path(base, "/abs/dir"), Path::new("/abs/dir"));
}

// ───────────────────────────── F-06 · select_targets ─────────────────────────────
// Oracle: kasetto src/fsops/mod.rs::tests::{select_targets_wildcard_is_sorted,
// select_targets_reports_missing_skill, select_targets_prefers_explicit_path_override,
// select_targets_resolves_relative_path_against_source_root}.
#[test]
fn parity_select_targets_wildcard_is_sorted() {
    use std::collections::HashMap;
    let mut available = HashMap::new();
    for name in ["zeta", "alpha", "mid"] {
        available.insert(name.to_string(), PathBuf::from(format!("/tmp/{name}")));
    }
    let sf = SkillsField::Wildcard("*".into());
    let (targets, _) = select_targets(&sf, &available, Path::new("/tmp")).expect("select");
    let names: Vec<&str> = targets.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["alpha", "mid", "zeta"]);
}

#[test]
fn parity_select_targets_reports_missing_skill() {
    use std::collections::HashMap;
    let mut available = HashMap::new();
    available.insert("present".to_string(), PathBuf::from("/tmp/present"));
    let sf = SkillsField::List(vec![
        SkillTarget::Name("present".to_string()),
        SkillTarget::Name("missing".to_string()),
    ]);
    let (targets, broken): (_, Vec<BrokenSkill>) =
        select_targets(&sf, &available, Path::new("/tmp")).expect("select");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].0, "present");
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].name, "missing");
    assert!(broken[0].reason.contains("skill not found"));
}

#[test]
fn parity_select_targets_prefers_explicit_path_override() {
    use std::collections::HashMap;
    let root = tmp("targets");
    let nested = root.join("skills-repo");
    let skill_dir = nested.join("custom-skill");
    fs::create_dir_all(&skill_dir).expect("create dirs");
    fs::write(skill_dir.join("SKILL.md"), "# Custom\n\nDesc\n").expect("write skill");

    let mut available = HashMap::new();
    available.insert(
        "custom-skill".to_string(),
        PathBuf::from("/tmp/wrong-location"),
    );
    let sf = SkillsField::List(vec![SkillTarget::Obj {
        name: "custom-skill".to_string(),
        path: Some(nested.to_string_lossy().to_string()),
    }]);
    let (targets, broken) = select_targets(&sf, &available, Path::new("/tmp")).expect("select");
    assert!(broken.is_empty());
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].0, "custom-skill");
    assert_eq!(targets[0].1, skill_dir);
}

#[test]
fn parity_select_targets_resolves_relative_path_against_source_root() {
    use std::collections::HashMap;
    let root = tmp("targets-rel");
    let skill_dir = root.join("skills/productivity/grill-me");
    fs::create_dir_all(&skill_dir).expect("create dirs");
    fs::write(skill_dir.join("SKILL.md"), "# Grill\n\nDesc\n").expect("write skill");

    let available = HashMap::new();
    let sf = SkillsField::List(vec![SkillTarget::Obj {
        name: "grill-me".to_string(),
        path: Some("skills/productivity".to_string()),
    }]);
    let (targets, broken) = select_targets(&sf, &available, &root).expect("select");
    assert!(broken.is_empty());
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].0, "grill-me");
    assert_eq!(targets[0].1, skill_dir);
}

#[test]
fn parity_select_targets_invalid_field_errors() {
    use std::collections::HashMap;
    // A non-`*` wildcard string is an error ("invalid skills field").
    let available = HashMap::new();
    let sf = SkillsField::Wildcard("everything".into());
    let err = select_targets(&sf, &available, Path::new("/tmp")).unwrap_err();
    assert!(err.to_string().contains("invalid skills field"));
}

// ───────────────────────────── F-07 · resolve_destinations ─────────────────────────────
// Oracle: kasetto src/fsops/mod.rs::resolve_destinations (explicit destination →
// [resolve_path]; else per-agent project/global by scope; ERR when no agents).
// Agent path table per src/model/agent.rs::tests::agent_paths_cover_supported_presets.
#[test]
fn parity_resolve_destinations() {
    let base = Path::new("/proj");

    // Explicit destination wins, resolved against base.
    let cfg: Config = serde_yaml::from_str("destination: ./skills\nskills: []\n").expect("parse");
    let dests = resolve_destinations(base, &cfg, Scope::Project).expect("resolve");
    assert_eq!(dests, vec![PathBuf::from("/proj/skills")]);

    // No destination, project scope → per-agent project_path.
    let cfg: Config = serde_yaml::from_str("agent: claude-code\nskills: []\n").expect("parse");
    let dests = resolve_destinations(base, &cfg, Scope::Project).expect("resolve");
    assert_eq!(dests, vec![PathBuf::from("/proj/.claude/skills")]);

    // Global scope → per-agent global_path (under HOME).
    let home = dirs_home().expect("home");
    let cfg: Config = serde_yaml::from_str("agent: codex\nskills: []\n").expect("parse");
    let dests = resolve_destinations(base, &cfg, Scope::Global).expect("resolve");
    assert_eq!(dests, vec![home.join(".codex/skills")]);

    // No destination, no agent → error.
    let cfg: Config = serde_yaml::from_str("skills: []\n").expect("parse");
    let err = resolve_destinations(base, &cfg, Scope::Project).unwrap_err();
    assert!(err
        .to_string()
        .contains("must define either destination or a supported agent preset"));
}

// ───────────────────────────── F-08 · resolve_mcp_settings_targets ─────────────────────────────
// Oracle: kasetto src/fsops/mod.rs::resolve_mcp_settings_targets (per-agent mcp
// target by scope, dedup by path; empty agents → []).
#[test]
fn parity_resolve_mcp_settings_targets() {
    let proj = Path::new("/proj");

    // No agents → empty (not an error).
    let cfg: Config = serde_yaml::from_str("destination: ./skills\nskills: []\n").expect("parse");
    let out = resolve_mcp_settings_targets(&cfg, Scope::Project, proj).expect("resolve");
    assert!(out.is_empty());

    // Single agent → exactly one target.
    let cfg: Config = serde_yaml::from_str("agent: claude-code\nskills: []\n").expect("parse");
    let out = resolve_mcp_settings_targets(&cfg, Scope::Project, proj).expect("resolve");
    assert_eq!(out.len(), 1);

    // Dedup by path: claude-code listed twice collapses to one target.
    let cfg: Config =
        serde_yaml::from_str("agent:\n  - claude-code\n  - claude-code\nskills: []\n")
            .expect("parse");
    let out = resolve_mcp_settings_targets(&cfg, Scope::Project, proj).expect("resolve");
    assert_eq!(out.len(), 1, "duplicate agents dedup by path");
}

// ───────────────────────────── F-09 · resolve_command_targets ─────────────────────────────
// Oracle: kasetto src/fsops/mod.rs::resolve_command_targets (per-agent command
// target by scope, FILTER unsupported (None), dedup; empty → []). Cursor has NO
// global command path (src/model/agent.rs::commands_global_path → None for Cursor).
#[test]
fn parity_resolve_command_targets() {
    let proj = Path::new("/proj");

    // No agents → empty.
    let cfg: Config = serde_yaml::from_str("destination: ./skills\nskills: []\n").expect("parse");
    let out = resolve_command_targets(&cfg, Scope::Global, proj).expect("resolve");
    assert!(out.is_empty());

    // [claude-code, cursor] at GLOBAL scope: cursor has no global command dir →
    // filtered out, leaving exactly one target (claude-code).
    let cfg: Config =
        serde_yaml::from_str("agent:\n  - claude-code\n  - cursor\nskills: []\n").expect("parse");
    let out = resolve_command_targets(&cfg, Scope::Global, proj).expect("resolve");
    assert_eq!(out.len(), 1, "unsupported (None) command target filtered");
}

// ───────────────────────────── F-10 · scope_root / relativize_dest / resolve_dest ─────────────────────────────
// Oracle: kasetto src/fsops/mod.rs::{scope_root,relativize_dest,resolve_dest}.
// Lock-portability core: store install paths relative to a scope root, resolve back.
#[test]
fn parity_scope_root_relativize_resolve_dest() {
    let proj = Path::new("/proj");

    // scope_root: Project → project_root; Global → home.
    assert_eq!(
        scope_root(Scope::Project, proj).expect("scope_root"),
        PathBuf::from("/proj")
    );
    assert_eq!(
        scope_root(Scope::Global, proj).expect("scope_root"),
        dirs_home().expect("home")
    );

    // relativize_dest: under-root → relative; outside-root → absolute kept.
    let root = Path::new("/proj");
    assert_eq!(
        relativize_dest(Path::new("/proj/.claude/skills"), root),
        ".claude/skills"
    );
    assert_eq!(
        relativize_dest(Path::new("/elsewhere/skills"), root),
        "/elsewhere/skills"
    );

    // resolve_dest is the inverse: relative → root.join; absolute kept.
    assert_eq!(
        resolve_dest(".claude/skills", root),
        PathBuf::from("/proj/.claude/skills")
    );
    assert_eq!(
        resolve_dest("/elsewhere/skills", root),
        PathBuf::from("/elsewhere/skills")
    );

    // Round-trip: resolve_dest(relativize_dest(p)) == p for under-root paths.
    let p = Path::new("/proj/.claude/skills");
    assert_eq!(resolve_dest(&relativize_dest(p, root), root), p);
}

// ───────────────────────────── FE-01 · edit value types ─────────────────────────────
// Oracle: kasetto src/fsops/config_edit.rs (Section::key/singular; Pin/Selector/
// SourceItem/RemoveOutcome shapes — exercised via the public mutation API below).
#[test]
fn parity_config_edit_value_types() {
    assert_eq!(Section::Skills.key(), "skills");
    assert_eq!(Section::Mcps.key(), "mcps");
    assert_eq!(Section::Commands.key(), "commands");
    assert_eq!(Section::Skills.singular(), "skill");
    assert_eq!(Section::Mcps.singular(), "mcp");
    assert_eq!(Section::Commands.singular(), "command");

    // RemoveOutcome value identity (PartialEq, as used by remove_names callers).
    assert_eq!(
        RemoveOutcome::Names(vec!["a".into()]),
        RemoveOutcome::Names(vec!["a".into()])
    );
    assert_ne!(RemoveOutcome::WholeItem, RemoveOutcome::NotFound);

    // SourceItem assembles from Pin + Selector + sub_dir (constructible publicly).
    let item = SourceItem {
        source: "https://x/a".into(),
        pin: Pin::Ref("v1".into()),
        sub_dir: Some("pack".into()),
        selector: Selector::Names(vec!["alpha".into()]),
    };
    assert_eq!(item.source, "https://x/a");
    assert!(matches!(item.pin, Pin::Ref(ref r) if r == "v1"));
}

fn wildcard_item(source: &str) -> SourceItem {
    SourceItem {
        source: source.to_string(),
        pin: Pin::None,
        sub_dir: None,
        selector: Selector::Wildcard,
    }
}

// ───────────────────────────── FE-02 · insert_item ─────────────────────────────
// Oracle: kasetto src/fsops/config_edit.rs::tests::{insert_appends_under_existing_section_preserving_comments,
// insert_creates_section_when_absent, insert_normalizes_inline_empty_list,
// insert_into_empty_file, insert_with_ref_and_named_list, insert_keeps_trailing_comment_after_new_item,
// remove_then_insert_round_trips_indentation}.
#[test]
fn parity_insert_appends_preserving_comments() {
    let text = "# my config\nskills:\n  - source: https://x/a\n    skills: \"*\"\n";
    let out = insert_item(text, Section::Skills, &wildcard_item("https://x/b")).unwrap();
    assert_eq!(
        out,
        "# my config\n\
         skills:\n\
         \x20 - source: https://x/a\n\
         \x20\x20\x20 skills: \"*\"\n\
         \x20 - source: https://x/b\n\
         \x20\x20\x20 skills: \"*\"\n"
    );
}

#[test]
fn parity_insert_creates_section_when_absent() {
    let text = "agent: claude-code\n";
    let out = insert_item(text, Section::Mcps, &wildcard_item("https://x/m")).unwrap();
    assert_eq!(
        out,
        "agent: claude-code\n\nmcps:\n  - source: https://x/m\n    mcps: \"*\"\n"
    );
}

#[test]
fn parity_insert_normalizes_inline_empty_list() {
    let text = "skills: []\n";
    let out = insert_item(text, Section::Skills, &wildcard_item("https://x/a")).unwrap();
    assert_eq!(out, "skills:\n  - source: https://x/a\n    skills: \"*\"\n");
}

#[test]
fn parity_insert_into_empty_file() {
    let out = insert_item("", Section::Commands, &wildcard_item("https://x/c")).unwrap();
    assert_eq!(
        out,
        "commands:\n  - source: https://x/c\n    commands: \"*\"\n"
    );
}

#[test]
fn parity_insert_with_ref_and_named_list() {
    let item = SourceItem {
        source: "https://x/a".into(),
        pin: Pin::Ref("v2.0".into()),
        sub_dir: Some("pack".into()),
        selector: Selector::Names(vec!["alpha".into(), "beta".into()]),
    };
    let out = insert_item("skills: []\n", Section::Skills, &item).unwrap();
    assert_eq!(
        out,
        "skills:\n\
         \x20 - source: https://x/a\n\
         \x20\x20\x20 ref: v2.0\n\
         \x20\x20\x20 sub-dir: pack\n\
         \x20\x20\x20 skills:\n\
         \x20\x20\x20\x20\x20 - alpha\n\
         \x20\x20\x20\x20\x20 - beta\n"
    );
}

#[test]
fn parity_insert_keeps_trailing_comment_after_new_item() {
    let text =
        "skills:\n  - source: https://x/a\n    skills: \"*\"\n\n# trailing note\nagent: cursor\n";
    let out = insert_item(text, Section::Skills, &wildcard_item("https://x/b")).unwrap();
    assert!(out.contains("- source: https://x/b"));
    let b_pos = out.find("https://x/b").unwrap();
    let note_pos = out.find("# trailing note").unwrap();
    assert!(b_pos < note_pos);
    assert!(out.contains("\nagent: cursor\n"));
}

#[test]
fn parity_insert_rejects_inline_non_empty_list() {
    // FE-02 ERR arm: inline non-empty list cannot be edited in place.
    let text = "skills: [a, b]\n";
    let err = insert_item(text, Section::Skills, &wildcard_item("https://x/c")).unwrap_err();
    assert!(err.to_string().contains("inline list"));
}

#[test]
fn parity_insert_round_trips_four_space_indent() {
    // FE-06 indent_of/indent-inheritance: new item adopts the existing 4-space dash indent.
    let text = "skills:\n    - source: https://x/a\n      skills: \"*\"\n";
    let out = insert_item(text, Section::Skills, &wildcard_item("https://x/b")).unwrap();
    assert!(out.contains("\n    - source: https://x/b"));
    assert!(out.contains("\n      skills: \"*\""));
}

// ───────────────────────────── FE-03 · remove_item + find_match ─────────────────────────────
// Oracle: kasetto src/fsops/config_edit.rs::tests::{remove_deletes_only_the_matching_item,
// remove_absent_source_is_noop, remove_ambiguous_without_pin_errors, remove_disambiguates_by_pin,
// remove_ambiguous_same_pin_different_sub_dir_errors, remove_disambiguates_by_sub_dir,
// remove_sub_dir_empty_matches_entry_without_sub_dir}.
#[test]
fn parity_remove_item_and_find_match() {
    // Deletes only the matching item.
    let text = "skills:\n  - source: https://x/a\n    skills: \"*\"\n  - source: https://x/b\n    skills: \"*\"\n";
    let (out, removed) = remove_item(text, Section::Skills, "https://x/a", None, None).unwrap();
    assert!(removed);
    assert_eq!(out, "skills:\n  - source: https://x/b\n    skills: \"*\"\n");

    // Absent source is a no-op (Ok(false)).
    let text = "skills:\n  - source: https://x/a\n    skills: \"*\"\n";
    let (out, removed) = remove_item(text, Section::Skills, "https://x/z", None, None).unwrap();
    assert!(!removed);
    assert_eq!(out, text);

    // Ambiguous without pin → "disambiguate" error.
    let text = "skills:\n  - source: https://x/a\n    ref: v1\n    skills: \"*\"\n  - source: https://x/a\n    ref: v2\n    skills: \"*\"\n";
    let err = remove_item(text, Section::Skills, "https://x/a", None, None).unwrap_err();
    assert!(err.to_string().contains("disambiguate"));

    // Disambiguate by pin.
    let (out, removed) =
        remove_item(text, Section::Skills, "https://x/a", Some("v1"), None).unwrap();
    assert!(removed);
    assert!(out.contains("ref: v2"));
    assert!(!out.contains("ref: v1"));

    // Ambiguous same pin, different sub-dir → "disambiguate".
    let text = "skills:\n  - source: https://x/a\n    sub-dir: pack-a\n    skills: \"*\"\n  - source: https://x/a\n    sub-dir: pack-b\n    skills: \"*\"\n";
    let err = remove_item(text, Section::Skills, "https://x/a", None, None).unwrap_err();
    assert!(err.to_string().contains("disambiguate"));

    // Disambiguate by sub-dir.
    let (out, removed) =
        remove_item(text, Section::Skills, "https://x/a", None, Some("pack-a")).unwrap();
    assert!(removed);
    assert!(out.contains("sub-dir: pack-b"));
    assert!(!out.contains("sub-dir: pack-a"));

    // sub_dir == Some("") matches an entry that has NO sub-dir field.
    let text = "skills:\n  - source: https://x/a\n    sub-dir: pack-a\n    skills: \"*\"\n  - source: https://x/a\n    skills: \"*\"\n";
    let (out, removed) = remove_item(text, Section::Skills, "https://x/a", None, Some("")).unwrap();
    assert!(removed);
    assert!(out.contains("sub-dir: pack-a"));
}

#[test]
fn parity_remove_last_item_leaves_bare_section_header() {
    // Oracle: kasetto config_edit.rs::tests::remove_last_item_leaves_bare_section_header.
    let text = "skills:\n  - source: https://x/a\n    skills: \"*\"\n";
    let (out, removed) = remove_item(text, Section::Skills, "https://x/a", None, None).unwrap();
    assert!(removed);
    assert_eq!(out, "skills:\n");
    let reinserted = insert_item(&out, Section::Skills, &wildcard_item("https://x/b")).unwrap();
    assert_eq!(
        reinserted,
        "skills:\n  - source: https://x/b\n    skills: \"*\"\n"
    );
}

// ───────────────────────────── FE-04 · remove_names ─────────────────────────────
// Oracle: kasetto src/fsops/config_edit.rs::tests::{remove_names_subtracts_one_keeps_entry,
// remove_names_last_name_drops_whole_entry, remove_names_missing_name_errors_without_mutating,
// remove_names_on_wildcard_errors, remove_names_object_form_errors,
// remove_names_absent_source_is_not_found, remove_last_named_item_collapses_then_can_be_reused}.
#[test]
fn parity_remove_names() {
    // Subtract one, keep entry.
    let text = "skills:\n  - source: https://x/a\n    skills:\n      - alpha\n      - beta\n";
    let (out, outcome) = remove_names(
        text,
        Section::Skills,
        "https://x/a",
        None,
        None,
        &["alpha".into()],
    )
    .unwrap();
    assert_eq!(outcome, RemoveOutcome::Names(vec!["alpha".into()]));
    assert_eq!(
        out,
        "skills:\n  - source: https://x/a\n    skills:\n      - beta\n"
    );

    // Last name → drop whole entry (WholeItem).
    let text = "skills:\n  - source: https://x/a\n    skills:\n      - alpha\n";
    let (out, outcome) = remove_names(
        text,
        Section::Skills,
        "https://x/a",
        None,
        None,
        &["alpha".into()],
    )
    .unwrap();
    assert_eq!(outcome, RemoveOutcome::WholeItem);
    assert_eq!(out, "skills:\n");

    // Missing name → error, no mutation.
    let text = "skills:\n  - source: https://x/a\n    skills:\n      - alpha\n";
    let err = remove_names(
        text,
        Section::Skills,
        "https://x/a",
        None,
        None,
        &["ghost".into()],
    )
    .unwrap_err();
    assert!(err.to_string().contains("not found"));

    // Wildcard entry → error ("remove the whole entry").
    let text = "skills:\n  - source: https://x/a\n    skills: \"*\"\n";
    let err = remove_names(
        text,
        Section::Skills,
        "https://x/a",
        None,
        None,
        &["alpha".into()],
    )
    .unwrap_err();
    assert!(err.to_string().contains("wildcard"));

    // Object-form entry → error ("edit directly").
    let text =
        "skills:\n  - source: https://x/a\n    skills:\n      - name: alpha\n        path: lib\n";
    let err = remove_names(
        text,
        Section::Skills,
        "https://x/a",
        None,
        None,
        &["alpha".into()],
    )
    .unwrap_err();
    assert!(err.to_string().contains("object-form"));

    // Absent source → NotFound.
    let text = "skills:\n  - source: https://x/a\n    skills:\n      - alpha\n";
    let (out, outcome) = remove_names(
        text,
        Section::Skills,
        "https://x/z",
        None,
        None,
        &["alpha".into()],
    )
    .unwrap();
    assert_eq!(outcome, RemoveOutcome::NotFound);
    assert_eq!(out, text);
}

#[test]
fn parity_remove_last_named_item_collapses() {
    // Oracle: kasetto config_edit.rs::tests::remove_last_named_item_collapses_then_can_be_reused.
    let text = "mcps:\n  - source: https://x/a\n    mcps:\n      - foo\n";
    let (out, outcome) = remove_names(
        text,
        Section::Mcps,
        "https://x/a",
        None,
        None,
        &["foo".into()],
    )
    .unwrap();
    assert_eq!(outcome, RemoveOutcome::WholeItem);
    assert_eq!(out, "mcps:\n");
}

// ───────────────────────────── FE-05 · item_exists + render_item ─────────────────────────────
// Oracle: kasetto src/fsops/config_edit.rs::tests::item_exists_matches_full_identity.
// render_item is private; it is exercised by the FE-02 insert vectors above
// (the exact `- source:`/ref/sub-dir/selector emission with indent).
#[test]
fn parity_item_exists_matches_full_identity() {
    let text =
        "skills:\n  - source: https://x/a\n    ref: v1\n    sub-dir: pack\n    skills: \"*\"\n";
    let same = SourceItem {
        source: "https://x/a".into(),
        pin: Pin::Ref("v1".into()),
        sub_dir: Some("pack".into()),
        selector: Selector::Wildcard,
    };
    assert!(item_exists(text, Section::Skills, &same));

    let diff_ref = SourceItem {
        pin: Pin::Ref("v2".into()),
        ..same
    };
    assert!(!item_exists(text, Section::Skills, &diff_ref));
}

// ───────────────────────────── FE-06 · raw-line YAML primitives ─────────────────────────────
// Oracle: kasetto src/fsops/config_edit.rs (parse_items/find_top_level/indent_of/
// section_inline_value/split_lines/join_lines/splice). These are PRIVATE `fn`s and
// are not reachable through agent-env's public API; they are parity-proved
// TRANSITIVELY through the FE-02/03/04 vectors that drive insert/remove and assert
// exact byte-for-byte output (newline preservation, indent inheritance, inline-list
// normalization, section creation, trailing-comment placement). See:
//   - join_lines newline preservation: FE-02 every assert_eq ends with `\n` matching input.
//   - find_top_level/next_top_level: FE-03 multi-item sections resolve the right ranges.
//   - parse_items shallowest-dash + indent_of: FE-02 four-space round-trip.
//   - section_inline_value normalization: FE-02 insert_normalizes_inline_empty_list.
// A direct unit test of these helpers would require crate-internal access; recorded
// here as covered-via-public-API to keep the parity surface honest.
#[test]
fn parity_config_edit_primitives_via_public_api() {
    // join_lines: a file WITHOUT a trailing newline stays without one.
    let no_nl = "skills:\n  - source: https://x/a\n    skills: \"*\"";
    let out = insert_item(no_nl, Section::Skills, &wildcard_item("https://x/b")).unwrap();
    assert!(
        !out.ends_with('\n'),
        "join_lines must not add a trailing newline when input had none"
    );

    // find_top_level over a non-section key + section creation appends at EOF.
    let out = insert_item(
        "agent: claude-code\n",
        Section::Skills,
        &wildcard_item("https://x/a"),
    )
    .unwrap();
    assert!(out.contains("\nskills:\n  - source: https://x/a"));
}

// ═════════════════════════════ PASS-4: runtime-state / profile / dirs / config-path leaf ═════════════════════════════
// Differential parity for the machine-local-state + dirs + skill-profile + config-path
// cluster. Vectors VERBATIM from kasetto v3.2.0 certified tests; where envctl renamed a
// product-self-named string (app dir, env var, filename), the rename is noted inline and
// envctl's value is asserted — the RESOLUTION LOGIC, not the literal "kasetto" string, is
// what parity protects.

// ───────────────────────────── XC-03 · XDG / home base-directory resolution ─────────────────────────────
// Oracle: kasetto src/fsops/dirs.rs::{dirs_home, dirs_xdg_{config,data,cache}_home,
// dirs_kasetto_{config,data,cache}}. kasetto's dirs.rs has no #[cfg(test)] module — the
// resolution logic is the certified contract (exercised transitively by state.rs's
// round_trip_runtime_state, which pins XDG_CACHE_HOME and reads it back through this layer).
// We drive the SAME logic through agent-env's PUBLIC dirs::* API. The per-product leaf is
// envctl-renamed `kasetto` → `agent-env` (asserted as such); the XDG bases are byte-for-byte.
#[test]
fn parity_dirs_xdg_resolution() {
    let _g = ENV_LOCK.lock().unwrap();
    // Save + restore every env var this layer consults (no leak across parity tests).
    let keys = ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"];
    let saved: Vec<(&str, Option<String>)> =
        keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();

    // dirs_home reads $HOME verbatim (kasetto dirs_home: env var → PathBuf).
    std::env::set_var("HOME", "/home/tester");
    assert_eq!(dirs_home().unwrap(), PathBuf::from("/home/tester"));

    // XDG_CONFIG_HOME honored only when non-empty; the app-dir helper appends the product
    // leaf. kasetto: `$XDG_CONFIG_HOME/kasetto`; envctl renames the leaf → `agent-env`.
    std::env::set_var("XDG_CONFIG_HOME", "/custom/cfg");
    assert_eq!(
        dirs_xdg_config_home().unwrap(),
        PathBuf::from("/custom/cfg")
    );
    assert_eq!(
        dirs_agent_env_config().unwrap(),
        PathBuf::from("/custom/cfg/agent-env"),
        "envctl renames kasetto's `kasetto` config leaf → `agent-env`"
    );
    // Empty override → fall back to `$HOME/.config` (kasetto: `Ok(p) if !p.is_empty()`).
    std::env::set_var("XDG_CONFIG_HOME", "");
    assert_eq!(
        dirs_xdg_config_home().unwrap(),
        PathBuf::from("/home/tester/.config")
    );
    // Unset → same fallback.
    std::env::remove_var("XDG_CONFIG_HOME");
    assert_eq!(
        dirs_xdg_config_home().unwrap(),
        PathBuf::from("/home/tester/.config")
    );
    assert_eq!(
        dirs_agent_env_config().unwrap(),
        PathBuf::from("/home/tester/.config/agent-env")
    );

    // XDG_DATA_HOME: unset → `$HOME/.local/share`; leaf renamed kasetto → agent-env.
    std::env::remove_var("XDG_DATA_HOME");
    assert_eq!(
        dirs_xdg_data_home().unwrap(),
        PathBuf::from("/home/tester/.local/share")
    );
    assert_eq!(
        dirs_agent_env_data().unwrap(),
        PathBuf::from("/home/tester/.local/share/agent-env")
    );
    // Non-empty override honored.
    std::env::set_var("XDG_DATA_HOME", "/custom/data");
    assert_eq!(
        dirs_agent_env_data().unwrap(),
        PathBuf::from("/custom/data/agent-env")
    );

    // XDG_CACHE_HOME: unset → `$HOME/.cache`; leaf renamed kasetto → agent-env.
    std::env::remove_var("XDG_CACHE_HOME");
    assert_eq!(
        dirs_xdg_cache_home().unwrap(),
        PathBuf::from("/home/tester/.cache")
    );
    assert_eq!(
        dirs_agent_env_cache().unwrap(),
        PathBuf::from("/home/tester/.cache/agent-env")
    );
    // Non-empty override honored.
    std::env::set_var("XDG_CACHE_HOME", "/custom/cache");
    assert_eq!(
        dirs_agent_env_cache().unwrap(),
        PathBuf::from("/custom/cache/agent-env")
    );

    // HOME unset → the documented "HOME is not set" error (kasetto dirs_home err arm).
    std::env::remove_var("HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
    assert!(dirs_home().is_err());
    // …and the fallback consumers propagate that error rather than panicking.
    assert!(dirs_xdg_config_home().is_err());

    for (k, v) in saved {
        match v {
            Some(val) => std::env::set_var(k, val),
            None => std::env::remove_var(k),
        }
    }
}

// ───────────────────────────── ST-01/ST-02 · RuntimeState round-trip + state path ─────────────────────────────
// Oracle: kasetto src/state.rs::tests::{round_trip_runtime_state,
// load_latest_failures_extracts_failed_actions}. The lock↔runtime separation (ADR-0001 §4)
// is preserved: state lives in the cache dir keyed by hash_str(lock_path), never in the lock.
// envctl renames kasetto's cache leaf (`dirs_kasetto_cache` → `dirs_agent_env_cache`) and
// resolves the global-data dir explicitly; for Scope::Project the lock path is
// `project_root/agent-env.lock` in BOTH, so the cache key is identical.
#[test]
fn parity_runtime_state_round_trip() {
    let _g = ENV_LOCK.lock().unwrap();
    let keys = ["HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME"];
    let saved: Vec<(&str, Option<String>)> =
        keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();

    let home = tmp("rt-home");
    let cache = tmp("rt-cache");
    let data = tmp("rt-data");
    // Pin every XDG base runtime_state_path touches (HOME + DATA for the data-dir resolution,
    // CACHE for the state file itself) so the vector is hermetic.
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_DATA_HOME", &data);
    let root = tmp("rt-proj");

    // VERBATIM kasetto round_trip_runtime_state vector.
    let mut state = RuntimeState {
        last_run: Some("123".into()),
        ..Default::default()
    };
    state.set_updated_at("src::a", "100".into());
    state.save_report_json(r#"{"actions":[]}"#);

    save_runtime_state(&state, Scope::Project, &root).unwrap();
    let loaded = load_runtime_state(Scope::Project, &root).unwrap();
    assert_eq!(loaded.last_run.as_deref(), Some("123"));
    assert_eq!(loaded.updated_at("src::a"), "100");
    assert_eq!(loaded.updated_at("missing"), "");

    // ST-02 path shape: cache/runtime/<hash>.json (hash of the lock path).
    let path = runtime_state_path(Scope::Project, &root).unwrap();
    assert!(path.starts_with(cache.join("agent-env/runtime")));
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("json"));

    // clear removes the file; a subsequent load returns RuntimeState::default().
    clear_runtime_state(Scope::Project, &root).unwrap();
    assert!(!path.exists());
    assert!(load_runtime_state(Scope::Project, &root)
        .unwrap()
        .last_run
        .is_none());
    // Missing file → default (no error).
    assert!(load_runtime_state(Scope::Project, &root)
        .unwrap()
        .installed_at
        .is_empty());

    for (k, v) in saved {
        match v {
            Some(val) => std::env::set_var(k, val),
            None => std::env::remove_var(k),
        }
    }
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&cache);
    let _ = fs::remove_dir_all(&data);
    let _ = fs::remove_dir_all(&root);
}

// ST-01 (pure, env-free): RuntimeState helpers + load_latest_failures action extraction.
// Oracle: kasetto src/state.rs::tests::load_latest_failures_extracts_failed_actions
// (+ updated_at/set_updated_at/forget exercised by round_trip).
#[test]
fn parity_runtime_state_failures_and_helpers() {
    // forget drops an install stamp; others survive.
    let mut state = RuntimeState::default();
    state.set_updated_at("src::a", "100".into());
    state.set_updated_at("src::b", "200".into());
    state.forget("src::a");
    assert_eq!(state.updated_at("src::a"), "");
    assert_eq!(state.updated_at("src::b"), "200");

    // VERBATIM kasetto vector: only `broken`/`source_error` actions become failures, in order.
    let mut state = RuntimeState::default();
    state.save_report_json(
        r#"{"actions":[
                {"status":"installed","skill":"good","source":"s"},
                {"status":"broken","skill":"bad","source":"s","error":"missing"},
                {"status":"source_error","skill":"err","source":"s2","error":"timeout"}
            ]}"#,
    );
    let failures = state.load_latest_failures();
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0].name, "bad");
    assert_eq!(failures[0].source, "s");
    assert_eq!(failures[0].reason, "missing");
    assert_eq!(failures[1].name, "err");
    assert_eq!(failures[1].reason, "timeout");

    // No cached report → no failures (kasetto early-return arm).
    let empty = RuntimeState::default();
    assert!(empty.load_latest_failures().is_empty());
}

// ───────────────────────────── P-01 · read_skill_profile(_from_dir) ─────────────────────────────
// Oracle: kasetto src/profile.rs::tests::{profile_prefers_heading_and_frontmatter_description,
// profile_falls_back_when_file_missing}. envctl ports this verbatim (no rename); the UI-only
// `list_color_enabled` helper is intentionally not ported (library is non-printing).
#[test]
fn parity_read_skill_profile() {
    // VERBATIM kasetto vector: front-matter `description:` wins; first `#` heading → title.
    let dir = tmp("profile");
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: slug-name\ndescription: from-front-matter\n---\n\n# Human Title\n\nBody line.\n",
    )
    .unwrap();
    let (name, description) = read_skill_profile_from_dir(&dir, "fallback");
    assert_eq!(name, "Human Title");
    assert_eq!(description, "from-front-matter");
    let _ = fs::remove_dir_all(&dir);

    // VERBATIM kasetto vector: missing SKILL.md → (fallback_name, "No description.").
    let dir = tmp("profile-missing");
    let (name, description) = read_skill_profile_from_dir(&dir, "fallback-name");
    assert_eq!(name, "fallback-name");
    assert_eq!(description, "No description.");
    let _ = fs::remove_dir_all(&dir);

    // read_skill_profile(destination) overload resolves the path (same logic, string arg).
    // Mirrors the title-from-heading + body-first-line description path.
    let dir = tmp("profile-dest");
    fs::write(dir.join("SKILL.md"), "# Title\n\nDesc.\n").unwrap();
    let (name, description) = read_skill_profile(&dir.to_string_lossy(), "fallback");
    assert_eq!(name, "Title");
    assert_eq!(description, "Desc.");
    let _ = fs::remove_dir_all(&dir);
}

// ───────────────────────────── P-02 · format_updated_ago ─────────────────────────────
// Oracle: kasetto src/profile.rs::tests::format_updated_ago_returns_unknown_for_invalid_input
// (+ the documented Ns/m/h/d-ago / "in Ns" thresholds in src/profile.rs). Ported verbatim.
#[test]
fn parity_format_updated_ago() {
    use envctl_agent_env::now_unix;
    // VERBATIM kasetto vector: an unparseable stamp → "unknown".
    assert_eq!(format_updated_ago("not-a-timestamp"), "unknown");

    // Threshold boundaries from kasetto src/profile.rs (d<60 → s, <3600 → m, <86_400 → h, else d).
    let now = now_unix();
    assert_eq!(format_updated_ago(&now.to_string()), "0s ago");
    assert_eq!(format_updated_ago(&(now - 30).to_string()), "30s ago");
    assert_eq!(format_updated_ago(&(now - 120).to_string()), "2m ago");
    assert_eq!(format_updated_ago(&(now - 7_200).to_string()), "2h ago");
    assert_eq!(format_updated_ago(&(now - 172_800).to_string()), "2d ago");
    // Future stamp → "in Ns".
    assert_eq!(format_updated_ago(&(now + 5).to_string()), "in 5s");
}

// ───────────────────────────── CP-01 · default_config_path / resolve_config_path ─────────────────────────────
// Oracle: kasetto src/lib.rs::tests (all 9). The PRIORITY LOGIC is verbatim; envctl renames
// the env var (`KASETTO_CONFIG` → `ENVCTL_AGENT_CONFIG`) and the local/global filename
// (`kasetto.yaml` → `agent-env.yaml`). resolve_config_path is the pure testable core; we drive
// it through the PUBLIC API and assert envctl's renamed constants (DEFAULT_CONFIG_FILENAME).
#[test]
fn parity_resolve_config_path_priority() {
    // Renamed env-var/filename constants carry the envctl identity (priority logic unchanged).
    assert_eq!(CONFIG_ENV_VAR, "ENVCTL_AGENT_CONFIG");
    assert_eq!(DEFAULT_CONFIG_FILENAME, "agent-env.yaml");
    assert_eq!(DEFAULT_GLOBAL_CONFIG_FILENAME, "agent-env.yaml");
    assert_eq!(PREFERENCES_FILENAME, "config.yaml");

    // (1) env var takes highest priority — kasetto env_var_takes_highest_priority.
    assert_eq!(
        resolve_config_path(
            Some("https://example.com/team.yaml".into()),
            None,
            true,
            None
        ),
        "https://example.com/team.yaml"
    );

    let prefs_dir = tmp("cp-prefs");
    let prefs = prefs_dir.join("config.yaml");
    fs::write(&prefs, "source: https://example.com/remote.yaml\n").unwrap();

    // (2) preferences `source:` used when no env var, no local — preferences_file_source_used_when_no_env_var.
    assert_eq!(
        resolve_config_path(None, Some(&prefs), false, None),
        "https://example.com/remote.yaml"
    );
    // (3) env var beats preferences file — env_var_beats_preferences_file.
    assert_eq!(
        resolve_config_path(
            Some("https://example.com/env.yaml".into()),
            Some(&prefs),
            false,
            None
        ),
        "https://example.com/env.yaml"
    );
    // (4) local config beats preferences `source:` — local_kasetto_yaml_beats_preferences_source.
    assert_eq!(
        resolve_config_path(None, Some(&prefs), true, None),
        DEFAULT_CONFIG_FILENAME
    );
    let _ = fs::remove_dir_all(&prefs_dir);

    // (5) local used when no env/prefs — local_kasetto_yaml_used_when_no_env_or_prefs.
    assert_eq!(
        resolve_config_path(None, None, true, None),
        DEFAULT_CONFIG_FILENAME
    );

    // (6) global config used when local absent — global_config_used_when_local_absent.
    let global_dir = tmp("cp-global");
    let global = global_dir.join("agent-env.yaml"); // renamed from kasetto.yaml
    fs::write(&global, "agent: claude-code\nskills: []\n").unwrap();
    assert_eq!(
        resolve_config_path(None, None, false, Some(&global)),
        global.to_string_lossy()
    );
    let _ = fs::remove_dir_all(&global_dir);

    // (7) fall back to the local filename when nothing exists — falls_back_to_local_filename_when_nothing_exists.
    assert_eq!(
        resolve_config_path(None, None, false, None),
        DEFAULT_CONFIG_FILENAME
    );

    // (8) a missing prefs file is skipped silently — missing_prefs_file_is_skipped_silently.
    let missing_dir = tmp("cp-no-prefs");
    let missing = missing_dir.join("config.yaml");
    assert_eq!(
        resolve_config_path(None, Some(&missing), true, None),
        DEFAULT_CONFIG_FILENAME
    );
    let _ = fs::remove_dir_all(&missing_dir);

    // (9) a prefs file without a `source:` key is skipped — prefs_file_without_source_key_is_skipped.
    let no_src_dir = tmp("cp-prefs-no-source");
    let no_src = no_src_dir.join("config.yaml");
    fs::write(&no_src, "some_other_key: value\n").unwrap();
    assert_eq!(
        resolve_config_path(None, Some(&no_src), true, None),
        DEFAULT_CONFIG_FILENAME
    );
    let _ = fs::remove_dir_all(&no_src_dir);
}

// CP-01 (env arm): default_config_path honors the RENAMED env override end-to-end.
// Oracle: the env-var arm of kasetto default_config_path (priority 1), proven through the
// public, env-reading entry point. envctl's env var is ENVCTL_AGENT_CONFIG.
#[test]
fn parity_default_config_path_env_override() {
    let _g = ENV_LOCK.lock().unwrap();
    let prev = std::env::var(CONFIG_ENV_VAR).ok();
    std::env::set_var(CONFIG_ENV_VAR, "https://example.com/from-env.yaml");
    assert_eq!(default_config_path(), "https://example.com/from-env.yaml");
    match prev {
        Some(v) => std::env::set_var(CONFIG_ENV_VAR, v),
        None => std::env::remove_var(CONFIG_ENV_VAR),
    }
}
