//! The central data-driven Component model.
//!
//! Design choice: components are DATA (deserialized from TOML), not a Rust trait
//! per component. A `Component` is a manifest record whose five lifecycle phases
//! (detect/install/verify/fix/remove) are each an optional `Hook` — the bash the
//! wizard already proved out, wrapped (never rewritten). Adding a tool means
//! adding a TOML entry, not recompiling. The one *behavioral* abstraction we keep
//! is `HookRunner`, so tests can inject a dry-run/recording runner.
use crate::model::{OpResult, Wiring};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The five verbs map onto these lifecycle phases on every component.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Detect,  // is it already present? (read-only; drives auto-detect)
    Install, // build-from-source / install (additive)
    Verify,  // post-install smoke test (read-only)
    Fix,     // idempotent repair (auto-fix)
    Remove,  // uninstall + unwire (reset)
}

/// A component = one tool the box manages (cuda, nvidia-open, yazelix, bun, ...).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub gpu_required: bool,
    #[serde(default)]
    pub destructive: bool,

    pub detect: Option<Hook>,
    pub install: Option<Hook>,
    pub verify: Option<Hook>,
    pub fix: Option<Hook>,
    pub remove: Option<Hook>,

    /// Declarative side effects this component owns (PATH/shell-rc/desktop/units).
    #[serde(default)]
    pub wiring: Wiring,

    /// Fail-closed safety guards for destructive phases (see `guard.rs`).
    #[serde(default)]
    pub guards: Vec<Guard>,
}

/// A Hook is the proven bash, wrapped — never rewritten. Exit 0 = success; for
/// Detect/Verify, 0 = present/healthy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Hook {
    /// Clean argv, no shell. e.g. command="nvidia-smi" args=["-L"].
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        needs_sudo: bool,
    },
    /// Inline bash (run via `bash -lc <script>`), or a `path` to a script file.
    /// This is how we wrap the existing wizard fragments verbatim.
    Script {
        #[serde(default)]
        script: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        needs_sudo: bool,
        #[serde(default = "default_login_shell")]
        login_shell: bool,
    },
    /// Reference a shipped script verbatim (boot-repair, gpu-verify). Never edited.
    ShippedScript {
        path: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        needs_sudo: bool,
    },
}

fn default_login_shell() -> bool {
    true
}

/// A guard is evaluated before any destructive phase. ANY failing guard aborts
/// that component's op with `Refused` (never a panic). Models
/// `ubuntu-boot-repair.sh`'s resolve+re-verify+refuse discipline. Implemented
/// fail-closed in `guard.rs`: when uncertain, REFUSE.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Guard {
    /// Resolve a block device by UUID and re-verify it carries that UUID.
    UuidResolves { uuid: String },
    /// Refuse if the resolved device == the live/running root device.
    NotLiveDevice { uuid: String },
    /// Refuse unless a path exists before touching it.
    PathExists { path: String },
    /// Refuse if the UUID is currently mounted (the "never umount /home").
    NotMounted { uuid: String },
    /// Generic predicate: a read-only Hook that must exit 0 to proceed.
    HookSucceeds { hook: Hook },
}

impl Component {
    pub fn hook(&self, phase: Phase) -> Option<&Hook> {
        match phase {
            Phase::Detect => self.detect.as_ref(),
            Phase::Install => self.install.as_ref(),
            Phase::Verify => self.verify.as_ref(),
            Phase::Fix => self.fix.as_ref(),
            Phase::Remove => self.remove.as_ref(),
        }
    }
}

/// The one behavioral seam: lets the CLI/GUI/tests inject dry-run / recording
/// runners. MUST be `Send + Sync` so a `Box<dyn HookRunner>` can live inside the
/// `Send + Sync + 'static` Engine moved into the GUI worker thread.
pub trait HookRunner: Send + Sync {
    fn run(&self, comp: &str, phase: Phase, hook: &Hook, dry_run: bool) -> OpResult;
}
