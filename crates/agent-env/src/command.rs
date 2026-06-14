//! Command (slash-command / prompt template) parsing and per-agent transforms.
//!
//! Ported verbatim from kasetto v3.2.0 `src/prompts/{mod,parse,transform}.rs` with NO
//! capability downgrade (see `.handoff/decisions/ADR-0001`). This module owns:
//! - [`parse`] — split Markdown-with-YAML-frontmatter into ([`Parsed`]) frontmatter/body,
//!   CRLF-normalizing first; an opening `---` with no closing `---` is a fail-closed error.
//! - [`render`] — the **five** command-format transforms ([`CommandFormat`]):
//!   `MarkdownFrontmatter`, `MarkdownPlain`, `PromptMd`, `PromptFile`
//!   (`$ARGUMENTS`→`{{{ input }}}`, `invokable: true` injected), and `GeminiToml`.
//! - [`destination_path`] — the format-derived relative filename under a [`CommandTarget`]
//!   (namespaced `:` → nested subdirs for the Markdown-shape formats; flattened with `-`
//!   for the plain/prompt/toml formats).
//! - [`apply_command`] — the driver: read a source `.md`, parse, render, and write the
//!   transformed output to the format-derived destination, creating parent dirs.
//!
//! NAMING: `description` extraction reads the frontmatter via `serde_yaml`. Error mapping
//! mirrors the other modules — kasetto's string-message `err(...)` lands on
//! [`crate::AgentEnvError::Message`].

use std::fs;
use std::path::{Path, PathBuf};

use crate::agent::{CommandFormat, CommandTarget};
use crate::{err, Result};

// ===========================================================================
// parse — ported from kasetto v3.2.0 `src/prompts/parse.rs`
// ===========================================================================

/// A parsed Markdown-with-YAML-frontmatter command source.
#[derive(Debug, Clone)]
pub struct Parsed {
    /// Frontmatter YAML text (between the `---` fences), without the fences.
    /// `None` if the source had no frontmatter.
    pub frontmatter: Option<String>,
    /// Body content after the closing `---` fence (or the whole file if none).
    pub body: String,
}

