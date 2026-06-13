//! No-downgrade proof (ADR-0001 §1): a config exercising **all 6 keys + `extends`** must
//! round-trip through the agent-env loader with every field preserved. If any key silently
//! drops, this test fails — the absorption would be a downgrade.

use std::fs;
use std::path::PathBuf;

use envctl_agent_env::{
    load_config_recursive, AgentField, CommandsField, GitPin, McpsField, Scope, SkillsField,
};

fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn all_six_keys_plus_extends_round_trip() {
    let root = temp_dir("agent-env-no-downgrade");

    // base.yaml carries `destination` + a skills source the child does not override.
    let base = root.join("base.yaml");
    fs::write(
        &base,
        "destination: ./out/skills\n\
         skills:\n\
         \x20 - source: https://github.com/base/pack\n\
         \x20\x20\x20 ref: v1.0\n\
         \x20\x20\x20 sub-dir: skills\n\
         \x20\x20\x20 skills: \"*\"\n",
    )
    .unwrap();

    // child.yaml extends base and exercises scope, agent (Many), mcps, commands, and a
    // second skills source.
    let child = root.join("child.yaml");
    fs::write(
        &child,
        "extends: ./base.yaml\n\
         scope: project\n\
         agent:\n\
         \x20 - claude-code\n\
         \x20 - codex\n\
         skills:\n\
         \x20 - source: https://gitlab.com/group/sub/repo\n\
         \x20\x20\x20 branch: dev\n\
         \x20\x20\x20 skills:\n\
         \x20\x20\x20\x20\x20 - one\n\
         mcps:\n\
         \x20 - source: https://github.com/me/mcps\n\
         \x20\x20\x20 mcps: \"*\"\n\
         commands:\n\
         \x20 - source: https://github.com/me/cmds\n\
         \x20\x20\x20 ref: v2\n\
         \x20\x20\x20 commands:\n\
         \x20\x20\x20\x20\x20 - review-pr\n",
    )
    .unwrap();

    let mut visited = std::collections::HashSet::new();
    let (value, _base_dir, _label) =
        load_config_recursive(child.to_str().unwrap(), None, &mut visited, 0).expect("load");
    let cfg: envctl_agent_env::Config = serde_yaml::from_value(value).expect("deserialize");

    // 1. destination — inherited from base.
    assert_eq!(cfg.destination.as_deref(), Some("./out/skills"));

    // 2. scope — set by child.
    assert_eq!(cfg.scope, Some(Scope::Project));

    // 3. agent — Many with both presets.
    match cfg.agent.as_ref().expect("agent present") {
        AgentField::Many(v) => assert_eq!(v.len(), 2),
        other => panic!("expected Many, got {other:?}"),
    }

    // 4. skills — base's `*` source + child's narrowed-list source (distinct identities).
    assert_eq!(cfg.skills.len(), 2);
    let base_skill = cfg
        .skills
        .iter()
        .find(|s| s.source == "https://github.com/base/pack")
        .expect("base skill preserved");
    assert_eq!(base_skill.git_pin(), GitPin::Ref("v1.0".into()));
    assert_eq!(base_skill.sub_dir.as_deref(), Some("skills"));
    assert!(matches!(base_skill.skills, SkillsField::Wildcard(_)));
    let child_skill = cfg
        .skills
        .iter()
        .find(|s| s.source == "https://gitlab.com/group/sub/repo")
        .expect("child skill present");
    assert_eq!(child_skill.git_pin(), GitPin::Branch("dev".into()));
    assert!(matches!(&child_skill.skills, SkillsField::List(items) if items.len() == 1));

    // 5. mcps — wildcard preserved.
    assert_eq!(cfg.mcps.len(), 1);
    assert!(matches!(cfg.mcps[0].mcps, McpsField::Wildcard(_)));

    // 6. commands — pinned list preserved.
    assert_eq!(cfg.commands.len(), 1);
    assert_eq!(cfg.commands[0].git_ref.as_deref(), Some("v2"));
    assert!(matches!(&cfg.commands[0].commands, CommandsField::List(items) if items.len() == 1));

    let _ = fs::remove_dir_all(&root);
}
