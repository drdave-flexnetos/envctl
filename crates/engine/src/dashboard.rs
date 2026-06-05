//! meta mission-control dashboard: read the `.meta.yaml` workspace config and
//! render a deterministic zellij KDL layout (tabs grouped by tag, pane-per-repo,
//! a fixed mission-control overview tab). Pure-Rust, sync, non-printing — the
//! render path is read-only and safe anytime; the deploy path is fail-closed and
//! dry-run by default (see `deploy`).
//!
//! No-drift guarantee: the generator READS `.meta.yaml` at runtime and never
//! hardcodes repos, so both the `envctl dashboard` CLI verb and the GUI parity
//! action (and a future `meta-dashboard` plugin shelling to `--json`) hit this
//! identical code path and cannot diverge.
//!
//! KDL is emitted as a `String` (no kdl crate dependency). Layout syntax is the
//! zellij layout KDL: a top-level `layout { tab name="..." { pane name="..."
//! cwd="..." command="..." { args "..." } } }` tree.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ============================================================ types ===========

/// One project entry parsed from `.meta.yaml` `projects`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaRepo {
    /// The project key (its directory name in the workspace).
    pub id: String,
    /// Git remote (informational; not used by the renderer).
    #[serde(default)]
    pub repo: Option<String>,
    /// `.meta.yaml` tags (drive tab grouping).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Declared dependencies (informational; preserved for downstream ordering).
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Declared capabilities (informational).
    #[serde(default)]
    pub provides: Vec<String>,
}

/// The parsed `.meta.yaml` workspace: an ordered list of projects plus the
/// absolute path of the workspace root (the dir containing `.meta.yaml`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaWorkspace {
    /// Absolute path of the directory containing `.meta.yaml`.
    pub root: PathBuf,
    /// Projects in declaration order (BTreeMap-from-YAML is sorted; see reader).
    pub repos: Vec<MetaRepo>,
}

/// Render-time knobs for the dashboard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardSpec {
    /// Layout name (the `<name>.kdl` file stem + the deploy target).
    pub name: String,
    /// Max panes per tab before spilling into numbered sub-tabs (`ai (1)`...).
    pub panes_per_tab: usize,
    /// Per-pane launcher command (the shipped `envctl-dashboard-pane` asset).
    pub pane_command: String,
}

impl Default for DashboardSpec {
    fn default() -> Self {
        DashboardSpec {
            name: "mission-control".into(),
            panes_per_tab: 6,
            pane_command: "envctl-dashboard-pane".into(),
        }
    }
}

/// One rendered tab: a name + its panes (one per repo).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardTab {
    pub name: String,
    pub panes: Vec<DashboardPane>,
}

/// One rendered pane: a repo id, its absolute cwd, the launcher command, and the
/// args passed to the launcher (the repo id, used as the mesh identity seed).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardPane {
    pub repo: String,
    pub cwd: PathBuf,
    pub command: String,
    pub args: Vec<String>,
}

/// The fully rendered plan: the ordered tabs + the emitted KDL string + the
/// resolved deploy target path. Returned by `render`, carried by `Event::Dashboard`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardPlan {
    pub name: String,
    pub tabs: Vec<DashboardTab>,
    pub kdl: String,
    /// Where `deploy` would write (`~/.config/yazelix/.../<name>.kdl`).
    pub target: PathBuf,
}

// =========================================================== reader ===========

/// Locate `.meta.yaml` by walking UP from `start`, honoring an explicit override
/// (`--meta-file` / `$META_FILE`) first. Returns the resolved file path.
pub fn locate_meta_file(start: &Path, override_path: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(p) = override_path {
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
        anyhow::bail!("meta file not found: {}", p.display());
    }
    if let Ok(env) = std::env::var("META_FILE") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Ok(p);
        }
        anyhow::bail!("$META_FILE not found: {}", p.display());
    }
    let mut dir: Option<&Path> = Some(start);
    while let Some(d) = dir {
        let cand = d.join(".meta.yaml");
        if cand.is_file() {
            return Ok(cand);
        }
        dir = d.parent();
    }
    anyhow::bail!(
        "no .meta.yaml found walking up from {} (pass --meta-file or set $META_FILE)",
        start.display()
    )
}

