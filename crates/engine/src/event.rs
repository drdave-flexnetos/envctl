//! Progress/log streaming. The engine NEVER prints; it emits `Event`s into an
//! `EventSink` (a newtype over `mpsc::Sender<Event>`). All payloads are
//! `Send + 'static`, so events cross the GUI worker→UI channel unchanged, and the
//! CLI drains the same vocabulary. (`EventSink::channel()`, not `new()`, keeps
//! clippy's `new_ret_no_self` happy — it returns a channel pair, not `Self`.)
use crate::agent::report::{AgentLockDriftItem, AgentReport, AgentVerb};
use crate::agent::AgentScope;
use crate::component::Phase;
use crate::dashboard::{DashboardPlan, DeployOutcome};
use crate::model::{EnvReport, OpResult, RunSummary};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    RunStarted {
        phase: Phase,
        total: usize,
        dry_run: bool,
    },
    StepStarted {
        component: String,
        phase: Phase,
        index: usize,
        total: usize,
    },
    Log {
        component: String,
        stream: Stream,
        line: String,
    },
    StepFinished {
        result: OpResult,
    },
    Telemetry(Telemetry),
    /// The read-only inventory, emitted at the end of auto-detect (drives the
    /// GUI Components grid + Dashboard).
    Report {
        report: EnvReport,
    },
    GuardRefused {
        component: String,
        reason: String,
    },
    RunFinished {
        summary: RunSummary,
    },
    /// The rendered mission-control dashboard plan (read-only render result).
    /// Drives the CLI stdout/JSON output + the GUI dashboard preview.
    Dashboard {
        plan: DashboardPlan,
    },
    /// The outcome of a dashboard deploy (dry-run preview or applied write).
    DashboardDeployed {
        outcome: DeployOutcome,
    },
    /// An agent-asset verb run started (sync/add/remove/lock/list/clean). `dry_run`
    /// reflects preview-vs-apply; `lock_mode` is the resolved mode label.
    AgentRunStarted {
        verb: AgentVerb,
        scope: AgentScope,
        dry_run: bool,
        lock_mode: String,
    },
    /// One per-asset action from an agent-asset verb (live tree for GUI/CLI): the
    /// source it came from, the asset name, the outcome status, and any error detail.
    AgentAction {
        source: Option<String>,
        asset: Option<String>,
        status: String,
        error: Option<String>,
    },
    /// An agent-asset verb run finished — carries the full typed report.
    AgentRunFinished {
        report: AgentReport,
    },
    /// The drift result of `agent_lock --check` (empty = lock is up to date).
    AgentLockChecked {
        drift: Vec<AgentLockDriftItem>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Telemetry {
    pub at_ms: u128,
    pub gpus: Vec<GpuSample>,
    pub load_avg: Option<f32>,
    pub mem_used_mb: Option<u64>,
    pub mem_total_mb: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GpuSample {
    pub index: u32,
    pub name: String,
    pub util_pct: u32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub temp_c: u32,
    pub power_w: Option<u32>,
}

/// Send-able sink: `Sender<Event>` is `Send`, so an `EventSink` moves into the
/// worker thread.
#[derive(Clone)]
pub struct EventSink(Sender<Event>);

impl EventSink {
    /// Construct a sink + its receiving end. (Named `channel`, not `new`, on
    /// purpose: it returns a pair, not `Self`.)
    pub fn channel() -> (EventSink, Receiver<Event>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (EventSink(tx), rx)
    }

    /// A sink whose receiver is dropped: every `emit` is silently discarded.
    /// Used where events aren't consumed (e.g. guard predicate hooks).
    pub fn null() -> EventSink {
        let (tx, _rx) = std::sync::mpsc::channel();
        EventSink(tx)
    }

    pub fn emit(&self, ev: Event) {
        let _ = self.0.send(ev);
    }

    /// Time a closure and stamp `duration_ms` on its result. (The caller — the
    /// executor — owns `StepFinished` emission, so every component emits exactly
    /// one, hook or not.)
    pub fn timed<F: FnOnce() -> OpResult>(&self, f: F) -> OpResult {
        let t = Instant::now();
        let mut r = f();
        r.duration_ms = t.elapsed().as_millis();
        r
    }
}
