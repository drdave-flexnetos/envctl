//! Central types: Registry (real load + Kahn topo-sort), OpResult/OpStatus,
//! EnvReport/ComponentState/ToolState, Wiring + sub-structs, RunPlan, RunSummary,
//! AddRepoSpec. Verb→Phase mapping: install→Install, reset→Remove, auto-fix→Fix,
//! auto-detect→Detect(+Verify).
use crate::component::{Component, Phase};
use crate::error::EngineError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The loaded set of components, indexed by id, with a dependency-respecting order.
pub struct Registry {
    by_id: BTreeMap<String, Component>,
    order: Vec<String>,
}

#[derive(Deserialize)]
struct ManifestFile {
    /// Each TOML file holds one or more `[[component]]` tables.
    #[serde(default)]
    component: Vec<Component>,
}

impl Registry {
    /// Load every `<dir>/*.toml` and `<dir>/components.d/*.toml` (add-repo drop-ins).
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let mut by_id: BTreeMap<String, Component> = BTreeMap::new();
        if !dir.is_dir() {
            return Err(EngineError::ManifestDir(dir.display().to_string()).into());
        }
        let mut files: Vec<PathBuf> = Vec::new();
        for d in [dir.to_path_buf(), dir.join("components.d")] {
            if let Ok(rd) = std::fs::read_dir(&d) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|s| s.to_str()) == Some("toml") {
                        files.push(p);
                    }
                }
            }
        }
        files.sort();
        for f in files {
            let text = std::fs::read_to_string(&f)?;
            let mf: ManifestFile = toml::from_str(&text).map_err(|e| EngineError::Manifest {
                file: f.display().to_string(),
                source: e,
            })?;
            for c in mf.component {
                // manifest lint: reject literal `~` in Command argv (would not expand).
                if let Some(crate::component::Hook::Command { args, .. }) = c.remove.as_ref() {
                    if args.iter().any(|a| a.starts_with('~') || a.contains("\"~")) {
                        return Err(anyhow::anyhow!(
                            "component '{}': remove Command hook uses a literal '~' (won't expand); use a Script hook or Wiring",
                            c.id
                        ));
                    }
                }
                by_id.insert(c.id.clone(), c);
            }
        }
        let order = topo_sort(&by_id)?;
        Ok(Registry { by_id, order })
    }

    pub fn get(&self, id: &str) -> Option<&Component> {
        self.by_id.get(id)
    }
    pub fn ordered(&self) -> impl Iterator<Item = &Component> {
        self.order.iter().filter_map(move |id| self.by_id.get(id))
    }
    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.order.iter()
    }

    /// Transitive reverse-dependents of `id` (components that require it, directly
    /// or via a chain), in registry order. Used by `reset --cascade`.
    pub fn reverse_dependents(&self, id: &str) -> Vec<&Component> {
        let mut ids: Vec<String> = Vec::new();
        for cid in &self.order {
            if cid != id && is_prereq_of(&self.by_id, id, cid) {
                ids.push(cid.clone());
            }
        }
        ids.iter().filter_map(|c| self.by_id.get(c)).collect()
    }
    pub fn len(&self) -> usize {
        self.by_id.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// `id` plus its transitive prerequisites, in dependency order.
    pub fn closure(&self, id: &str) -> anyhow::Result<Vec<&Component>> {
        if self.by_id.get(id).is_none() {
            return Err(EngineError::UnknownComponent(id.into()).into());
        }
        let mut out = Vec::new();
        for cid in &self.order {
            if cid == id || is_prereq_of(&self.by_id, cid, id) {
                if let Some(c) = self.by_id.get(cid) {
                    out.push(c);
                }
            }
        }
        Ok(out)
    }
}

