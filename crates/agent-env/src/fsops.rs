//! Filesystem + path/target resolution — ported verbatim from kasetto v3.2.0
//! `src/fsops/{mod,copy,settings}.rs` (ledger F-03..F-10).
//!
//! This is the resolution core that maps a loaded [`Config`] + a [`Scope`] onto the concrete
//! native install paths, plus the recursive directory copier and the JSON settings wrapper.
//! Every helper is non-printing and returns [`crate::Result`]; kasetto's box-error `?`
//! conversions map onto [`crate::AgentEnvError`] (`Io` / `Json` / the `err(...)`
//! [`Message`](crate::AgentEnvError::Message) channel).
//!
//! Ledger rows:
//! - F-03: [`copy_dir`] / [`copy_dir_contents`] / [`copy_file`] (symlink-following, depth-guarded,
//!   permission-preserving directory copy; Windows READONLY strip).
//! - F-04: [`SettingsFile`] (load-or-`{}` / pretty-save JSON wrapper) — a faithful superset of the
//!   parallel PR #73 port, kept self-contained here.
//! - F-05: [`resolve_path`] (leading-`~` home expansion only).
//! - F-06: [`select_targets`] + [`BrokenSkill`] + [`TargetSelection`].
//! - F-07: [`resolve_destinations`] (one skills path per agent, scope-aware).
//! - F-08: [`resolve_mcp_settings_targets`] (one MCP config target per agent, deduped).
//! - F-09: [`resolve_command_targets`] (one command dir per agent, filtered + deduped).
//! - F-10: [`scope_root`] / [`relativize_dest`] / [`resolve_dest`] (the lock-portability core).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent::{CommandTarget, McpSettingsTarget};
use crate::config::{Config, Scope, SkillTarget, SkillsField};
use crate::dirs::{dirs_agent_env_config, dirs_home};
use crate::{err, Result};

// ---------------------------------------------------------------------------
// F-04: SettingsFile (JSON load/mutate/save wrapper)
// ---------------------------------------------------------------------------

/// Wrapper for loading, mutating, and saving agent settings JSON files.
///
/// Ported from kasetto v3.2.0 `src/fsops/settings.rs` (ledger F-04). A faithful superset of
/// the parallel PR #73 port; a later merge reconciles the two to this copy.
pub struct SettingsFile {
    path: PathBuf,
    /// The parsed JSON document; callers mutate this in place before [`SettingsFile::save`].
    pub data: serde_json::Value,
}

impl SettingsFile {
    /// Load an existing JSON file or start with an empty `{}`.
    pub fn load(path: &Path) -> Result<Self> {
        let data = if path.exists() {
            let text = fs::read_to_string(path)?;
            serde_json::from_str(&text)
                .map_err(|e| err(format!("invalid settings JSON {}: {e}", path.display())))?
        } else {
            serde_json::json!({})
        };
        Ok(Self {
            path: path.to_path_buf(),
            data,
        })
    }

    /// Write pretty-printed JSON back to disk, creating parent dirs if needed.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(&self.data)?)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// F-03: recursive directory copy
// ---------------------------------------------------------------------------

/// Depth ceiling for [`copy_dir_contents`] recursion — a symlink cycle would otherwise
/// recurse forever; exceeding this is treated as a cycle and refused (fail-closed).
const MAX_COPY_DEPTH: u32 = 32;

/// Replace `dst` with a fresh recursive copy of `src` (destination removed first).
pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        fs::remove_dir_all(dst)?;
    }
    fs::create_dir_all(dst)?;
    copy_dir_contents(src, dst, 0)
}

