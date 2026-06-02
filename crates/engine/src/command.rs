//! The GUI worker API. `EngineCommand` (App→worker) and `EngineEvent` (worker→App,
//! the same `Event` vocabulary the CLI sees) plus `run_event_loop`, which the GUI
//! spawns on ONE thread. Every parameter is `Send + 'static`, so the
//! `std::thread::spawn` closure in the GUI satisfies its bounds.
//!
//! Live streaming: a forwarder thread relays engine events to the UI as they
//! arrive. Telemetry (Phase 5): a DEDICATED sampler thread emits `Event::Telemetry`
//! on a cadence the GUI controls via `TelemetryControl` (backoff when off-Dashboard
//! / unfocused), so a long `engine.run` never starves telemetry.
use crate::{component::Phase, model::{AddRepoSpec, RunPlan}, Engine, Event, EventSink};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum EngineCommand {
    Detect,
    Install { targets: Vec<String>, dry_run: bool },
    Fix { targets: Vec<String>, dry_run: bool },
    Remove { targets: Vec<String>, dry_run: bool },
    AddRepo { spec: AddRepoSpec, dry_run: bool },
    SampleTelemetry,
    Shutdown,
}

pub type EngineEvent = Event;

/// Shared control for the telemetry sampler thread. The GUI sets `cadence_ms`
/// (e.g. 1000 on Dashboard-focused, 10000 elsewhere) and calls `wake()` to sample
/// now. `Clone` is cheap (all Arc).
#[derive(Clone)]
pub struct TelemetryControl {
    alive: Arc<AtomicBool>,
    cadence_ms: Arc<AtomicU64>,
    wake: Arc<Condvar>,
    lock: Arc<Mutex<()>>,
}
impl Default for TelemetryControl {
    fn default() -> Self {
        TelemetryControl {
            alive: Arc::new(AtomicBool::new(true)),
            cadence_ms: Arc::new(AtomicU64::new(1000)),
            wake: Arc::new(Condvar::new()),
            lock: Arc::new(Mutex::new(())),
        }
    }
}
impl TelemetryControl {
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the sampling cadence (ms, clamped to >=250) and wake the sampler.
    pub fn set_cadence(&self, ms: u64) {
        self.cadence_ms.store(ms.max(250), Ordering::Relaxed);
        self.wake.notify_all();
    }
    pub fn sample_now(&self) {
        self.wake.notify_all();
    }
    fn stop(&self) {
        self.alive.store(false, Ordering::Relaxed);
        self.wake.notify_all();
    }
}

fn spawn_sampler(sink: EventSink, ctrl: TelemetryControl) {
    std::thread::spawn(move || {
        while ctrl.alive.load(Ordering::Relaxed) {
            sink.emit(Event::Telemetry(crate::telemetry::sample()));
            let cadence = Duration::from_millis(ctrl.cadence_ms.load(Ordering::Relaxed).max(250));
            let guard = ctrl.lock.lock().unwrap();
            let _ = ctrl.wake.wait_timeout(guard, cadence);
        }
    });
}

pub fn run_event_loop(
    engine: Engine,
    cmd_rx: Receiver<EngineCommand>,
    evt_tx: Sender<EngineEvent>,
    ctrl: TelemetryControl,
    mut repaint: Box<dyn FnMut() + Send + 'static>,
) {
    let (sink, ev_rx) = EventSink::channel();

    // Forwarder: relay every engine event to the UI the instant it's emitted.
    std::thread::spawn(move || {
        while let Ok(ev) = ev_rx.recv() {
            if evt_tx.send(ev).is_err() {
                break;
            }
            repaint();
        }
    });

    // Dedicated telemetry sampler (cadence controlled by the GUI).
    spawn_sampler(sink.clone(), ctrl.clone());

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            EngineCommand::Shutdown => {
                ctrl.stop();
                break;
            }
            EngineCommand::Detect => {
                let _ = engine.detect(&sink);
            }
            EngineCommand::Install { targets, dry_run } => {
                let _ = engine.run(RunPlan::new(Phase::Install, targets, dry_run), &sink);
            }
            EngineCommand::Fix { targets, dry_run } => {
                let _ = engine.run(RunPlan::new(Phase::Fix, targets, dry_run), &sink);
            }
            EngineCommand::Remove { targets, dry_run } => {
                let _ = engine.run(RunPlan::new(Phase::Remove, targets, dry_run), &sink);
            }
            EngineCommand::AddRepo { spec, dry_run } => {
                let _ = engine.add_repo(spec, dry_run, &sink);
            }
            EngineCommand::SampleTelemetry => {
                ctrl.sample_now(); // wake the sampler; it no longer samples on this thread
            }
        }
    }
}