/// Kahn topo sort; ties broken by manifest (BTreeMap key) order for determinism.
fn topo_sort(by: &BTreeMap<String, Component>) -> anyhow::Result<Vec<String>> {
    use std::collections::{BTreeSet, HashMap, VecDeque};
    let mut indeg: HashMap<&str, usize> = by.keys().map(|k| (k.as_str(), 0)).collect();
    for c in by.values() {
        // audit fix: dedup requires before counting indegree. The Kahn decrement
        // below uses `.any()` (fires once per unique dep), so counting every
        // occurrence here made a duplicate dep (requires=["bun","bun"]) leave
        // indegree stuck above 0 -> a bogus DependencyCycle.
        for dep in c.requires.iter().collect::<BTreeSet<_>>() {
            if !by.contains_key(dep) {
                return Err(EngineError::UnknownDependency {
                    by: c.id.clone(),
                    dep: dep.clone(),
                }
                .into());
            }
            *indeg.get_mut(c.id.as_str()).unwrap() += 1;
        }
    }
    let mut q: VecDeque<&str> = by
        .keys()
        .filter(|k| indeg[k.as_str()] == 0)
        .map(|k| k.as_str())
        .collect();
    let mut out = Vec::new();
    while let Some(n) = q.pop_front() {
        out.push(n.to_string());
        for c in by.values() {
            if c.requires.iter().any(|d| d == n) {
                let e = indeg.get_mut(c.id.as_str()).unwrap();
                *e -= 1;
                if *e == 0 {
                    q.push_back(c.id.as_str());
                }
            }
        }
    }
    if out.len() != by.len() {
        let stuck = by
            .keys()
            .find(|k| !out.contains(k))
            .cloned()
            .unwrap_or_default();
        return Err(EngineError::DependencyCycle(stuck).into());
    }
    Ok(out)
}

fn is_prereq_of(by: &BTreeMap<String, Component>, cand: &str, target: &str) -> bool {
    if let Some(t) = by.get(target) {
        if t.requires.iter().any(|d| d == cand) {
            return true;
        }
        return t.requires.iter().any(|d| is_prereq_of(by, cand, d));
    }
    false
}

/// The outcome of running one phase on one component.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpResult {
    pub component: String,
    pub phase: Phase,
    pub status: OpStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpStatus {
    Ok,
    Failed,
    Skipped,
    /// A dependency failed/was missing, so this component was not attempted.
    SkippedBlocked,
    /// A guard refused the (destructive) op (fail-closed).
    Refused,
    DryRun,
    NoHook,
    /// Acted, but the change only takes effect after a reboot (e.g. nvidia-open).
    RebootRequired,
    /// Acted (Remove/Fix hook returned Ok|NoHook) but the post-action re-verify
    /// under the FROZEN RunContext found the wrong end-state: reset left the
    /// component still detected, or auto-fix left verify still failing. Distinct
    /// from Failed (the hook itself errored).
    Incomplete,
}

/// The read-only inventory produced by auto-detect.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnvReport {
    pub generated_at: String,
    // host
    pub kernel: Option<String>,
    pub os: Option<String>,
    pub cpu_model: Option<String>,
    pub cpu_threads: usize,
    pub mem_total_mb: u64,
    // gpu (PCI floor works even before the driver is installed)
    pub gpu_present: bool,
    pub gpu_count: usize,
    pub gpus: Vec<String>,
    pub driver_loaded: bool,
    pub driver_version: Option<String>,
    pub cuda_version: Option<String>,
    pub open_kernel_module: bool,
    pub software_rendered: bool,
    // tools + managed components
    pub tools: Vec<ToolState>,
    pub components: Vec<ComponentState>,
    // computed: how detected state diverges from the manifest's desired state
    #[serde(default)]
    pub drift: Vec<DriftItem>,
}

/// One way the detected environment diverges from the manifest's desired state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriftItem {
    pub component: String,
    pub kind: DriftKind,
    pub severity: Severity,
    pub suggested_verb: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    /// Declared + installable, but not detected.
    Missing,
    /// Detected, but its verify hook failed.
    Unhealthy,
    /// Detected, but the envctl shell-rc wiring it owns is absent.
    WiringMissing,
    /// GPUs present but the kernel driver isn't loaded (software-rendered).
    DriverInactive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolState {
    pub name: String,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentState {
    pub id: String,
    pub name: String,
    pub detected: bool,
    pub healthy: Option<bool>,
    pub wiring_present: bool,
    pub note: String,
}

/// Declarative side effects a component owns: written by install, unwound by
/// reset, reconciled by auto-fix. Mirrors the wizard's guarded ~/.bashrc blocks,
/// .desktop autostarts, and PATH edits.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Wiring {
    #[serde(default)]
    pub path_entries: Vec<String>,
    #[serde(default)]
    pub shell_rc: Vec<ShellRcBlock>,
    #[serde(default)]
    pub desktop_entries: Vec<DesktopEntry>,
    #[serde(default)]
    pub systemd_user: Vec<SystemdUnit>,
    // ---- Phase 3: system-scope (sudo + timestamped backup before clobber).
    // Struct order = apply() order; revert() walks the reverse-safe order. ----
    #[serde(default)]
    pub apt_repos: Vec<AptRepo>,
    #[serde(default)]
    pub nix_conf_lines: Vec<NixConfLine>,
    #[serde(default)]
    pub cdi_specs: Vec<CdiSpec>,
    #[serde(default)]
    pub alternatives: Vec<Alternative>,
    /// User data dirs — deleted ONLY with --purge (after UUID re-verify).
    #[serde(default)]
    pub data_paths: Vec<DataPath>,
    /// Config dirs — kept with --keep-config, else removed (engine-owned).
    #[serde(default)]
    pub config_paths: Vec<DataPath>,
}

