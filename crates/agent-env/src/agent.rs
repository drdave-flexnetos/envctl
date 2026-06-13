//! Per-preset native path mapping for the 21-preset [`Agent`] enum, plus the MCP /
//! command destination format+target value types — ported verbatim from kasetto v3.2.0
//! `src/model/agent.rs` and `src/model/mod.rs` (the `McpSettings*` / `Command*` types).
//!
//! Ledger rows: M-11 (`global_path`), M-12 (`project_path`), M-13
//! (`mcp_settings_target`), M-14 (`mcp_project_target`), M-15 (`commands_global_path`),
//! M-16 (`commands_project_path`), M-17 (`all_mcp_*_targets`), M-18 (`all_command_*_targets`),
//! M-19 (`command_*_targets`), M-20 (private helpers), M-26 (`McpSettingsFormat` /
//! `McpSettingsTarget`), M-27 (`CommandFormat` / `CommandTarget`).
//!
//! NAMING (envctl, not kasetto): kasetto's `continue` preset writes its OWN merge marker
//! file `.continue/mcpServers/kasetto.json` — this is kasetto's self-named drop file inside
//! the agent-native `.continue/mcpServers/` directory, not an agent-native path. It is kept
//! VERBATIM here so the absorbed behavior matches byte-for-byte; TASK-0013's Engine wiring
//! (the layer that actually owns the product identity) reconciles the file name. The agent's
//! real config locations (`.claude/skills`, `.codex/config.toml`, the VS Code user
//! `mcp.json`, …) are agent-native and are NEVER renamed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::{Agent, AGENT_PRESETS};

/// How agent configs merge pack `mcpServers` into a native config file.
///
/// Ported from kasetto v3.2.0 `src/model/mod.rs` (ledger M-26).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum McpSettingsFormat {
    /// `{ "mcpServers": { ... } }` (Claude, Cursor, Gemini CLI, Roo, Cline, etc.).
    McpServers,
    /// VS Code / GitHub Copilot user `mcp.json`: `{ "servers": { ... } }`.
    VsCodeServers,
    /// OpenCode `opencode.json`: `{ "mcp": { "name": { "type": "local"|"remote", ... } } }`.
    OpenCode,
    /// OpenAI Codex `~/.codex/config.toml` (`[mcp_servers.name]` tables).
    CodexToml,
}

/// Destination file and merge format for MCP sync / clean (ledger M-26).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpSettingsTarget {
    /// Absolute (or scope-relative) path of the native MCP config file.
    pub path: PathBuf,
    /// The merge format to apply at that path.
    pub format: McpSettingsFormat,
}

/// On-disk shape emitted for a command on a given agent.
///
/// Ported from kasetto v3.2.0 `src/model/mod.rs` (ledger M-27).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandFormat {
    /// Verbatim Markdown with YAML frontmatter (Claude Code style).
    MarkdownFrontmatter,
    /// Markdown body only — frontmatter stripped.
    MarkdownPlain,
    /// `<name>.prompt.md` — frontmatter preserved (GitHub Copilot).
    PromptMd,
    /// `<name>.prompt` (Continue Dev) — frontmatter preserved, `invokable: true` injected.
    PromptFile,
    /// `<name>.toml` (Gemini CLI custom commands).
    GeminiToml,
}

/// Destination directory and write format for command sync / clean (ledger M-27).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandTarget {
    /// Absolute (or scope-relative) path of the native command directory.
    pub path: PathBuf,
    /// The write format to apply in that directory.
    pub format: CommandFormat,
}

/// Deduped native MCP config files for every known agent (for `clean` manifest wipe).
///
/// Ledger M-17. The `kasetto_config` arg is threaded through verbatim (kasetto reserves it
/// for a future per-config override); it is currently unused by every preset.
pub fn all_mcp_settings_targets(home: &Path, kasetto_config: &Path) -> Vec<McpSettingsTarget> {
    dedup_targets(
        AGENT_PRESETS
            .iter()
            .map(|a| a.mcp_settings_target(home, kasetto_config)),
    )
}

/// Deduped project-level MCP config files for every known agent (for `clean` in project scope).
///
/// Ledger M-17.
pub fn all_mcp_project_targets(project_root: &Path) -> Vec<McpSettingsTarget> {
    dedup_targets(
        AGENT_PRESETS
            .iter()
            .map(|a| a.mcp_project_target(project_root)),
    )
}

