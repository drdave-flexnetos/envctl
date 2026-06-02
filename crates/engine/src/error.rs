//! Typed `EngineError` (thiserror) for setup-time failures, the `RunContext`
//! (resolve-once identities, no TOCTOU), and the best-effort `run_phase()`
//! wrapper — the Rust analogue of the wizard's `run()`. A failing hook is
//! `OpStatus::Failed`, NEVER an `Err`: only setup problems abort the run.
use crate::component::{Component, HookRunner, Phase};
use crate::event::{Event, EventSink};
use crate::model::{OpResult, OpStatus};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("manifest parse error in {file}: {source}")]
    Manifest {
        file: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("dependency cycle involving component '{0}'")]
    DependencyCycle(String),
    #[error("unknown component id '{0}'")]
    UnknownComponent(String),
    #[error("unknown dependency '{dep}' required by '{by}'")]
    UnknownDependency { by: String, dep: String },
    #[error("manifest dir not found or unreadable: {0}")]
    ManifestDir(String),
}

/// Identities resolved ONCE at run start (resolve-once-then-re-verify, per
/// `ubuntu-boot-repair.sh`). Guards read from here; they never re-resolve
/// mid-run, which is what prevents the TOCTOU the guards exist to stop.
#[derive(Clone, Debug, Default)]
pub struct RunContext {
    pub gpu_present: bool,
    /// UUID of the live/running root filesystem (`findmnt / -> blkid`).
    pub live_root_uuid: Option<String>,
}

/// Run one phase of one component, best-effort. Applies the gpu-required skip and
/// the fail-closed guard engine for destructive phases before dispatching the hook.
#[allow(clippy::too_many_arguments)]
pub fn run_phase(
    sink: &EventSink,
    comp: &Component,
    phase: Phase,
    runner: &dyn HookRunner,
    dry_run: bool,
    ctx: &RunContext,
) -> OpResult {
    // gpu_required skip-with-reason (the wizard's lspci gate).
    if comp.gpu_required && !ctx.gpu_present {
        return OpResult {
            component: comp.id.clone(),
            phase,
            status: OpStatus::Skipped,
            exit_code: None,
            duration_ms: 0,
            message: "skipped: no NVIDIA GPU".into(),
            dry_run,
        };
    }

    // Guards on destructive phases -> Refused (safe), never Failed. Evaluated
    // even on dry-run so a preview tells the truth about what would be refused.
    if comp.destructive && matches!(phase, Phase::Remove | Phase::Fix) {
        if let Some(reason) = crate::guard::check_guards(&comp.guards, runner, ctx) {
            sink.emit(Event::GuardRefused {
                component: comp.id.clone(),
                reason: reason.clone(),
            });
            return OpResult {
                component: comp.id.clone(),
                phase,
                status: OpStatus::Refused,
                exit_code: None,
                duration_ms: 0,
                message: reason,
                dry_run,
            };
        }
    }

    match comp.hook(phase) {
        None => OpResult {
            component: comp.id.clone(),
            phase,
            status: OpStatus::NoHook,
            exit_code: None,
            duration_ms: 0,
            message: String::new(),
            dry_run,
        },
        Some(hook) => sink.timed(|| runner.run(&comp.id, phase, hook, dry_run)),
    }
}
