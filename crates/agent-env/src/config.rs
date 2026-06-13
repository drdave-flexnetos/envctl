//! The `kasetto.yaml` / agent-env config model — ported from kasetto v3.2.0
//! `src/model/config.rs` + `src/model/agent.rs`.
//!
//! The schema is **6 keys + `extends`**: `destination`, `scope`, `agent`, `skills`,
//! `mcps`, `commands` (the `extends` key is stripped at the YAML layer before
//! deserialization — see [`crate::extend`]). The `*Field` / `*Entry` / `*SourceSpec`
//! enums are `#[serde(untagged)]` so a single YAML key accepts either a `"*"` wildcard
//! string or an explicit list of names / `{ name, path }` objects.

use serde::{Deserialize, Serialize};

/// Provisioning scope — drives scope-relative lock destinations.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    #[serde(rename = "global")]
    Global,
    #[serde(rename = "project")]
    Project,
}

/// Deserialized `kasetto.yaml`: the full sync request (destination, scope, agents, sources).
#[derive(Debug, Deserialize)]
pub struct Config {
    pub destination: Option<String>,
    #[serde(default)]
    pub scope: Option<Scope>,
    #[serde(default)]
    pub agent: Option<AgentField>,
    #[serde(default)]
    pub skills: Vec<SourceSpec>,
    #[serde(default)]
    pub mcps: Vec<McpSourceSpec>,
    #[serde(default)]
    pub commands: Vec<CommandSourceSpec>,
}

impl Config {
    /// The agents this config targets (flattening the `One | Many` field).
    pub fn agents(&self) -> Vec<Agent> {
        match &self.agent {
            Some(AgentField::One(a)) => vec![*a],
            Some(AgentField::Many(v)) => v.clone(),
            None => vec![],
        }
    }

    /// The effective scope: config YAML `scope:` field, else `Global` default.
    pub fn resolved_scope(&self) -> Scope {
        self.scope.unwrap_or_default()
    }
}

/// Resolve the effective scope: CLI override > config YAML `scope:` field > Global default.
///
/// Note: kasetto's variant additionally falls back to reading the default config path when
/// no `Config` is supplied; that file-read fallback is a `sync`-command concern deferred to
/// TASK-0013. The library form takes the loaded `Config` directly.
pub fn resolve_scope(cli_override: Option<Scope>, cfg: Option<&Config>) -> Scope {
    if let Some(s) = cli_override {
        return s;
    }
    if let Some(cfg) = cfg {
        return cfg.resolved_scope();
    }
    Scope::Global
}

/// A skill source: where to fetch from and which skills to install.
#[derive(Debug, Deserialize)]
pub struct SourceSpec {
    pub source: String,
    pub branch: Option<String>,
    /// Pin to a git tag, commit SHA, or any ref. Takes priority over `branch`.
    /// When set, no main/master fallback is attempted.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional subdirectory inside the source repo/path to use as the skill root.
    /// Supports both `sub-dir` and `sub_dir` YAML keys.
    #[serde(default, rename = "sub-dir", alias = "sub_dir")]
    pub sub_dir: Option<String>,
    pub skills: SkillsField,
}

/// What the user specified to identify a version of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitPin {
    /// Explicit ref (tag, SHA, etc.) — no fallback.
    Ref(String),
    /// Explicit branch name — no fallback.
    Branch(String),
    /// Nothing specified — try "main", fall back to "master".
    Default,
}

impl SourceSpec {
    /// Resolve the effective git pin: `ref` > `branch` > default.
    pub fn git_pin(&self) -> GitPin {
        if let Some(r) = &self.git_ref {
            GitPin::Ref(r.clone())
        } else if let Some(b) = &self.branch {
            GitPin::Branch(b.clone())
        } else {
            GitPin::Default
        }
    }

    /// The `source_revision` label this spec records: `ref:<r>` / `branch:<b>` /
    /// `branch:main` for remotes, `local` for a local path. Used to detect when a
    /// user retargeted a source (changed `ref` / `branch`).
    pub fn expected_revision(&self) -> String {
        if !self.source.contains("://") {
            return "local".into();
        }
        match self.git_pin() {
            GitPin::Ref(r) => format!("ref:{r}"),
            GitPin::Branch(b) => format!("branch:{b}"),
            GitPin::Default => "branch:main".into(),
        }
    }
}

/// Free-function form of [`SourceSpec::git_pin`] (convenience for callers that hold a
/// `(ref, branch)` pair without a full `SourceSpec`).
pub fn git_pin_of(git_ref: Option<&str>, branch: Option<&str>) -> GitPin {
    if let Some(r) = git_ref {
        GitPin::Ref(r.to_string())
    } else if let Some(b) = branch {
        GitPin::Branch(b.to_string())
    } else {
        GitPin::Default
    }
}

/// An MCP source: where to fetch from and which MCP servers to install.
#[derive(Debug, Deserialize)]
pub struct McpSourceSpec {
    pub source: String,
    pub branch: Option<String>,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Mirrors `skills[].skills`: `"*"` to discover all, or a list of names / `{ name, path }`.
    pub mcps: McpsField,
}