/// HashSet path-dedup + sort for MCP targets (ledger M-20).
fn dedup_targets(iter: impl Iterator<Item = McpSettingsTarget>) -> Vec<McpSettingsTarget> {
    let mut seen = HashSet::<PathBuf>::new();
    let mut out: Vec<McpSettingsTarget> = iter.filter(|t| seen.insert(t.path.clone())).collect();
    out.sort_by(|x, y| x.path.cmp(&y.path));
    out
}

/// Flatten `Option`, HashSet path-dedup + sort for command targets (ledger M-20).
fn dedup_command_targets(iter: impl Iterator<Item = Option<CommandTarget>>) -> Vec<CommandTarget> {
    let mut seen = HashSet::<PathBuf>::new();
    let mut out: Vec<CommandTarget> = iter
        .flatten()
        .filter(|t| seen.insert(t.path.clone()))
        .collect();
    out.sort_by(|x, y| x.path.cmp(&y.path));
    out
}

/// Deduped global command directories for every known agent (ledger M-18).
pub fn all_command_global_targets(home: &Path) -> Vec<CommandTarget> {
    dedup_command_targets(AGENT_PRESETS.iter().map(|a| a.commands_global_path(home)))
}

/// Deduped project-level command directories for every known agent (ledger M-18).
pub fn all_command_project_targets(project_root: &Path) -> Vec<CommandTarget> {
    dedup_command_targets(
        AGENT_PRESETS
            .iter()
            .map(|a| a.commands_project_path(project_root)),
    )
}

/// Deduped global command directories for a specific set of agents — used by `doctor` to
/// scope the COMMAND DIRECTORIES panel to what the config wires (ledger M-19).
pub fn command_global_targets(home: &Path, agents: &[Agent]) -> Vec<CommandTarget> {
    dedup_command_targets(agents.iter().map(|a| a.commands_global_path(home)))
}

/// Deduped project command directories for a specific set of agents (ledger M-19).
pub fn command_project_targets(project_root: &Path, agents: &[Agent]) -> Vec<CommandTarget> {
    dedup_command_targets(agents.iter().map(|a| a.commands_project_path(project_root)))
}

/// Build a supported [`CommandTarget`] at `base/rel` (ledger M-20 helper `cmd`).
#[inline]
fn cmd(base: &Path, rel: &str, format: CommandFormat) -> Option<CommandTarget> {
    Some(CommandTarget {
        path: base.join(rel),
        format,
    })
}

/// VS Code / Copilot user-profile `mcp.json` (not Insiders) — OS-branched (ledger M-20).
fn vscode_user_mcp_json(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Code/User/mcp.json")
    } else if cfg!(target_os = "windows") {
        let base = std::env::var("APPDATA").unwrap_or_default();
        PathBuf::from(base).join("Code/User/mcp.json")
    } else {
        home.join(".config/Code/User/mcp.json")
    }
}

/// Build an `McpServers`-format [`McpSettingsTarget`] at `base/rel` (ledger M-20 helper).
#[inline]
fn mcp_servers_target(base: &Path, rel: &str) -> McpSettingsTarget {
    McpSettingsTarget {
        path: base.join(rel),
        format: McpSettingsFormat::McpServers,
    }
}

impl Agent {
    /// Per-preset GLOBAL skills directory, relative to `home` (ledger M-11).
    pub fn global_path(self, home: &Path) -> PathBuf {
        match self {
            Agent::Amp | Agent::Replit => home.join(".config/agents/skills"),
            Agent::Antigravity => home.join(".gemini/antigravity/skills"),
            Agent::Augment => home.join(".augment/skills"),
            Agent::ClaudeCode => home.join(".claude/skills"),
            Agent::Cline | Agent::Warp => home.join(".agents/skills"),
            Agent::Codex => home.join(".codex/skills"),
            Agent::Continue => home.join(".continue/skills"),
            Agent::Cursor => home.join(".cursor/skills"),
            Agent::GeminiCli => home.join(".gemini/skills"),
            Agent::GithubCopilot => home.join(".copilot/skills"),
            Agent::Goose => home.join(".config/goose/skills"),
            Agent::Junie => home.join(".junie/skills"),
            Agent::KiroCli => home.join(".kiro/skills"),
            Agent::OpenClaw => home.join(".openclaw/skills"),
            Agent::OpenCode => home.join(".config/opencode/skills"),
            Agent::OpenHands => home.join(".openhands/skills"),
            Agent::Roo => home.join(".roo/skills"),
            Agent::Trae => home.join(".trae/skills"),
            Agent::Windsurf => home.join(".codeium/windsurf/skills"),
        }
    }