/// Parse a `.meta.yaml` file into a `MetaWorkspace`. Pure-Rust YAML (serde_yaml).
pub fn read_workspace(meta_file: &Path) -> anyhow::Result<MetaWorkspace> {
    let text = std::fs::read_to_string(meta_file)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", meta_file.display()))?;
    parse_workspace(&text, meta_file)
}

/// The shape of `.meta.yaml` we parse. We deserialize only the fields we use and
/// ignore the rest (meta's config has more keys we don't touch).
#[derive(Deserialize)]
struct RawMeta {
    #[serde(default)]
    projects: BTreeMap<String, RawProject>,
}

/// A project value may be a bare string (`repo: git@...`) or a map. We accept a
/// map and pull the fields we care about; serde_yaml ignores unknown keys.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawProject {
    /// `name: "git@github.com:org/x.git"` — bare repo URL.
    Url(String),
    /// `name: { repo: ..., tags: [...], depends_on: [...], provides: [...] }`.
    Map(RawProjectMap),
}

#[derive(Deserialize, Default)]
struct RawProjectMap {
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    provides: Vec<String>,
}

/// Parse `.meta.yaml` text into a `MetaWorkspace`. The root is the file's parent.
pub fn parse_workspace(text: &str, meta_file: &Path) -> anyhow::Result<MetaWorkspace> {
    let raw: RawMeta = serde_yaml::from_str(text)
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", meta_file.display()))?;
    let root = meta_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    // BTreeMap gives a deterministic (lexicographic) order — declaration order
    // is not preserved by YAML maps, so we use the stable sorted key order as the
    // canonical declaration order for the meta-core tab + golden tests.
    let repos = raw
        .projects
        .into_iter()
        .map(|(id, p)| match p {
            RawProject::Url(repo) => MetaRepo {
                id,
                repo: Some(repo),
                tags: Vec::new(),
                depends_on: Vec::new(),
                provides: Vec::new(),
            },
            RawProject::Map(m) => MetaRepo {
                id,
                repo: m.repo,
                tags: m.tags,
                depends_on: m.depends_on,
                provides: m.provides,
            },
        })
        .collect();
    Ok(MetaWorkspace { root, repos })
}

// ========================================================= grouping ===========

/// Canonical tab order. Untagged repos land in the synthetic `meta-core` tab;
/// tagged repos go to the first matching known tab; anything else → `untriaged`.
const KNOWN_TABS: &[&str] = &[
    "meta-core",
    "tools/env",
    "ops",
    "ai",
    "docs",
    "mcp",
    "hubs",
    "untriaged",
];

/// Map a repo to its tab name. Untagged → `meta-core`. Otherwise the FIRST of its
/// tags that matches a known tab (preserving KNOWN_TABS priority); if no tag
/// matches a known tab, the first tag is used verbatim; empty-but-present → untriaged.
fn tab_for(repo: &MetaRepo) -> String {
    if repo.tags.is_empty() {
        return "meta-core".into();
    }
    // Honor KNOWN_TABS priority so a repo tagged both `ai` and `ops` is deterministic.
    for known in KNOWN_TABS {
        if repo.tags.iter().any(|t| t == known) {
            return (*known).to_string();
        }
    }
    // A tag we don't recognize: keep it verbatim as its own tab (deterministic).
    repo.tags
        .first()
        .cloned()
        .unwrap_or_else(|| "untriaged".into())
}

/// Group repos into ordered (tab-name -> repos) buckets. Within a bucket repos
/// keep workspace order; buckets are ordered by KNOWN_TABS, then any extra
/// custom tabs in sorted order for determinism.
fn group(workspace: &MetaWorkspace) -> Vec<(String, Vec<&MetaRepo>)> {
    let mut buckets: BTreeMap<String, Vec<&MetaRepo>> = BTreeMap::new();
    for r in &workspace.repos {
        buckets.entry(tab_for(r)).or_default().push(r);
    }
    let mut out: Vec<(String, Vec<&MetaRepo>)> = Vec::new();
    // Known tabs first, in canonical order.
    for known in KNOWN_TABS {
        if let Some(v) = buckets.remove(*known) {
            out.push(((*known).to_string(), v));
        }
    }
    // Any remaining custom tabs, sorted (BTreeMap iteration is already sorted).
    for (k, v) in buckets {
        out.push((k, v));
    }
    out
}

