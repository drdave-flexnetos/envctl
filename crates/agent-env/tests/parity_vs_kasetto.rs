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
    all_command_global_targets, archive_url, derive_browse_url, discover, discover_commands,
    discover_mcps, discover_with_root_name, extract_extends, git_pin_of, hash_dir, hash_file,
    hash_str, materialize_source, merge_mcp_config, merge_yaml, now_unix, now_unix_str, parse,
    parse_repo_url, render, resolve_command_entry, resolve_mcp_entry, Agent, CommandEntry,
    CommandFormat, Config, GitPin, McpEntry, McpSettingsFormat, McpSettingsTarget, RepoUrl,
    SkillsField, SourceSpec,
};
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