    /// Native GLOBAL MCP config location and merge format for this agent (ledger M-13).
    pub fn mcp_settings_target(self, home: &Path, _kasetto_config: &Path) -> McpSettingsTarget {
        match self {
            Agent::ClaudeCode => mcp_servers_target(home, ".claude.json"),
            Agent::Cursor => mcp_servers_target(home, ".cursor/mcp.json"),
            Agent::GithubCopilot => McpSettingsTarget {
                path: vscode_user_mcp_json(home),
                format: McpSettingsFormat::VsCodeServers,
            },
            Agent::GeminiCli => mcp_servers_target(home, ".gemini/settings.json"),
            Agent::Roo => mcp_servers_target(home, ".roo/mcp_settings.json"),
            Agent::Windsurf => mcp_servers_target(home, ".codeium/windsurf/mcp_config.json"),
            Agent::Cline => {
                mcp_servers_target(home, ".cline/data/settings/cline_mcp_settings.json")
            }
            Agent::Continue => mcp_servers_target(home, ".continue/mcpServers/kasetto.json"),
            Agent::Amp | Agent::Replit => mcp_servers_target(home, ".config/agents/mcp.json"),
            Agent::Antigravity => mcp_servers_target(home, ".gemini/antigravity/mcp.json"),
            Agent::Augment => mcp_servers_target(home, ".augment/mcp.json"),
            Agent::Warp => mcp_servers_target(home, ".warp/mcp.json"),
            Agent::Codex => McpSettingsTarget {
                path: home.join(".codex/config.toml"),
                format: McpSettingsFormat::CodexToml,
            },
            Agent::Goose => mcp_servers_target(home, ".config/goose/mcp.json"),
            Agent::Junie => mcp_servers_target(home, ".junie/mcp.json"),
            Agent::KiroCli => mcp_servers_target(home, ".kiro/mcp.json"),
            Agent::OpenClaw => mcp_servers_target(home, ".openclaw/mcp.json"),
            Agent::OpenCode => McpSettingsTarget {
                path: home.join(".config/opencode/opencode.json"),
                format: McpSettingsFormat::OpenCode,
            },
            Agent::OpenHands => mcp_servers_target(home, ".openhands/mcp.json"),
            Agent::Trae => mcp_servers_target(home, ".trae/mcp.json"),
        }
    }

    /// Per-preset PROJECT-local skills directory, relative to `project_root` (ledger M-12).
    pub fn project_path(self, project_root: &Path) -> PathBuf {
        match self {
            Agent::Amp | Agent::Replit => project_root.join(".agents/skills"),
            Agent::Antigravity => project_root.join(".gemini/antigravity/skills"),
            Agent::Augment => project_root.join(".augment/skills"),
            Agent::ClaudeCode => project_root.join(".claude/skills"),
            Agent::Cline | Agent::Warp => project_root.join(".agents/skills"),
            Agent::Codex => project_root.join(".codex/skills"),
            Agent::Continue => project_root.join(".continue/skills"),
            Agent::Cursor => project_root.join(".cursor/skills"),
            Agent::GeminiCli => project_root.join(".gemini/skills"),
            Agent::GithubCopilot => project_root.join(".copilot/skills"),
            Agent::Goose => project_root.join(".goose/skills"),
            Agent::Junie => project_root.join(".junie/skills"),
            Agent::KiroCli => project_root.join(".kiro/skills"),
            Agent::OpenClaw => project_root.join(".openclaw/skills"),
            Agent::OpenCode => project_root.join(".opencode/skills"),
            Agent::OpenHands => project_root.join(".openhands/skills"),
            Agent::Roo => project_root.join(".roo/skills"),
            Agent::Trae => project_root.join(".trae/skills"),
            Agent::Windsurf => project_root.join(".windsurf/skills"),
        }
    }