/// Recursively copy the entries of `src` into `dst`, following symlinks and guarding against
/// symlink cycles via [`MAX_COPY_DEPTH`].
pub fn copy_dir_contents(src: &Path, dst: &Path, depth: u32) -> Result<()> {
    if depth > MAX_COPY_DEPTH {
        return Err(err(format!(
            "copy depth limit ({MAX_COPY_DEPTH}) exceeded — possible symlink cycle at {}",
            src.display()
        )));
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let target = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            let resolved = fs::canonicalize(&src_path)?;
            let meta = fs::metadata(&resolved)?;
            if meta.is_dir() {
                fs::create_dir_all(&target)?;
                copy_dir_contents(&resolved, &target, depth + 1)?;
            } else {
                copy_file(&resolved, &target)?;
            }
        } else if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_dir_contents(&src_path, &target, depth + 1)?;
        } else {
            copy_file(&src_path, &target)?;
        }
    }
    Ok(())
}

/// Copy a single file, creating parent dirs and (on Windows) stripping a propagated
/// READONLY attribute so later `remove_dir_all` re-syncs do not wedge.
pub fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    // fs::copy uses kernel-level copy where available and preserves
    // permissions, so executable scripts inside skills keep their +x bit.
    fs::copy(src, dst)?;
    // A propagated READONLY attribute would wedge every later re-sync on
    // Windows: remove_dir_all fails with PermissionDenied on read-only files.
    // Unix is unaffected (unlink is governed by the parent dir).
    #[cfg(windows)]
    {
        let mut perms = fs::metadata(dst)?.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            fs::set_permissions(dst, perms)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// F-06: select_targets
// ---------------------------------------------------------------------------

/// The `(resolved targets, broken skills)` pair returned by [`select_targets`].
pub type TargetSelection = (Vec<(String, PathBuf)>, Vec<BrokenSkill>);

/// A skill the config asked for that could not be resolved to a real source directory.
#[derive(Debug)]
pub struct BrokenSkill {
    /// The requested skill name.
    pub name: String,
    /// Why it could not be resolved.
    pub reason: String,
}

/// Resolve a [`SkillsField`] against the discovered `available` map into concrete
/// `(name, path)` targets, collecting unresolved entries as [`BrokenSkill`]s.
///
/// - Wildcard `"*"` → every available skill, sorted by name (HashMap order is random;
///   sorting keeps install order / labels / `--json` output stable across runs).
/// - `List` of names → looked up in `available`, missing ones become broken.
/// - `List` of `{ name, path }` objects → the explicit `path` (absolute, or resolved against
///   `source_root`) is checked for a `SKILL.md`; absent ones become broken; a `path`-less object
///   falls back to the `available` lookup.
/// - Any other wildcard string (`*`-mismatch) is an error (`"invalid skills field"`).
pub fn select_targets(
    sf: &SkillsField,
    available: &HashMap<String, PathBuf>,
    source_root: &Path,
) -> Result<TargetSelection> {
    let mut out = Vec::new();
    let mut broken = Vec::new();
    match sf {
        SkillsField::Wildcard(s) if s == "*" => {
            for (k, v) in available {
                out.push((k.clone(), v.clone()));
            }
            // HashMap iteration order is random; sort so install order, labels,
            // and --json output are stable across runs.
            out.sort_by(|a, b| a.0.cmp(&b.0));
        }
        SkillsField::List(items) => {
            for it in items {
                match it {
                    SkillTarget::Name(name) => {
                        if let Some(p) = available.get(name) {
                            out.push((name.clone(), p.clone()));
                        } else {
                            broken.push(BrokenSkill {
                                name: name.clone(),
                                reason: format!("skill not found: {name}"),
                            });
                        }
                    }
                    SkillTarget::Obj { name, path } => {
                        if let Some(path) = path {
                            let base = PathBuf::from(path);
                            let base = if base.is_absolute() {
                                base
                            } else {
                                source_root.join(base)
                            };
                            let d = base.join(name);
                            if d.join("SKILL.md").exists() {
                                out.push((name.clone(), d));
                                continue;
                            }
                            broken.push(BrokenSkill {
                                name: name.clone(),
                                reason: format!(
                                    "skill not found at `{}`",
                                    base.join(name).display()
                                ),
                            });
                            continue;
                        }
                        if let Some(p) = available.get(name) {
                            out.push((name.clone(), p.clone()));
                        } else {
                            broken.push(BrokenSkill {
                                name: name.clone(),
                                reason: format!("skill not found: {name}"),
                            });
                        }
                    }
                }
            }
        }
        _ => return Err(err("invalid skills field")),
    }
    Ok((out, broken))
}

// ---------------------------------------------------------------------------
// F-05: resolve_path
// ---------------------------------------------------------------------------

/// Resolve `raw` against `base`, expanding **only** a leading `~` (home prefix).
///
/// A `~` elsewhere in the path is an ordinary character (e.g. `./backup~old`) and must not be
/// rewritten. Absolute results are returned as-is; relative ones are joined onto `base`.
pub fn resolve_path(base: &Path, raw: &str) -> PathBuf {
    // Expand only a leading `~` (home prefix); a `~` elsewhere in the path is
    // an ordinary character (e.g. `./backup~old`) and must not be rewritten.
    let p = match raw
        .strip_prefix("~/")
        .or(if raw == "~" { Some("") } else { None })
    {
        Some(rest) => match dirs_home() {
            Ok(home) => home.join(rest),
            Err(_) => PathBuf::from(raw),
        },
        None => PathBuf::from(raw),
    };
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

// ---------------------------------------------------------------------------
// F-07: resolve_destinations
// ---------------------------------------------------------------------------

/// Returns one skills path per configured agent, respecting scope.
/// Falls back to explicit `destination` if set.
pub fn resolve_destinations(base: &Path, cfg: &Config, scope: Scope) -> Result<Vec<PathBuf>> {
    if let Some(destination) = cfg.destination.as_deref() {
        return Ok(vec![resolve_path(base, destination)]);
    }
    let agents = cfg.agents();
    if agents.is_empty() {
        return Err(err(
            "config must define either destination or a supported agent preset",
        ));
    }
    match scope {
        Scope::Project => Ok(agents.iter().map(|a| a.project_path(base)).collect()),
        Scope::Global => {
            let home = dirs_home()?;
            Ok(agents.iter().map(|a| a.global_path(&home)).collect())
        }
    }
}

// ---------------------------------------------------------------------------
// F-08: resolve_mcp_settings_targets
// ---------------------------------------------------------------------------

/// Returns one MCP settings target per configured agent, respecting scope, deduped by path.
pub fn resolve_mcp_settings_targets(
    cfg: &Config,
    scope: Scope,
    project_root: &Path,
) -> Result<Vec<McpSettingsTarget>> {
    let agents = cfg.agents();
    if agents.is_empty() {
        return Ok(vec![]);
    }
    let mut seen = std::collections::HashSet::<PathBuf>::new();
    let mut out = Vec::new();
    match scope {
        Scope::Project => {
            for a in agents {
                let t = a.mcp_project_target(project_root);
                if seen.insert(t.path.clone()) {
                    out.push(t);
                }
            }
        }
        Scope::Global => {
            let home = dirs_home()?;
            let agent_env_config = dirs_agent_env_config()?;
            for a in agents {
                let t = a.mcp_settings_target(&home, &agent_env_config);
                if seen.insert(t.path.clone()) {
                    out.push(t);
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// F-09: resolve_command_targets
// ---------------------------------------------------------------------------

/// Returns one commands directory per configured agent (filtering unsupported), deduped.
pub fn resolve_command_targets(
    cfg: &Config,
    scope: Scope,
    project_root: &Path,
) -> Result<Vec<CommandTarget>> {
    let agents = cfg.agents();
    if agents.is_empty() {
        return Ok(vec![]);
    }
    let mut seen = std::collections::HashSet::<PathBuf>::new();
    let mut out = Vec::new();
    match scope {
        Scope::Project => {
            for a in agents {
                if let Some(t) = a.commands_project_path(project_root) {
                    if seen.insert(t.path.clone()) {
                        out.push(t);
                    }
                }
            }
        }
        Scope::Global => {
            let home = dirs_home()?;
            for a in agents {
                if let Some(t) = a.commands_global_path(&home) {
                    if seen.insert(t.path.clone()) {
                        out.push(t);
                    }
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// F-10: scope_root / relativize_dest / resolve_dest (lock portability)
// ---------------------------------------------------------------------------

/// Root that lock-file `destination` paths are stored relative to, so the
/// committed lock stays portable across machines and users.
/// Project scope → the project root; Global scope → the user's home directory.
pub fn scope_root(scope: Scope, project_root: &Path) -> Result<PathBuf> {
    match scope {
        Scope::Project => Ok(project_root.to_path_buf()),
        Scope::Global => dirs_home(),
    }
}

/// Make an absolute install path portable by storing it relative to `root`.
/// Paths outside `root` (e.g. a custom absolute `destination`) are kept as-is.
pub fn relativize_dest(abs: &Path, root: &Path) -> String {
    match abs.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => abs.to_string_lossy().to_string(),
    }
}

/// Inverse of [`relativize_dest`]: resolve a stored `destination` back to an
/// absolute path. Already-absolute values (legacy locks, out-of-root paths)
/// are returned unchanged.
pub fn resolve_dest(stored: &str, root: &Path) -> PathBuf {
    let p = PathBuf::from(stored);
    if p.is_absolute() {
        p
    } else {
        root.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique scratch directory for tests (process-pid + nanos keyed; mirrors the local
    /// `temp_dir` helper used elsewhere in this crate).
    fn temp_dir(prefix: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }

    // --- F-05: resolve_path (ported verbatim from kasetto src/fsops/mod.rs) ---

    #[test]
    fn resolve_path_expands_only_leading_tilde() {
        // Do not mutate the process-global HOME (sibling threads read it). The bare-`~`
        // result IS the resolved home, so `~/skills` must equal it joined with `skills` —
        // race-immune regardless of what HOME the ambient process carries.
        let base = Path::new("/base");
        let home = resolve_path(base, "~");
        assert!(home.is_absolute(), "~ must expand to an absolute home");
        assert_eq!(resolve_path(base, "~/skills"), home.join("skills"));
        // A `~` that is not the home prefix is an ordinary path character.
        assert_eq!(
            resolve_path(base, "backup~old/skills"),
            Path::new("/base/backup~old/skills")
        );
        // A bare relative path joins onto `base`.
        assert_eq!(resolve_path(base, "rel/dir"), Path::new("/base/rel/dir"));
        // An absolute path is returned as-is.
        assert_eq!(resolve_path(base, "/abs/dir"), Path::new("/abs/dir"));
    }

    // --- F-06: select_targets (ported verbatim from kasetto src/fsops/mod.rs) ---

    #[test]
    fn select_targets_wildcard_is_sorted() {
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
    fn select_targets_non_star_wildcard_is_error() {
        // A wildcard string other than "*" falls through to the `invalid skills field` arm.
        let available = HashMap::new();
        let sf = SkillsField::Wildcard("all".into());
        let res = select_targets(&sf, &available, Path::new("/tmp"));
        assert!(res.is_err());
    }

    #[test]
    fn select_targets_reports_missing_skill() {
        let mut available = HashMap::new();
        available.insert("present".to_string(), PathBuf::from("/tmp/present"));
        let sf = SkillsField::List(vec![
            SkillTarget::Name("present".to_string()),
            SkillTarget::Name("missing".to_string()),
        ]);

        let (targets, broken) = select_targets(&sf, &available, Path::new("/tmp")).expect("select");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].0, "present");
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].name, "missing");
        assert!(broken[0].reason.contains("skill not found"));
    }

    #[test]
    fn select_targets_prefers_explicit_path_override() {
        let root = temp_dir("agent-env-targets");
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

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn select_targets_resolves_relative_path_against_source_root() {
        let root = temp_dir("agent-env-targets-rel");
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

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn select_targets_obj_missing_skill_md_is_broken() {
        let root = temp_dir("agent-env-targets-nomd");
        let skill_dir = root.join("pack/no-md");
        fs::create_dir_all(&skill_dir).expect("create dirs");
        // No SKILL.md written.

        let available = HashMap::new();
        let sf = SkillsField::List(vec![SkillTarget::Obj {
            name: "no-md".to_string(),
            path: Some("pack".to_string()),
        }]);
        let (targets, broken) = select_targets(&sf, &available, &root).expect("select");
        assert!(targets.is_empty());
        assert_eq!(broken.len(), 1);
        assert!(broken[0].reason.contains("skill not found at"));

        let _ = fs::remove_dir_all(&root);
    }

    // --- F-10: scope_root / relativize_dest / resolve_dest round-trip ---

    #[test]
    fn scope_root_project_is_project_root() {
        let pr = Path::new("/work/proj");
        assert_eq!(scope_root(Scope::Project, pr).expect("root"), pr);
    }

    #[test]
    fn scope_root_global_is_home() {
        // `scope_root(Global, _)` delegates to `dirs_home()` and ignores `project_root`.
        // Assert it is an absolute home path (do not mutate/compare against a separately
        // read HOME — sibling threads may swap the process-global env mid-test).
        let root = scope_root(Scope::Global, Path::new("/ignored")).expect("root");
        assert!(
            root.is_absolute(),
            "global scope root must be the absolute home"
        );
    }

    #[test]
    fn relativize_then_resolve_round_trips() {
        let root = Path::new("/home/u");
        let abs = root.join(".claude/skills/foo");
        let stored = relativize_dest(&abs, root);
        assert_eq!(stored, ".claude/skills/foo");
        assert_eq!(resolve_dest(&stored, root), abs);
    }

    #[test]
    fn relativize_keeps_out_of_root_paths_absolute() {
        let root = Path::new("/home/u");
        let outside = Path::new("/opt/custom/skills");
        let stored = relativize_dest(outside, root);
        assert_eq!(stored, "/opt/custom/skills");
        // resolve_dest keeps an already-absolute stored value unchanged.
        assert_eq!(resolve_dest(&stored, root), outside);
    }

    // --- F-07/F-08/F-09: resolution against the agent-env Config + Agent path methods ---

    #[test]
    fn resolve_destinations_explicit_destination_wins() {
        let cfg: Config =
            serde_yaml::from_str("destination: ./out/skills\nskills: []\n").expect("parse config");
        let base = Path::new("/proj");
        let dests = resolve_destinations(base, &cfg, Scope::Project).expect("resolve");
        assert_eq!(dests, vec![PathBuf::from("/proj/out/skills")]);
    }

    #[test]
    fn resolve_destinations_per_agent_project_scope() {
        let cfg: Config = serde_yaml::from_str("agent:\n  - claude-code\n  - cursor\nskills: []\n")
            .expect("parse config");
        let base = Path::new("/proj");
        let dests = resolve_destinations(base, &cfg, Scope::Project).expect("resolve");
        assert_eq!(
            dests,
            vec![base.join(".claude/skills"), base.join(".cursor/skills"),]
        );
    }

    #[test]
    fn resolve_destinations_requires_destination_or_agent() {
        let cfg: Config = serde_yaml::from_str("skills: []\n").expect("parse config");
        let res = resolve_destinations(Path::new("/proj"), &cfg, Scope::Project);
        assert!(res.is_err());
    }

    #[test]
    fn resolve_mcp_settings_targets_project_dedupes() {
        // antigravity + augment both fall through to project `.mcp.json`; expect one entry.
        let cfg: Config =
            serde_yaml::from_str("agent:\n  - antigravity\n  - augment\nskills: []\n")
                .expect("parse config");
        let pr = Path::new("/proj");
        let targets = resolve_mcp_settings_targets(&cfg, Scope::Project, pr).expect("resolve");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, pr.join(".mcp.json"));
    }

    #[test]
    fn resolve_mcp_settings_targets_empty_agents_is_empty() {
        let cfg: Config = serde_yaml::from_str("skills: []\n").expect("parse config");
        let targets =
            resolve_mcp_settings_targets(&cfg, Scope::Project, Path::new("/proj")).expect("ok");
        assert!(targets.is_empty());
    }

    #[test]
    fn resolve_command_targets_filters_unsupported() {
        // codex has no project command surface; claude-code does → exactly one target.
        let cfg: Config = serde_yaml::from_str("agent:\n  - claude-code\n  - codex\nskills: []\n")
            .expect("parse config");
        let pr = Path::new("/proj");
        let targets = resolve_command_targets(&cfg, Scope::Project, pr).expect("resolve");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, pr.join(".claude/commands"));
    }

    // --- F-03: copy_dir (ported verbatim from kasetto src/fsops/copy.rs) ---

    #[cfg(unix)]
    #[test]
    fn copy_dir_preserves_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let src = temp_dir("agent-env-copy-perm-src");
        fs::create_dir_all(&src).expect("create src");
        let script = src.join("run.sh");
        fs::write(&script, "#!/bin/sh\n").expect("write script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");

        let dst = temp_dir("agent-env-copy-perm-dst");
        copy_dir(&src, &dst).expect("copy dir");

        let mode = fs::metadata(dst.join("run.sh"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "executable bit must survive the copy");

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_follows_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let src = temp_dir("agent-env-copy-src");
        let refs_dir = src.join("references");
        fs::create_dir_all(&refs_dir).expect("create refs");
        fs::write(refs_dir.join("guide.md"), "hello").expect("write file");
        symlink("references", src.join("linked-references")).expect("create symlink");

        let dst = temp_dir("agent-env-copy-dst");
        copy_dir(&src, &dst).expect("copy dir");

        assert!(dst.join("linked-references/guide.md").is_file());
        assert!(dst.join("references/guide.md").is_file());

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_contents_depth_guard_refuses_symlink_cycle() {
        use std::os::unix::fs::symlink;

        // Build a self-referential symlink cycle: `cycle/loop -> cycle`.
        let src = temp_dir("agent-env-copy-cycle-src");
        let cycle = src.join("cycle");
        fs::create_dir_all(&cycle).expect("create cycle");
        symlink(&cycle, cycle.join("loop")).expect("create cycle symlink");

        let dst = temp_dir("agent-env-copy-cycle-dst");
        let res = copy_dir(&src, &dst);
        assert!(res.is_err(), "symlink cycle must hit the depth guard");
        let msg = format!("{}", res.unwrap_err());
        assert!(
            msg.contains("copy depth limit") || msg.contains("symlink cycle"),
            "unexpected error: {msg}"
        );

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    // --- F-04: SettingsFile (ported verbatim from kasetto src/fsops/settings.rs) ---

    #[test]
    fn settings_file_load_creates_empty_for_missing_file() {
        let dir = temp_dir("agent-env-sf-missing");
        let path = dir.join("nonexistent.json");
        let sf = SettingsFile::load(&path).expect("load");
        assert_eq!(sf.data, serde_json::json!({}));
    }

    #[test]
    fn settings_file_load_parses_existing_json() {
        let dir = temp_dir("agent-env-sf-parse");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"mcpServers":{}}"#).unwrap();

        let sf = SettingsFile::load(&path).expect("load");
        assert!(sf.data["mcpServers"].is_object());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_file_save_creates_parent_dirs() {
        let dir = temp_dir("agent-env-sf-save");
        let nested = dir.join("deep").join("path").join("settings.json");

        let mut sf = SettingsFile::load(&nested).expect("load");
        sf.data["key"] = serde_json::json!("value");
        sf.save().expect("save");

        let text = fs::read_to_string(&nested).unwrap();
        let val: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(val["key"], "value");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_file_load_rejects_invalid_json() {
        let dir = temp_dir("agent-env-sf-invalid");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        fs::write(&path, "not valid json {{{").unwrap();

        let result = SettingsFile::load(&path);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&dir);
    }
}