// ========================================================= renderer ===========

/// Render the workspace + spec into a `DashboardPlan`. Pure & deterministic:
/// the same inputs always produce byte-identical KDL (golden-file tested).
pub fn render(workspace: &MetaWorkspace, spec: &DashboardSpec) -> DashboardPlan {
    let cap = spec.panes_per_tab.max(1);
    let mut tabs: Vec<DashboardTab> = Vec::new();

    // Fixed mission-control overview tab first (a single free shell, focused).
    tabs.push(DashboardTab {
        name: "mission-control".into(),
        panes: vec![DashboardPane {
            repo: "overview".into(),
            cwd: workspace.root.clone(),
            command: spec.pane_command.clone(),
            args: vec!["overview".into()],
        }],
    });

    for (tab_name, repos) in group(workspace) {
        // Spill into numbered sub-tabs when a group exceeds the cap.
        let chunks: Vec<&[&MetaRepo]> = repos.chunks(cap).collect();
        let multi = chunks.len() > 1;
        for (i, chunk) in chunks.iter().enumerate() {
            let name = if multi {
                format!("{tab_name} ({})", i + 1)
            } else {
                tab_name.clone()
            };
            let panes = chunk
                .iter()
                .map(|r| DashboardPane {
                    repo: r.id.clone(),
                    cwd: workspace.root.join(&r.id),
                    command: spec.pane_command.clone(),
                    args: vec![r.id.clone()],
                })
                .collect();
            tabs.push(DashboardTab { name, panes });
        }
    }

    let kdl = render_kdl(&tabs);
    DashboardPlan {
        name: spec.name.clone(),
        tabs,
        kdl,
        target: layout_target(&spec.name),
    }
}

/// Emit the zellij layout KDL. The mission-control tab is marked `focus=true`.
fn render_kdl(tabs: &[DashboardTab]) -> String {
    let mut s = String::new();
    s.push_str("// Generated by envctl dashboard — do not edit by hand.\n");
    s.push_str("// envctl-owned: regenerate with `envctl dashboard --deploy --apply`.\n");
    s.push_str("layout {\n");
    for tab in tabs {
        let focus = if tab.name == "mission-control" {
            " focus=true"
        } else {
            ""
        };
        s.push_str(&format!(
            "    tab name={}{} {{\n",
            kdl_str(&tab.name),
            focus
        ));
        for pane in &tab.panes {
            s.push_str(&format!(
                "        pane name={} cwd={} command={} {{\n",
                kdl_str(&pane.repo),
                kdl_str(&pane.cwd.to_string_lossy()),
                kdl_str(&pane.command),
            ));
            if !pane.args.is_empty() {
                let args: Vec<String> = pane.args.iter().map(|a| kdl_str(a)).collect();
                s.push_str(&format!("            args {}\n", args.join(" ")));
            }
            s.push_str("        }\n");
        }
        s.push_str("    }\n");
    }
    s.push_str("}\n");
    s
}

/// Quote a string as a KDL string literal (double-quoted, backslash + quote
/// escaped). KDL uses the same escapes as JSON for our purposes.
fn kdl_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

// =========================================================== deploy ===========

const OWNER_MARKER: &str = "Generated by envctl dashboard";

/// Resolve the deploy target: `~/.config/yazelix/configs/zellij/layouts/<name>.kdl`.
pub fn layout_target(name: &str) -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
            PathBuf::from(home).join(".config")
        });
    base.join("yazelix/configs/zellij/layouts")
        .join(format!("{name}.kdl"))
}

/// The outcome of a deploy attempt (carried back so the front-ends can report).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployOutcome {
    pub target: PathBuf,
    /// True if we wrote (or, on dry-run, WOULD write) the layout.
    pub written: bool,
    /// True only when an actual write happened (false on dry-run).
    pub applied: bool,
    /// A backup path, if an existing envctl-owned file was backed up first.
    pub backup: Option<PathBuf>,
    /// Human-readable notes (dry-run preview, refusal reasons, backups).
    pub notes: Vec<String>,
}