    /// Project-local MCP config location and merge format for this agent (ledger M-14).
    pub fn mcp_project_target(self, project_root: &Path) -> McpSettingsTarget {
        match self {
            Agent::ClaudeCode => McpSettingsTarget {
                path: project_root.join(".mcp.json"),
                format: McpSettingsFormat::McpServers,
            },
            Agent::Cursor => mcp_servers_target(project_root, ".cursor/mcp.json"),
            Agent::GithubCopilot => McpSettingsTarget {
                path: project_root.join(".vscode/mcp.json"),
                format: McpSettingsFormat::VsCodeServers,
            },
            Agent::GeminiCli => mcp_servers_target(project_root, ".gemini/settings.json"),
            Agent::Roo => mcp_servers_target(project_root, ".roo/mcp.json"),
            Agent::Windsurf => mcp_servers_target(project_root, ".windsurf/mcp.json"),
            Agent::Cline => mcp_servers_target(project_root, ".cline_mcp_servers.json"),
            Agent::Continue => {
                mcp_servers_target(project_root, ".continue/mcpServers/kasetto.json")
            }
            Agent::Codex => McpSettingsTarget {
                path: project_root.join(".codex/config.toml"),
                format: McpSettingsFormat::CodexToml,
            },
            Agent::Amp => mcp_servers_target(project_root, ".amp/mcp.json"),
            Agent::Trae => mcp_servers_target(project_root, ".trae/mcp.json"),
            Agent::Junie => mcp_servers_target(project_root, ".junie/mcp/mcp.json"),
            Agent::KiroCli => mcp_servers_target(project_root, ".kiro/settings/mcp.json"),
            Agent::OpenCode => McpSettingsTarget {
                path: project_root.join(".opencode/opencode.json"),
                format: McpSettingsFormat::OpenCode,
            },
            Agent::Antigravity
            | Agent::Augment
            | Agent::Goose
            | Agent::OpenClaw
            | Agent::OpenHands
            | Agent::Replit
            | Agent::Warp => mcp_servers_target(project_root, ".mcp.json"),
        }
    }