impl McpSourceSpec {
    /// View this MCP source as a generic [`SourceSpec`] (wildcard skills) for the resolver.
    pub fn as_source_spec(&self) -> SourceSpec {
        SourceSpec {
            source: self.source.clone(),
            branch: self.branch.clone(),
            git_ref: self.git_ref.clone(),
            sub_dir: None,
            skills: SkillsField::Wildcard("*".to_string()),
        }
    }
}

/// The `mcps` field on an `McpSourceSpec` — mirrors `SkillsField` exactly.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum McpsField {
    /// `mcps: "*"` — discover all MCP files in the source.
    Wildcard(String),
    /// `mcps: [...]` — explicit list of names or `{ name, path }` objects.
    List(Vec<McpEntry>),
}

/// One entry in `mcps[].mcps` — mirrors `SkillTarget`.
///
/// - Plain string `"github"` → `mcps/github.json`
/// - Object `{ name: github, path: tools }` → `tools/github.json`
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum McpEntry {
    Name(String),
    Obj { name: String, path: Option<String> },
}

/// A command source: where to fetch from and which slash commands to install.
#[derive(Debug, Deserialize)]
pub struct CommandSourceSpec {
    pub source: String,
    pub branch: Option<String>,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    #[serde(default, rename = "sub-dir", alias = "sub_dir")]
    pub sub_dir: Option<String>,
    pub commands: CommandsField,
}

impl CommandSourceSpec {
    /// View this command source as a generic [`SourceSpec`] (wildcard skills) for the resolver.
    pub fn as_source_spec(&self) -> SourceSpec {
        SourceSpec {
            source: self.source.clone(),
            branch: self.branch.clone(),
            git_ref: self.git_ref.clone(),
            sub_dir: self.sub_dir.clone(),
            skills: SkillsField::Wildcard("*".to_string()),
        }
    }
}

/// The `commands` field on a `CommandSourceSpec` — mirrors `McpsField` / `SkillsField`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CommandsField {
    Wildcard(String),
    List(Vec<CommandEntry>),
}

/// One entry in `commands[].commands` — mirrors `McpEntry`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CommandEntry {
    Name(String),
    Obj { name: String, path: Option<String> },
}

/// The `skills` field on a `SourceSpec`: `"*"` wildcard or an explicit list.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SkillsField {
    Wildcard(String),
    List(Vec<SkillTarget>),
}

/// One entry in `skills[].skills`: a bare name or a `{ name, path }` object.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SkillTarget {
    Name(String),
    Obj { name: String, path: Option<String> },
}

/// The `agent` config key — a single preset or a list of presets.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum AgentField {
    One(Agent),
    Many(Vec<Agent>),
}

/// The 21-preset agent enum — the named agent targets kasetto knows how to write native
/// paths for. Ported verbatim from kasetto v3.2.0 `src/model/agent.rs` (a subset would be a
/// downgrade per ADR-0001 §6).
///
/// NOTE (TASK-0012 scope): only the enum **shape** + serde renames are ported here. The
/// per-agent native path-mapping methods (`global_path`, `mcp_settings_target`,
/// `commands_*_path`, …) are deferred to TASK-0013 (the Engine wiring card).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    #[serde(rename = "amp")]
    Amp,
    #[serde(rename = "antigravity")]
    Antigravity,
    #[serde(rename = "augment")]
    Augment,
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "cline")]
    Cline,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "continue")]
    Continue,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "gemini-cli")]
    GeminiCli,
    #[serde(rename = "github-copilot")]
    GithubCopilot,
    #[serde(rename = "goose")]
    Goose,
    #[serde(rename = "junie")]
    Junie,
    #[serde(rename = "kiro-cli")]
    KiroCli,
    #[serde(rename = "openclaw")]
    OpenClaw,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "openhands")]
    OpenHands,
    #[serde(rename = "replit")]
    Replit,
    #[serde(rename = "roo")]
    Roo,
    #[serde(rename = "trae")]
    Trae,
    #[serde(rename = "warp")]
    Warp,
    #[serde(rename = "windsurf")]
    Windsurf,
}