/// Fail-closed deploy of a rendered plan to the zellij layouts dir.
///
/// Discipline (mirrors `wiring.rs`):
///   * `dry_run = true` (the default) only previews — NO file is written;
///   * an existing file that is NOT envctl-owned is REFUSED unless `force`;
///   * an existing envctl-owned file is backed up (`.bak.<nanos>`) before overwrite;
///   * the parent dir is created as needed.
pub fn deploy(plan: &DashboardPlan, dry_run: bool, force: bool) -> anyhow::Result<DeployOutcome> {
    let target = plan.target.clone();
    let mut notes = Vec::new();
    let existing = std::fs::read_to_string(&target).ok();

    // Ownership check: refuse to clobber a foreign file without --force.
    let foreign = existing
        .as_deref()
        .map(|c| !c.contains(OWNER_MARKER))
        .unwrap_or(false);
    if foreign && !force {
        anyhow::bail!(
            "refusing to overwrite non-envctl file {} — pass --force to clobber it",
            target.display()
        );
    }

    if dry_run {
        notes.push(format!(
            "dry-run: would write {} bytes to {}",
            plan.kdl.len(),
            target.display()
        ));
        if existing.is_some() {
            notes.push(if foreign {
                "would back up the existing foreign file (--force) before overwrite".into()
            } else {
                "would back up the existing envctl file before overwrite".into()
            });
        }
        return Ok(DeployOutcome {
            target,
            written: true,
            applied: false,
            backup: None,
            notes,
        });
    }

    // Apply path.
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("create {}: {e}", parent.display()))?;
    }
    let mut backup = None;
    if existing.is_some() {
        let bak = PathBuf::from(format!("{}.bak.{}", target.display(), now_nanos()));
        std::fs::copy(&target, &bak).map_err(|e| {
            anyhow::anyhow!("backup {} -> {}: {e}", target.display(), bak.display())
        })?;
        notes.push(format!("backed up existing file -> {}", bak.display()));
        backup = Some(bak);
    }
    std::fs::write(&target, &plan.kdl)
        .map_err(|e| anyhow::anyhow!("write {}: {e}", target.display()))?;
    notes.push(format!("wrote layout {}", target.display()));
    Ok(DeployOutcome {
        target,
        written: true,
        applied: true,
        backup,
        notes,
    })
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

// ============================================================ tests ===========

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
projects:
  meta_cli:
    repo: git@github.com:FlexNetOS/meta_cli.git
  meta_core:
    repo: git@github.com:FlexNetOS/meta_core.git
  envctl:
    repo: git@github.com:FlexNetOS/envctl.git
    tags: [tools/env]
  loop_lib:
    repo: git@github.com:FlexNetOS/loop_lib.git
    tags: [ops]
    depends_on: [meta_core]
  weave:
    repo: git@github.com:FlexNetOS/weave.git
    tags: [mcp]
  repowire: git@github.com:FlexNetOS/repowire.git