/// An apt repo = a (.list file + keyring file) PAIR. apply writes the keyring
/// first then the .list; revert removes the .list FIRST then the keyring (never
/// orphan a repo whose key is gone -> `apt update` errors).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AptRepo {
    pub list_file: String,
    pub list_line: String,
    pub keyring_path: String,
    pub keyring_url: String,
    /// True if the URL serves ASCII-armored gpg (needs `gpg --dearmor`).
    #[serde(default)]
    pub dearmor: bool,
    #[serde(default = "default_true")]
    pub apt_update: bool,
}

/// A grep -qF-guarded line in /etc/nix/nix.custom.conf. revert removes only
/// exact-match owned lines, then restarts nix-daemon once.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NixConfLine {
    pub line: String,
}

/// /etc/cdi/nvidia.yaml generated by `nvidia-ctk cdi generate`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CdiSpec {
    pub output: String,
    #[serde(default = "default_cdi_args")]
    pub generate_args: Vec<String>,
}

/// `update-alternatives` slot. revert uses --remove (not --remove-all).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alternative {
    pub link: String,
    pub name: String,
    /// Absolute path, OR a bare command resolved via `which` at apply time.
    pub target: String,
    #[serde(default = "default_alt_priority")]
    pub priority: u32,
}

fn default_true() -> bool {
    true
}
fn default_alt_priority() -> u32 {
    60
}
fn default_cdi_args() -> Vec<String> {
    vec!["cdi".into(), "generate".into()]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShellRcBlock {
    pub file: String,
    pub marker: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DesktopEntry {
    pub filename: String,
    pub content: String,
    #[serde(default)]
    pub one_shot: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemdUnit {
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub enable: bool,
}

/// What to run: one phase across a set of targets (empty = the whole roster).
#[derive(Clone, Debug)]
pub struct RunPlan {
    pub phase: Phase,
    pub targets: Vec<String>,
    pub dry_run: bool,
    /// Reset gates (untargeted/cascade/purge/keep-config). Default = no gates.
    pub gates: ResetGates,
}

impl RunPlan {
    pub fn new(phase: Phase, targets: Vec<String>, dry_run: bool) -> Self {
        RunPlan { phase, targets, dry_run, gates: ResetGates::default() }
    }
    pub fn with_gates(mut self, gates: ResetGates) -> Self {
        self.gates = gates;
        self
    }
}

/// Rolled-up result of a run, mirroring the wizard's `fail[]` roster.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RunSummary {
    pub results: Vec<OpResult>,
    pub failed: Vec<String>,
    pub refused: Vec<String>,
    #[serde(default)]
    pub skipped_blocked: Vec<String>,
    /// Acted but the post-action re-verify disagreed (a half-failed reset/fix).
    #[serde(default)]
    pub incomplete: Vec<String>,
}

impl RunSummary {
    pub fn ok(&self) -> bool {
        self.failed.is_empty() && self.refused.is_empty() && self.incomplete.is_empty()
    }
}

/// Destructive-reset gates (CLI flags). Only consulted on `Phase::Remove`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResetGates {
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub cascade: bool,
    #[serde(default)]
    pub keep_config: bool,
    #[serde(default)]
    pub purge: bool,
}

/// A data/config directory a component owns. `data_paths` are deleted ONLY with
/// `--purge` after a fail-closed UUID re-verify; `uuid: None` is always refused.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataPath {
    pub path: String,
    #[serde(default)]
    pub uuid: Option<String>,
}