/// Every preset value (for clean / enumerating native paths — used by TASK-0013).
pub const AGENT_PRESETS: &[Agent] = &[
    Agent::Amp,
    Agent::Antigravity,
    Agent::Augment,
    Agent::ClaudeCode,
    Agent::Cline,
    Agent::Codex,
    Agent::Continue,
    Agent::Cursor,
    Agent::GeminiCli,
    Agent::GithubCopilot,
    Agent::Goose,
    Agent::Junie,
    Agent::KiroCli,
    Agent::OpenClaw,
    Agent::OpenCode,
    Agent::OpenHands,
    Agent::Replit,
    Agent::Roo,
    Agent::Trae,
    Agent::Warp,
    Agent::Windsurf,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_scope_prefers_cli_override() {
        assert_eq!(resolve_scope(Some(Scope::Project), None), Scope::Project);
        assert_eq!(resolve_scope(Some(Scope::Global), None), Scope::Global);
    }

    #[test]
    fn resolve_scope_uses_config_then_default() {
        let cfg: Config = serde_yaml::from_str("scope: project\nskills: []\n").expect("parse");
        assert_eq!(resolve_scope(None, Some(&cfg)), Scope::Project);
        assert_eq!(resolve_scope(None, None), Scope::Global);
    }

    #[test]
    fn config_commands_parses_wildcard() {
        let yaml = r#"
skills: []
commands:
  - source: https://github.com/me/cmds
    commands: "*"
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(cfg.commands.len(), 1);
        assert!(matches!(
            cfg.commands[0].commands,
            CommandsField::Wildcard(_)
        ));
    }

    #[test]
    fn config_commands_parses_plain_strings_and_objects() {
        let yaml = r#"
skills: []
commands:
  - source: https://github.com/me/cmds
    ref: v1.0
    sub-dir: commands
    commands:
      - review-pr
      - name: deploy
        path: ops
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(cfg.commands[0].git_ref.as_deref(), Some("v1.0"));
        assert_eq!(cfg.commands[0].sub_dir.as_deref(), Some("commands"));
        let CommandsField::List(ref entries) = cfg.commands[0].commands else {
            panic!("expected list");
        };
        assert_eq!(entries.len(), 2);
        assert!(matches!(&entries[0], CommandEntry::Name(n) if n == "review-pr"));
        assert!(
            matches!(&entries[1], CommandEntry::Obj { name, path: Some(p) } if name == "deploy" && p == "ops")
        );
    }

    #[test]
    fn config_commands_supports_sub_dir_alias() {
        let yaml = r#"
skills: []
commands:
  - source: https://github.com/me/cmds
    sub_dir: nested/commands
    commands: "*"
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(cfg.commands[0].sub_dir.as_deref(), Some("nested/commands"));
    }

    #[test]
    fn skills_parses_wildcard_list_and_objects() {
        let yaml = r#"
skills:
  - source: https://github.com/me/a
    skills: "*"
  - source: https://github.com/me/b
    sub-dir: pack
    skills:
      - one
      - name: two
        path: nested
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(cfg.skills.len(), 2);
        assert!(matches!(cfg.skills[0].skills, SkillsField::Wildcard(_)));
        assert_eq!(cfg.skills[1].sub_dir.as_deref(), Some("pack"));
        let SkillsField::List(ref items) = cfg.skills[1].skills else {
            panic!("expected list");
        };
        assert!(matches!(&items[0], SkillTarget::Name(n) if n == "one"));
        assert!(
            matches!(&items[1], SkillTarget::Obj { name, path: Some(p) } if name == "two" && p == "nested")
        );
    }

    #[test]
    fn git_pin_precedence_ref_beats_branch() {
        let cfg: Config = serde_yaml::from_str(
            "skills:\n  - source: https://x/a\n    branch: dev\n    ref: v9\n    skills: \"*\"\n",
        )
        .expect("parse");
        assert_eq!(cfg.skills[0].git_pin(), GitPin::Ref("v9".into()));
        assert_eq!(cfg.skills[0].expected_revision(), "ref:v9");
    }

    #[test]
    fn git_pin_branch_then_default() {
        let branch: Config = serde_yaml::from_str(
            "skills:\n  - source: https://x/a\n    branch: dev\n    skills: \"*\"\n",
        )
        .expect("parse");
        assert_eq!(branch.skills[0].git_pin(), GitPin::Branch("dev".into()));
        assert_eq!(branch.skills[0].expected_revision(), "branch:dev");

        let default: Config =
            serde_yaml::from_str("skills:\n  - source: https://x/a\n    skills: \"*\"\n")
                .expect("parse");
        assert_eq!(default.skills[0].git_pin(), GitPin::Default);
        assert_eq!(default.skills[0].expected_revision(), "branch:main");
    }

    #[test]
    fn local_source_revision_is_local() {
        let cfg: Config =
            serde_yaml::from_str("skills:\n  - source: ./local/pack\n    skills: \"*\"\n")
                .expect("parse");
        assert_eq!(cfg.skills[0].expected_revision(), "local");
    }

    #[test]
    fn git_pin_of_free_function_matches_method() {
        assert_eq!(
            git_pin_of(Some("v1"), Some("dev")),
            GitPin::Ref("v1".into())
        );
        assert_eq!(git_pin_of(None, Some("dev")), GitPin::Branch("dev".into()));
        assert_eq!(git_pin_of(None, None), GitPin::Default);
    }

    #[test]
    fn agent_field_parses_one_and_many() {
        let one: Config = serde_yaml::from_str("agent: claude-code\nskills: []\n").expect("parse");
        assert_eq!(one.agents(), vec![Agent::ClaudeCode]);

        let many: Config =
            serde_yaml::from_str("agent:\n  - codex\n  - cursor\nskills: []\n").expect("parse");
        assert_eq!(many.agents(), vec![Agent::Codex, Agent::Cursor]);

        let none: Config = serde_yaml::from_str("skills: []\n").expect("parse");
        assert!(none.agents().is_empty());
    }

    #[test]
    fn agent_presets_count_is_twenty_one() {
        assert_eq!(AGENT_PRESETS.len(), 21);
    }
}
