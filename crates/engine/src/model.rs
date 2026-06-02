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
    use std::collections::{HashMap, VecDeque};
    let mut indeg: HashMap<&str, usize> = by.keys().map(|k| (k.as_str(), 0)).collect();
    for c in by.values() {
        for dep in &c.requires {
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
}

/// Rolled-up result of a run, mirroring the wizard's `fail[]` roster.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RunSummary {
    pub results: Vec<OpResult>,
    pub failed: Vec<String>,
    pub refused: Vec<String>,
    #[serde(default)]
    pub skipped_blocked: Vec<String>,
}

impl RunSummary {
    pub fn ok(&self) -> bool {
        self.failed.is_empty() && self.refused.is_empty()
    }
}

/// A request to build-from-source and wire in an upstream repo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddRepoSpec {
    pub id: String,
    pub git_url: String,
    #[serde(default)]
    pub git_ref: Option<String>,
    pub build_cmd: String,
    #[serde(default)]
    pub bin_dir: Option<PathBuf>,
    #[serde(default)]
    pub verify_cmd: Option<String>,
}