    /// Global commands directory and write format for this agent, if supported (ledger M-15).
    ///
    /// `None` = the agent has no global custom-command surface (12 presets).
    pub fn commands_global_path(self, home: &Path) -> Option<CommandTarget> {
        match self {
            Agent::ClaudeCode => cmd(home, ".claude/commands", CommandFormat::MarkdownFrontmatter),
            Agent::Windsurf => cmd(
                home,
                ".codeium/windsurf/global_workflows",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::OpenCode => cmd(
                home,
                ".config/opencode/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::Continue => cmd(home, ".continue/prompts", CommandFormat::PromptFile),
            Agent::Amp => cmd(
                home,
                ".config/amp/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::Augment => cmd(
                home,
                ".augment/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::Roo => cmd(home, ".roo/commands", CommandFormat::MarkdownFrontmatter),
            Agent::Codex => cmd(home, ".codex/prompts", CommandFormat::MarkdownFrontmatter),
            Agent::GeminiCli => cmd(home, ".gemini/commands", CommandFormat::GeminiToml),
            Agent::Cursor
            | Agent::Cline
            | Agent::GithubCopilot
            | Agent::Junie
            | Agent::OpenHands
            | Agent::Antigravity
            | Agent::Goose
            | Agent::KiroCli
            | Agent::OpenClaw
            | Agent::Replit
            | Agent::Trae
            | Agent::Warp => None,
        }
    }

    /// Project-local commands directory and write format for this agent, if supported
    /// (ledger M-16). `None` = the agent has no project custom-command surface (8 presets).
    pub fn commands_project_path(self, project_root: &Path) -> Option<CommandTarget> {
        match self {
            Agent::ClaudeCode => cmd(
                project_root,
                ".claude/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::Cursor => cmd(
                project_root,
                ".cursor/commands",
                CommandFormat::MarkdownPlain,
            ),
            Agent::Windsurf => cmd(
                project_root,
                ".windsurf/workflows",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::Cline => cmd(
                project_root,
                ".clinerules/workflows",
                CommandFormat::MarkdownPlain,
            ),
            Agent::OpenCode => cmd(
                project_root,
                ".opencode/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::Continue => cmd(project_root, ".continue/prompts", CommandFormat::PromptFile),
            Agent::GithubCopilot => cmd(project_root, ".github/prompts", CommandFormat::PromptMd),
            Agent::Amp => cmd(
                project_root,
                ".agents/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::Augment => cmd(
                project_root,
                ".augment/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::Roo => cmd(
                project_root,
                ".roo/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::Junie => cmd(
                project_root,
                ".junie/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::OpenHands => cmd(
                project_root,
                ".openhands/microagents",
                CommandFormat::MarkdownFrontmatter,
            ),
            Agent::GeminiCli => cmd(project_root, ".gemini/commands", CommandFormat::GeminiToml),
            Agent::Antigravity
            | Agent::Codex
            | Agent::Goose
            | Agent::KiroCli
            | Agent::OpenClaw
            | Agent::Replit
            | Agent::Trae
            | Agent::Warp => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Ported verbatim from kasetto v3.2.0 src/model/agent.rs `mod tests` ---

    #[test]
    fn commands_global_path_known_agents() {
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
    }

    #[test]
    fn commands_project_path_known_agents() {
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
    }

    #[test]
    fn all_command_global_targets_dedupes_and_sorts() {
        let home = Path::new("/tmp/home");
        let all = all_command_global_targets(home);
        assert!(!all.is_empty());
        for w in all.windows(2) {
            assert!(w[0].path <= w[1].path);
        }
    }

    // --- Added all-21-preset coverage (kasetto's tests are thinner; parity start) ---

    /// kasetto-config arg is reserved/unused; pass a dummy through every preset.
    fn kcfg() -> &'static Path {
        Path::new("/tmp/kasetto.yaml")
    }

    #[test]
    fn global_path_every_preset_exact() {
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

    #[test]
    fn project_path_every_preset_exact() {
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

    #[test]
    fn mcp_settings_target_every_preset_exact() {
        let h = Path::new("/h");
        // (agent, relative path, format) — github-copilot is OS-branched, tested separately.
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
            let t = a.mcp_settings_target(h, kcfg());
            assert_eq!(t.path, h.join(rel), "mcp_settings_target path {a:?}");
            assert_eq!(t.format, *fmt, "mcp_settings_target format {a:?}");
        }
        // github-copilot OS branch (Linux on the CI host).
        let copilot = Agent::GithubCopilot.mcp_settings_target(h, kcfg());
        assert_eq!(copilot.format, McpSettingsFormat::VsCodeServers);
        assert_eq!(copilot.path, vscode_user_mcp_json(h));
    }

    #[test]
    fn mcp_project_target_every_preset_exact() {
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
            // Fallthrough presets → ".mcp.json" McpServers.
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

    #[test]
    fn commands_global_path_full_supported_set() {
        let h = Path::new("/h");
        let supported: &[(Agent, &str, CommandFormat)] = &[
            (
                Agent::ClaudeCode,
                ".claude/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Windsurf,
                ".codeium/windsurf/global_workflows",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::OpenCode,
                ".config/opencode/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Continue,
                ".continue/prompts",
                CommandFormat::PromptFile,
            ),
            (
                Agent::Amp,
                ".config/amp/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Augment,
                ".augment/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Roo,
                ".roo/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Codex,
                ".codex/prompts",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::GeminiCli,
                ".gemini/commands",
                CommandFormat::GeminiToml,
            ),
        ];
        for (a, rel, fmt) in supported {
            let t = a.commands_global_path(h).expect("supported");
            assert_eq!(t.path, h.join(rel), "commands_global_path {a:?}");
            assert_eq!(t.format, *fmt, "commands_global_path fmt {a:?}");
        }
        let unsupported = [
            Agent::Cursor,
            Agent::Cline,
            Agent::GithubCopilot,
            Agent::Junie,
            Agent::OpenHands,
            Agent::Antigravity,
            Agent::Goose,
            Agent::KiroCli,
            Agent::OpenClaw,
            Agent::Replit,
            Agent::Trae,
            Agent::Warp,
        ];
        assert_eq!(unsupported.len(), 12);
        for a in unsupported {
            assert!(a.commands_global_path(h).is_none(), "{a:?} should be None");
        }
        assert_eq!(supported.len() + unsupported.len(), AGENT_PRESETS.len());
    }

    #[test]
    fn commands_project_path_full_supported_set() {
        let p = Path::new("/p");
        let supported: &[(Agent, &str, CommandFormat)] = &[
            (
                Agent::ClaudeCode,
                ".claude/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Cursor,
                ".cursor/commands",
                CommandFormat::MarkdownPlain,
            ),
            (
                Agent::Windsurf,
                ".windsurf/workflows",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Cline,
                ".clinerules/workflows",
                CommandFormat::MarkdownPlain,
            ),
            (
                Agent::OpenCode,
                ".opencode/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Continue,
                ".continue/prompts",
                CommandFormat::PromptFile,
            ),
            (
                Agent::GithubCopilot,
                ".github/prompts",
                CommandFormat::PromptMd,
            ),
            (
                Agent::Amp,
                ".agents/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Augment,
                ".augment/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Roo,
                ".roo/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::Junie,
                ".junie/commands",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::OpenHands,
                ".openhands/microagents",
                CommandFormat::MarkdownFrontmatter,
            ),
            (
                Agent::GeminiCli,
                ".gemini/commands",
                CommandFormat::GeminiToml,
            ),
        ];
        for (a, rel, fmt) in supported {
            let t = a.commands_project_path(p).expect("supported");
            assert_eq!(t.path, p.join(rel), "commands_project_path {a:?}");
            assert_eq!(t.format, *fmt, "commands_project_path fmt {a:?}");
        }
        let unsupported = [
            Agent::Antigravity,
            Agent::Codex,
            Agent::Goose,
            Agent::KiroCli,
            Agent::OpenClaw,
            Agent::Replit,
            Agent::Trae,
            Agent::Warp,
        ];
        assert_eq!(unsupported.len(), 8);
        for a in unsupported {
            assert!(a.commands_project_path(p).is_none(), "{a:?} should be None");
        }
        assert_eq!(supported.len() + unsupported.len(), AGENT_PRESETS.len());
    }

    #[test]
    fn all_mcp_settings_targets_dedupes_and_sorts() {
        let h = Path::new("/h");
        let all = all_mcp_settings_targets(h, kcfg());
        assert!(!all.is_empty());
        // amp|replit collapse to one ".config/agents/mcp.json"; so do other shared paths.
        for w in all.windows(2) {
            assert!(w[0].path < w[1].path, "must be strictly sorted + deduped");
        }
    }

    #[test]
    fn all_mcp_project_targets_dedupes_and_sorts() {
        let p = Path::new("/p");
        let all = all_mcp_project_targets(p);
        assert!(!all.is_empty());
        // The 7 fallthrough presets all collapse to a single ".mcp.json".
        let mcp_json = p.join(".mcp.json");
        assert_eq!(all.iter().filter(|t| t.path == mcp_json).count(), 1);
        for w in all.windows(2) {
            assert!(w[0].path < w[1].path);
        }
    }

    #[test]
    fn all_command_targets_dedupe_strictly() {
        let h = Path::new("/h");
        let p = Path::new("/p");
        for all in [
            all_command_global_targets(h),
            all_command_project_targets(p),
        ] {
            assert!(!all.is_empty());
            for w in all.windows(2) {
                assert!(w[0].path < w[1].path);
            }
        }
    }

    #[test]
    fn scoped_command_targets_filter_to_given_agents() {
        let h = Path::new("/h");
        let p = Path::new("/p");
        // Cursor has no global command surface but a project one.
        let g = command_global_targets(h, &[Agent::Cursor, Agent::ClaudeCode]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].path, h.join(".claude/commands"));
        let pr = command_project_targets(p, &[Agent::Cursor, Agent::ClaudeCode]);
        assert_eq!(pr.len(), 2);
    }

    #[test]
    fn empty_agent_set_yields_empty_scoped_targets() {
        let h = Path::new("/h");
        let p = Path::new("/p");
        assert!(command_global_targets(h, &[]).is_empty());
        assert!(command_project_targets(p, &[]).is_empty());
    }
}