"#;

    fn ws() -> MetaWorkspace {
        parse_workspace(FIXTURE, Path::new("/work/.meta.yaml")).unwrap()
    }

    #[test]
    fn reader_parses_projects_map_and_url_forms() {
        let w = ws();
        assert_eq!(w.root, Path::new("/work"));
        // 6 projects, sorted by key (BTreeMap).
        let ids: Vec<&str> = w.repos.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "envctl",
                "loop_lib",
                "meta_cli",
                "meta_core",
                "repowire",
                "weave"
            ]
        );
        // bare-url form parsed with no tags
        let repowire = w.repos.iter().find(|r| r.id == "repowire").unwrap();
        assert!(repowire.tags.is_empty());
        assert_eq!(
            repowire.repo.as_deref(),
            Some("git@github.com:FlexNetOS/repowire.git")
        );
        // map form with tags + depends_on
        let loop_lib = w.repos.iter().find(|r| r.id == "loop_lib").unwrap();
        assert_eq!(loop_lib.tags, vec!["ops"]);
        assert_eq!(loop_lib.depends_on, vec!["meta_core"]);
    }

    #[test]
    fn grouping_untagged_go_to_meta_core() {
        let w = ws();
        let groups = group(&w);
        let names: Vec<&str> = groups.iter().map(|(n, _)| n.as_str()).collect();
        // meta-core first, then tools/env, ops, mcp (canonical order; absent tabs skipped)
        assert_eq!(names, vec!["meta-core", "tools/env", "ops", "mcp"]);
        let core = &groups[0].1;
        let core_ids: Vec<&str> = core.iter().map(|r| r.id.as_str()).collect();
        // untagged: meta_cli, meta_core, repowire (workspace/sorted order)
        assert_eq!(core_ids, vec!["meta_cli", "meta_core", "repowire"]);
    }

    #[test]
    fn render_has_fixed_overview_tab_first() {
        let w = ws();
        let plan = render(&w, &DashboardSpec::default());
        assert_eq!(plan.tabs[0].name, "mission-control");
        assert_eq!(plan.tabs[0].panes[0].repo, "overview");
    }

    #[test]
    fn spill_into_numbered_subtabs_when_over_cap() {
        // 3 untagged repos with cap=2 -> meta-core (1), meta-core (2)
        let w = ws();
        let spec = DashboardSpec {
            panes_per_tab: 2,
            ..DashboardSpec::default()
        };
        let plan = render(&w, &spec);
        let names: Vec<&str> = plan.tabs.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"meta-core (1)"));
        assert!(names.contains(&"meta-core (2)"));
        // and the panes are split 2 + 1
        let t1 = plan
            .tabs
            .iter()
            .find(|t| t.name == "meta-core (1)")
            .unwrap();
        let t2 = plan
            .tabs
            .iter()
            .find(|t| t.name == "meta-core (2)")
            .unwrap();
        assert_eq!(t1.panes.len(), 2);
        assert_eq!(t2.panes.len(), 1);
    }

    #[test]
    fn golden_kdl() {
        let w = ws();
        let plan = render(&w, &DashboardSpec::default());
        let expected = include_str!("../tests/golden/mission-control.kdl");
        assert_eq!(plan.kdl, expected, "KDL drifted from golden file");
    }

    #[test]
    fn deploy_dry_run_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("envctl-dash-{}", now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("mission-control.kdl");
        let plan = DashboardPlan {
            name: "mission-control".into(),
            tabs: vec![],
            kdl: "// Generated by envctl dashboard\nlayout {\n}\n".into(),
            target: target.clone(),
        };
        let out = deploy(&plan, true, false).unwrap();
        assert!(out.written);
        assert!(!out.applied);
        assert!(!target.exists(), "dry-run must not write a file");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deploy_refuses_foreign_file_without_force() {
        let dir = std::env::temp_dir().join(format!("envctl-dash-{}", now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("mission-control.kdl");
        std::fs::write(&target, "layout { tab name=\"hand-written\" {} }\n").unwrap();
        let plan = DashboardPlan {
            name: "mission-control".into(),
            tabs: vec![],
            kdl: "// Generated by envctl dashboard\nlayout {\n}\n".into(),
            target: target.clone(),
        };
        // refused without --force (apply path)
        let err = deploy(&plan, false, false).unwrap_err();
        assert!(err.to_string().contains("refusing to overwrite"));
        // file untouched
        assert!(std::fs::read_to_string(&target)
            .unwrap()
            .contains("hand-written"));
        // with --force it backs up then overwrites
        let out = deploy(&plan, false, true).unwrap();
        assert!(out.applied);
        assert!(out.backup.is_some());
        assert!(std::fs::read_to_string(&target)
            .unwrap()
            .contains("envctl dashboard"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deploy_apply_writes_and_backs_up_owned_file() {
        let dir = std::env::temp_dir().join(format!("envctl-dash-{}", now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("mission-control.kdl");
        let plan = DashboardPlan {
            name: "mission-control".into(),
            tabs: vec![],
            kdl: "// Generated by envctl dashboard\nlayout {\n}\n".into(),
            target: target.clone(),
        };
        // first apply: fresh write, no backup
        let out = deploy(&plan, false, false).unwrap();
        assert!(out.applied);
        assert!(out.backup.is_none());
        assert!(target.exists());
        // second apply: owned file -> backed up, no --force needed
        let out2 = deploy(&plan, false, false).unwrap();
        assert!(out2.applied);
        assert!(out2.backup.is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn locate_walks_up() {
        let dir = std::env::temp_dir().join(format!("envctl-locate-{}", now_nanos()));
        let nested = dir.join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join(".meta.yaml"), "projects: {}\n").unwrap();
        let found = locate_meta_file(&nested, None).unwrap();
        assert_eq!(found, dir.join(".meta.yaml"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