impl Parsed {
    /// Extract the `description` field from the frontmatter YAML, if present.
    pub fn description(&self) -> Option<String> {
        let fm = self.frontmatter.as_deref()?;
        let value: serde_yaml::Value = serde_yaml::from_str(fm).ok()?;
        value
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

/// Split a Markdown file into (frontmatter, body).
///
/// Frontmatter is recognized only when the file starts with `---` on its own
/// line and a matching closing `---` line is present.
pub fn parse(text: &str) -> Result<Parsed> {
    let normalized = text.replace("\r\n", "\n");
    let stripped = normalized.strip_prefix("---\n");
    let Some(rest) = stripped else {
        return Ok(Parsed {
            frontmatter: None,
            body: normalized,
        });
    };
    // Find a line that is exactly "---" or "---\n" inside rest.
    let mut idx = 0usize;
    let bytes = rest.as_bytes();
    let mut found: Option<(usize, usize)> = None;
    while idx < bytes.len() {
        let line_end = rest[idx..]
            .find('\n')
            .map(|n| idx + n)
            .unwrap_or(bytes.len());
        let line = &rest[idx..line_end];
        if line == "---" {
            // Frontmatter ends at idx; body starts after line_end + 1 (skip newline).
            let body_start = (line_end + 1).min(bytes.len());
            found = Some((idx, body_start));
            break;
        }
        idx = line_end + 1;
    }
    let Some((fm_end, body_start)) = found else {
        return Err(err(
            "command source has an opening `---` but no closing `---` for the frontmatter",
        ));
    };
    let frontmatter = rest[..fm_end].trim_end_matches('\n').to_string();
    let body = rest[body_start..].to_string();
    Ok(Parsed {
        frontmatter: Some(frontmatter),
        body,
    })
}

// ===========================================================================
// transform — ported from kasetto v3.2.0 `src/prompts/transform.rs`
// ===========================================================================

/// Returns the on-disk relative filename for a command, given its name and format.
///
/// Namespaced names with `:` map to nested subdirectories for formats that keep
/// the original Markdown shape. Plain formats flatten namespaces with `-`.
fn derive_relpath(name: &str, format: CommandFormat) -> PathBuf {
    match format {
        CommandFormat::MarkdownFrontmatter => name_to_nested_path(name, "md"),
        CommandFormat::MarkdownPlain => PathBuf::from(format!("{}.md", flatten_name(name))),
        CommandFormat::PromptMd => PathBuf::from(format!("{}.prompt.md", flatten_name(name))),
        CommandFormat::PromptFile => PathBuf::from(format!("{}.prompt", flatten_name(name))),
        CommandFormat::GeminiToml => PathBuf::from(format!("{}.toml", flatten_name(name))),
    }
}

fn name_to_nested_path(name: &str, ext: &str) -> PathBuf {
    let mut parts: Vec<&str> = name.split(':').filter(|p| !p.is_empty()).collect();
    let Some(last) = parts.pop() else {
        return PathBuf::from(format!("command.{ext}"));
    };
    let mut path = PathBuf::new();
    for p in parts {
        path.push(p);
    }
    path.push(format!("{last}.{ext}"));
    path
}

fn flatten_name(name: &str) -> String {
    name.replace(':', "-")
}

/// Render `parsed` to bytes for the given `format`.
pub fn render(parsed: &Parsed, format: CommandFormat) -> String {
    match format {
        CommandFormat::MarkdownFrontmatter | CommandFormat::PromptMd => {
            if let Some(fm) = &parsed.frontmatter {
                format!("---\n{}\n---\n{}", fm, parsed.body)
            } else {
                parsed.body.clone()
            }
        }
        CommandFormat::MarkdownPlain => parsed.body.clone(),
        CommandFormat::PromptFile => render_prompt_file(parsed),
        CommandFormat::GeminiToml => render_gemini_toml(parsed),
    }
}

fn render_prompt_file(parsed: &Parsed) -> String {
    // Continue Dev `.prompt` files use a YAML preamble between `---` fences with `invokable: true`.
    let body = parsed.body.replace("$ARGUMENTS", "{{{ input }}}");
    let mut preamble: Vec<String> = Vec::new();
    if let Some(fm) = &parsed.frontmatter {
        for line in fm.lines() {
            if line.trim().is_empty() {
                continue;
            }
            preamble.push(line.to_string());
        }
    }
    let has_invokable = preamble
        .iter()
        .any(|line| line.trim_start().starts_with("invokable:"));
    if !has_invokable {
        preamble.push("invokable: true".to_string());
    }
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&preamble.join("\n"));
    out.push('\n');
    out.push_str("---\n");
    out.push_str(&body);
    out
}

fn render_gemini_toml(parsed: &Parsed) -> String {
    let description = parsed.description().unwrap_or_default();
    let body = parsed.body.trim_end_matches('\n').to_string();
    let mut out = String::new();
    if !description.is_empty() {
        out.push_str(&format!("description = {}\n", toml_string(&description)));
    }
    out.push_str("prompt = \"\"\"\n");
    out.push_str(&body);
    out.push_str("\n\"\"\"\n");
    out
}

fn toml_string(s: &str) -> String {
    // Basic TOML string escape — sufficient for description one-liners.
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// Resolve where this command should be written under `target.path` and return the absolute path.
pub fn destination_path(target: &CommandTarget, name: &str) -> PathBuf {
    target.path.join(derive_relpath(name, target.format))
}

/// Ensure parent directories of `path` exist.
pub fn ensure_parent_dirs(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

// ===========================================================================
// driver — ported from kasetto v3.2.0 `src/prompts/mod.rs`
// ===========================================================================

/// Read a Markdown command file at `source`, parse it, and write the transformed
/// output into `target.path` under the format-derived relative filename.
///
/// Returns the absolute path of the written file.
pub fn apply_command(source: &Path, target: &CommandTarget, name: &str) -> Result<PathBuf> {
    let text = fs::read_to_string(source).map_err(|e| {
        err(format!(
            "failed to read command source {}: {e}",
            source.display()
        ))
    })?;
    let parsed = parse(&text)?;
    let rendered = render(&parsed, target.format);
    let dest = destination_path(target, name);
    ensure_parent_dirs(&dest)?;
    fs::write(&dest, rendered)
        .map_err(|e| err(format!("failed to write command {}: {e}", dest.display())))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-test unique temp dir (mirrors the helper used by the other modules' tests).
    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    // --- Ported verbatim from kasetto v3.2.0 src/prompts/parse.rs `mod tests` ---

    #[test]
    fn parses_frontmatter_and_body() {
        let text = "---\ndescription: hi\nargument-hint: <n>\n---\nBody here.\n";
        let p = parse(text).unwrap();
        assert!(p
            .frontmatter
            .as_deref()
            .unwrap()
            .contains("description: hi"));
        assert_eq!(p.body, "Body here.\n");
        assert_eq!(p.description().as_deref(), Some("hi"));
    }

    #[test]
    fn no_frontmatter_means_whole_body() {
        let p = parse("just markdown\n").unwrap();
        assert!(p.frontmatter.is_none());
        assert_eq!(p.body, "just markdown\n");
    }

    #[test]
    fn missing_closing_fence_is_error() {
        let text = "---\ndescription: nope\nBody never closed.\n";
        assert!(parse(text).is_err());
    }

    // --- Ported verbatim from kasetto v3.2.0 src/prompts/transform.rs `mod tests` ---

    fn sample() -> Parsed {
        parse("---\ndescription: do thing\nargument-hint: <n>\n---\nUse $ARGUMENTS here.\n")
            .unwrap()
    }

    #[test]
    fn nested_paths_for_markdown_frontmatter() {
        let p = derive_relpath("git:commit", CommandFormat::MarkdownFrontmatter);
        assert_eq!(p, PathBuf::from("git/commit.md"));
        let p2 = derive_relpath("commit", CommandFormat::MarkdownFrontmatter);
        assert_eq!(p2, PathBuf::from("commit.md"));
    }

    #[test]
    fn flat_names_for_other_formats() {
        assert_eq!(
            derive_relpath("git:commit", CommandFormat::MarkdownPlain),
            PathBuf::from("git-commit.md")
        );
        assert_eq!(
            derive_relpath("git:commit", CommandFormat::PromptMd),
            PathBuf::from("git-commit.prompt.md")
        );
        assert_eq!(
            derive_relpath("git:commit", CommandFormat::PromptFile),
            PathBuf::from("git-commit.prompt")
        );
        assert_eq!(
            derive_relpath("git:commit", CommandFormat::GeminiToml),
            PathBuf::from("git-commit.toml")
        );
    }

    #[test]
    fn markdown_frontmatter_round_trip() {
        let p = sample();
        let rendered = render(&p, CommandFormat::MarkdownFrontmatter);
        assert!(rendered.starts_with("---\n"));
        assert!(rendered.contains("description: do thing"));
        assert!(rendered.contains("Use $ARGUMENTS here."));
    }

    #[test]
    fn markdown_plain_strips_frontmatter() {
        let p = sample();
        let rendered = render(&p, CommandFormat::MarkdownPlain);
        assert!(!rendered.contains("description:"));
        assert!(rendered.contains("Use $ARGUMENTS here."));
    }

    #[test]
    fn prompt_md_preserves_frontmatter() {
        let p = sample();
        let rendered = render(&p, CommandFormat::PromptMd);
        assert!(rendered.starts_with("---\n"));
        assert!(rendered.contains("description: do thing"));
    }

    #[test]
    fn prompt_file_injects_invokable_and_rewrites_arguments() {
        let p = sample();
        let rendered = render(&p, CommandFormat::PromptFile);
        assert!(rendered.contains("invokable: true"));
        assert!(rendered.contains("{{{ input }}}"));
        assert!(!rendered.contains("$ARGUMENTS"));
    }

    #[test]
    fn prompt_file_does_not_double_invokable() {
        let parsed = parse("---\ninvokable: false\n---\nx\n").unwrap();
        let rendered = render(&parsed, CommandFormat::PromptFile);
        let count = rendered.matches("invokable:").count();
        assert_eq!(count, 1);
        assert!(rendered.contains("invokable: false"));
    }

    #[test]
    fn gemini_toml_emits_description_and_prompt() {
        let p = sample();
        let rendered = render(&p, CommandFormat::GeminiToml);
        assert!(rendered.contains("description = \"do thing\""));
        assert!(rendered.contains("prompt = \"\"\""));
        assert!(rendered.contains("Use $ARGUMENTS here."));
    }

    // --- Ported verbatim from kasetto v3.2.0 src/prompts/mod.rs `mod tests` ---
    // (`crate::fsops::temp_dir` adapted to the local helper above.)

    #[test]
    fn apply_command_writes_nested_markdown() {
        let src_dir = temp_dir("agent-env-cmd-src");
        fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("commit.md");
        fs::write(&src, "---\ndescription: hi\n---\nbody\n").unwrap();

        let dst_dir = temp_dir("agent-env-cmd-dst");
        let target = CommandTarget {
            path: dst_dir.clone(),
            format: CommandFormat::MarkdownFrontmatter,
        };
        let out = apply_command(&src, &target, "git:commit").unwrap();
        assert!(out.ends_with("git/commit.md"));
        let text = fs::read_to_string(&out).unwrap();
        assert!(text.contains("description: hi"));

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    #[test]
    fn apply_command_writes_gemini_toml() {
        let src_dir = temp_dir("agent-env-cmd-gem");
        fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("deploy.md");
        fs::write(&src, "---\ndescription: ship it\n---\nrun $ARGUMENTS\n").unwrap();

        let dst_dir = temp_dir("agent-env-cmd-gem-dst");
        let target = CommandTarget {
            path: dst_dir.clone(),
            format: CommandFormat::GeminiToml,
        };
        let out = apply_command(&src, &target, "deploy").unwrap();
        assert!(out.ends_with("deploy.toml"));
        let text = fs::read_to_string(&out).unwrap();
        assert!(text.contains("description = \"ship it\""));
        assert!(text.contains("prompt = \"\"\""));

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    // --- Added DUAL-GATE coverage: destination_path + render per all 5 formats ---

    #[test]
    fn destination_path_all_five_formats() {
        let target_for = |format| CommandTarget {
            path: PathBuf::from("/base"),
            format,
        };
        // Markdown-shape (nested for `:`).
        assert_eq!(
            destination_path(
                &target_for(CommandFormat::MarkdownFrontmatter),
                "git:commit"
            ),
            PathBuf::from("/base/git/commit.md")
        );
        // Flat formats.
        assert_eq!(
            destination_path(&target_for(CommandFormat::MarkdownPlain), "git:commit"),
            PathBuf::from("/base/git-commit.md")
        );
        assert_eq!(
            destination_path(&target_for(CommandFormat::PromptMd), "git:commit"),
            PathBuf::from("/base/git-commit.prompt.md")
        );
        assert_eq!(
            destination_path(&target_for(CommandFormat::PromptFile), "git:commit"),
            PathBuf::from("/base/git-commit.prompt")
        );
        assert_eq!(
            destination_path(&target_for(CommandFormat::GeminiToml), "git:commit"),
            PathBuf::from("/base/git-commit.toml")
        );
    }

    #[test]
    fn render_all_five_formats_no_frontmatter() {
        let p = parse("plain body only\n").unwrap();
        // MarkdownFrontmatter / PromptMd fall back to body when no frontmatter.
        assert_eq!(
            render(&p, CommandFormat::MarkdownFrontmatter),
            "plain body only\n"
        );
        assert_eq!(render(&p, CommandFormat::PromptMd), "plain body only\n");
        assert_eq!(
            render(&p, CommandFormat::MarkdownPlain),
            "plain body only\n"
        );
        // PromptFile still injects an invokable preamble even with no frontmatter.
        let pf = render(&p, CommandFormat::PromptFile);
        assert!(pf.starts_with("---\ninvokable: true\n---\n"));
        assert!(pf.contains("plain body only"));
        // GeminiToml: no description line, just the prompt heredoc.
        let gt = render(&p, CommandFormat::GeminiToml);
        assert!(!gt.contains("description ="));
        assert!(gt.contains("prompt = \"\"\""));
        assert!(gt.contains("plain body only"));
    }

    #[test]
    fn gemini_toml_escapes_description_quotes() {
        let p = parse("---\ndescription: say \"hi\"\n---\nbody\n").unwrap();
        let rendered = render(&p, CommandFormat::GeminiToml);
        assert!(rendered.contains("description = \"say \\\"hi\\\"\""));
    }

    #[test]
    fn frontmatter_crlf_is_normalized() {
        let p = parse("---\r\ndescription: hi\r\n---\r\nBody.\r\n").unwrap();
        assert_eq!(p.description().as_deref(), Some("hi"));
        assert_eq!(p.body, "Body.\n");
    }

    #[test]
    fn opening_fence_without_closing_is_error_edge() {
        // Opening `---` present, never closed → fail-closed error (frontmatter edge case).
        assert!(parse("---\ndescription: x\nno closing fence here\n").is_err());
    }
}
