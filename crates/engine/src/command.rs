//! The GUI worker API. `EngineCommand` (App→worker) and `EngineEvent` (worker→App,
//! the same `Event` vocabulary the CLI sees) plus `run_event_loop`, which the GUI
//! spawns on ONE thread. CRITICAL: every parameter is `Send + 'static`, so the
//! `std::thread::spawn` closure in the GUI satisfies its bounds. The engine crate
//! itself depends on NO egui type — the repaint hook is injected as a boxed
//! `FnMut()`.
//!
//! Live streaming (Phase 2): a dedicated forwarder thread relays engine events to
//! the UI *as they arrive*, so a 20-minute apt/nix/CUDA build streams line-by-line
//! into the GUI Live Logs rather than arriving in a post-op burst. Everything —
//! including telemetry — flows through the one engine `sink`.
use crate::{component::Phase, model::{AddRepoSpec, RunPlan}, Engine, Event, EventSink};
use std::sync::mpsc::{Receiver, Sender};

/// App -> worker. Owned data only (Strings, owned specs) => `Send + 'static`.
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

/// worker -> App. Reuse the engine `Event` enum so CLI and GUI share one
/// event vocabulary (the identical-API guarantee).
pub type EngineEvent = Event;

pub fn run_event_loop(
    engine: Engine,
    cmd_rx: Receiver<EngineCommand>,
    evt_tx: Sender<EngineEvent>,
    mut repaint: Box<dyn FnMut() + Send + 'static>,
) {
    let (sink, ev_rx) = EventSink::channel();

    // Forwarder thread: relay every engine event to the UI the instant it is
    // emitted (live streaming), requesting a repaint after each. Captures only
    // Send + 'static values. Exits when `sink` is dropped (this fn returns).
    std::thread::spawn(move || {
        while let Ok(ev) = ev_rx.recv() {
            if evt_tx.send(ev).is_err() {
                break;
            }
            repaint();
        }
    });

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            EngineCommand::Shutdown => break,
            EngineCommand::Detect => {
                let _ = engine.detect(&sink);
            }
            EngineCommand::Install { targets, dry_run } => {
                let _ = engine.run(RunPlan { phase: Phase::Install, targets, dry_run }, &sink);
            }
            EngineCommand::Fix { targets, dry_run } => {
                let _ = engine.run(RunPlan { phase: Phase::Fix, targets, dry_run }, &sink);
            }
            EngineCommand::Remove { targets, dry_run } => {
                let _ = engine.run(RunPlan { phase: Phase::Remove, targets, dry_run }, &sink);
            }
            EngineCommand::AddRepo { spec, dry_run } => {
                let _ = engine.add_repo(spec, dry_run, &sink);
            }
            EngineCommand::SampleTelemetry => {
                sink.emit(Event::Telemetry(crate::telemetry::sample()));
            }
        }
    }
}