/// A request to build-from-source and wire in an upstream repo (Phase 4).
/// `build_cmd` stays a `String` ("" = let the detector choose); all new fields are
/// `#[serde(default)]` and the struct derives `Default` so old call sites that use
/// `..Default::default()` keep compiling.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AddRepoSpec {
    pub id: String,
    pub git_url: String,
    #[serde(default)]
    pub local_path: Option<PathBuf>,
    #[serde(default)]
    pub git_ref: Option<String>,
    /// "" = detector default; non-empty overrides the whole build command.
    #[serde(default)]
    pub build_cmd: String,
    #[serde(default)]
    pub build_system: Option<BuildSystem>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub strategy: BuildStrategy,
    #[serde(default)]
    pub bin_dir: Option<PathBuf>,
    #[serde(default)]
    pub daemon: bool,
    #[serde(default)]
    pub verify_cmd: Option<String>,
    /// SAFETY opt-in: only with `--build` do we run the upstream build / AI agent /
    /// install. Without it add-repo is acquire+detect+preview only.
    #[serde(default)]
    pub allow_build: bool,
    /// Opt-in to back-up-then-replace a real foreign file at an install target.
    #[serde(default)]
    pub force: bool,
    /// Opt-in to `git clone --recurse-submodules` (off by default).
    #[serde(default)]
    pub recurse_submodules: bool,
}

/// Which build-system the pipeline drives. `Auto` (default) sniffs signal files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildSystem {
    #[default]
    Auto,
    Cargo,
    Cmake,
    Meson,
    Autotools,
    Make,
    Node,
    Python,
    NixFlake,
    Go,
    Zig,
}

/// HOW we transform the acquired tree before/while building + installing it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuildStrategy {
    #[default]
    AsIs,
    CherryPick {
        bins: Vec<String>,
    },
    Rename {
        renames: Vec<RenameRule>,
    },
    Refactor {
        refactor: Refactor,
    },
}

/// `old=new` install-name remap.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenameRule {
    pub from: String,
    pub to: String,
}

/// A pre-build transform applied in-place inside the 0700 clone dir.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Refactor {
    /// Run a user-provided shell transform command in the clone (`bash -lc`).
    Patch { command: String },
    /// Drive an available AI coding CLI non-interactively IN the clone with a
    /// structured instruction; the agent does the code work, then the pipeline
    /// re-detects the (often now-cargo) tree and builds the result.
    Ai {
        #[serde(default)]
        agent: Option<AiAgent>,
        goal: RefactorGoal,
        #[serde(default)]
        instruction: Option<String>,
    },
}

/// The structured headline transforms the AI strategy can request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefactorGoal {
    PortToRust,
    CherryPickToCrate,
    RenameForSynergy,
    Custom,
}

/// AI coding CLIs envctl knows how to orchestrate non-interactively.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiAgent {
    Claude,
    Codex,
    Gemini,
    Kimi,
}

impl AiAgent {
    pub fn preference() -> [AiAgent; 4] {
        [AiAgent::Claude, AiAgent::Codex, AiAgent::Gemini, AiAgent::Kimi]
    }
    pub fn bin(self) -> &'static str {
        match self {
            AiAgent::Claude => "claude",
            AiAgent::Codex => "codex",
            AiAgent::Gemini => "gemini",
            AiAgent::Kimi => "kimi",
        }
    }
    /// Argv that runs the agent NON-INTERACTIVELY, STRUCTURALLY CONFINED to
    /// `clone_dir`, never with a skip-permissions / yolo flag. The instruction is a
    /// SINGLE argv element (no shell) for injection containment.
    pub fn argv(self, prompt: &str, clone_dir: &str) -> Vec<String> {
        match self {
            AiAgent::Claude => vec![
                "-p".into(),
                prompt.into(),
                "--permission-mode".into(),
                "acceptEdits".into(),
                "--add-dir".into(),
                clone_dir.into(),
                "--output-format".into(),
                "text".into(),
            ],
            // audit fix: `--cd` only sets cwd; without `--sandbox workspace-write`
            // codex exec is not filesystem-confined to the clone. Add the sandbox
            // flag so headless Codex is structurally confined like Claude's --add-dir.
            AiAgent::Codex => vec![
                "exec".into(),
                "--sandbox".into(),
                "workspace-write".into(),
                "--cd".into(),
                clone_dir.into(),
                prompt.into(),
            ],
            AiAgent::Gemini => vec!["-p".into(), prompt.into()],
            AiAgent::Kimi => vec!["-p".into(), prompt.into()],
        }
    }
}
