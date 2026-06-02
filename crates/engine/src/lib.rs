//! envctl engine: the single shared library. No printing, no UI, no clap.
//!
//! Both the CLI (`envctl`) and the GUI (`envctl-gui`) drive the box through the
//! *identical* `Engine` API below, so the two front-ends can never diverge.
pub mod component; // Component, Hook, Guard, Phase, HookRunner
pub mod model; // Registry, OpResult, OpStatus, EnvReport, Wiring, RunPlan, RunSummary, AddRepoSpec
pub mod event; // Event, EventSink, Stream, Telemetry, GpuSample
pub mod error; // EngineError, RunContext, run_phase
pub mod guard; // fail-closed UuidResolves/NotLiveDevice/NotMounted/PathExists/HookSucceeds
pub mod wiring; // apply()/revert() for Wiring (shell_rc backup-then-excise)
pub mod runner; // ProcessRunner (real) + DryRunRunner impls of HookRunner
pub mod detect; // EnvReport assembly: PCI floor / nvidia-smi / sysinfo / which probes
pub mod drift; // pure diff(EnvReport, Registry) -> Vec<DriftItem>
pub mod graph; // graph intelligence over the component dependency DAG
pub mod lock; // envctl.lock — content-hashed manifest-of-record + CI gate
pub mod telemetry; // sample() -> Telemetry (nvidia-smi CSV + sysinfo)
pub mod executor; // Engine::run(plan) best-effort loop + RunContext resolve + add_repo
pub mod detect_build; // Phase 4: build-system detector table -> BuildPlan
pub mod addrepo; // Phase 4: the staged build-from-source pipeline + confined AI agent
pub mod install; // Phase 4: symlink artifacts into ~/.local/bin (refuse-unmanaged) + wire-in
pub mod register; // Phase 4: synthesize the components.d drop-in (provenance + rebuild)
pub mod command; // EngineCommand / EngineEvent + run_event_loop (GUI worker API)

pub use component::{Component, Guard, Hook, HookRunner, Phase};
pub use model::{
    AddRepoSpec, AiAgent, BuildStrategy, BuildSystem, ComponentState, DataPath, DesktopEntry,
    DriftItem, DriftKind, EnvReport, OpResult, OpStatus, Refactor, RefactorGoal, Registry,
    RenameRule, ResetGates, RunPlan, RunSummary, Severity, ShellRcBlock, SystemdUnit, ToolState,
    Wiring,
};
pub use event::{Event, EventSink, GpuSample, Stream, Telemetry};
pub use error::{EngineError, RunContext};
pub use runner::{DryRunRunner, ProcessRunner};
pub use command::{run_event_loop, EngineCommand, EngineEvent, TelemetryControl};

use std::path::PathBuf;
use std::sync::Arc;

/// Top-level engine handle: owns the Registry, manifest dir, and a HookRunner.
/// Cheaply cloneable (Arc inside) and `Send + Sync + 'static` so it can be moved
/// into the GUI worker-thread closure.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<EngineInner>,
}

struct EngineInner {
    registry: Registry,
    manifest_dir: PathBuf,
    // dyn-dispatched; `trait HookRunner: Send + Sync` makes Box<dyn HookRunner>
    // carry Send+Sync automatically, which is what keeps Engine Send+Sync.
    runner: Box<dyn HookRunner>,
}

impl Engine {
    /// Load a manifest dir into an Engine backed by the real ProcessRunner.
    pub fn load(manifest_dir: PathBuf) -> anyhow::Result<Engine> {
        let registry = Registry::load(&manifest_dir)?;
        Ok(Engine {
            inner: Arc::new(EngineInner {
                registry,
                manifest_dir,
                runner: Box::new(ProcessRunner),
            }),
        })
    }

    /// Default manifest dir: `$ENVCTL_MANIFEST_DIR`, else `./manifest`.
    pub fn load_default() -> anyhow::Result<Engine> {
        let dir = std::env::var("ENVCTL_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("manifest"));
        Engine::load(dir)
    }

    /// Construct an Engine with a custom HookRunner (used by tests: DryRunRunner).
    pub fn with_runner(manifest_dir: PathBuf, runner: Box<dyn HookRunner>) -> anyhow::Result<Engine> {
        let registry = Registry::load(&manifest_dir)?;
        Ok(Engine {
            inner: Arc::new(EngineInner {
                registry,
                manifest_dir,
                runner,
            }),
        })
    }

    pub fn registry(&self) -> &Registry {
        &self.inner.registry
    }

    /// The manifest directory (where `envctl.lock` + `components.d/` live).
    pub fn manifest_dir(&self) -> &std::path::Path {
        &self.inner.manifest_dir
    }

    /// THE shared mutating entrypoint (install/reset/auto-fix). Best-effort:
    /// `Err` only for setup-time problems; `Ok(summary)` where `!summary.ok()`
    /// means some components failed or were refused. Emits Events into `sink`.
    pub fn run(&self, plan: RunPlan, sink: &EventSink) -> anyhow::Result<RunSummary> {
        executor::run(&self.inner.registry, self.inner.runner.as_ref(), plan, sink)
    }

    /// Read-only auto-detect. Never writes. Used identically by `envctl
    /// auto-detect` and the GUI status grid. Emits a final `Event::Report`.
    pub fn detect(&self, sink: &EventSink) -> anyhow::Result<EnvReport> {
        detect::run(&self.inner.registry, self.inner.runner.as_ref(), sink)
    }

    /// add-repo: synthesize a build-from-source Component, persist a drop-in
    /// under `<manifest_dir>/components.d/<id>.toml` (atomic + backed up), then
    /// (unless dry_run) install it.
    /// Interactive handoff: clone + drop the user into an agent session in the
    /// clone (for cherry-pick / port-to-rust). Blocks on the real terminal; runs
    /// on the caller's (main) thread, NOT the GUI worker. Never as root.
    pub fn connect_repo(&self, spec: &AddRepoSpec) -> anyhow::Result<()> {
        crate::addrepo::connect_agent(spec)
    }

    pub fn add_repo(
        &self,
        spec: AddRepoSpec,
        dry_run: bool,
        sink: &EventSink,
    ) -> anyhow::Result<RunSummary> {
        executor::add_repo(
            &self.inner.manifest_dir,
            &self.inner.registry,
            self.inner.runner.as_ref(),
            spec,
            dry_run,
            sink,
        )
    }
}
