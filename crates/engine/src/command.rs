//! The GUI worker API. `EngineCommand` (App→worker) and `EngineEvent` (worker→App,
//! the same `Event` vocabulary the CLI sees) plus `run_event_loop`, which the GUI
//! spawns on ONE thread. CRITICAL: every parameter is `Send + 'static`, so the
//! `std::thread::spawn` closure in the GUI satisfies its bounds. The engine crate
//! itself depends on NO egui type — the repaint hook is injected as a boxed
//! `FnMut()`.
//!
//! Liveness note (Phase 2): events are drained into `evt_tx` AFTER each engine op
//! returns. Because the engine emits synchronously on this worker thread, that is
//! a post-op burst, not live mid-build streaming. Correct + compiles; live
//! streaming (one shared channel) is a Phase-2 item (see ROADMAP.md).
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
    // The engine emits into `sink`; we forward `sink`'s receiver into evt_tx.
    let (sink, ev_rx) = EventSink::channel();
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            EngineCommand::Shutdown => break,
            EngineCommand::Detect => {
                let _ = engine.detect(&sink);
                drain(&ev_rx, &evt_tx, &mut repaint);
            }
            EngineCommand::Install { targets, dry_run } => {
                run_plan(&engine, &sink, Phase::Install, targets, dry_run, &ev_rx, &evt_tx, &mut repaint);
            }
            EngineCommand::Fix { targets, dry_run } => {
                run_plan(&engine, &sink, Phase::Fix, targets, dry_run, &ev_rx, &evt_tx, &mut repaint);
            }
            EngineCommand::Remove { targets, dry_run } => {
                run_plan(&engine, &sink, Phase::Remove, targets, dry_run, &ev_rx, &evt_tx, &mut repaint);
            }
            EngineCommand::AddRepo { spec, dry_run } => {
                let _ = engine.add_repo(spec, dry_run, &sink);
                drain(&ev_rx, &evt_tx, &mut repaint);
            }
            EngineCommand::SampleTelemetry => {
                let _ = evt_tx.send(Event::Telemetry(crate::telemetry::sample()));
                repaint();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_plan(
    engine: &Engine,
    sink: &EventSink,
    phase: Phase,
    targets: Vec<String>,
    dry_run: bool,
    ev_rx: &Receiver<Event>,
    evt_tx: &Sender<Event>,
    repaint: &mut Box<dyn FnMut() + Send + 'static>,
) {
    let _ = engine.run(RunPlan { phase, targets, dry_run }, sink);
    drain(ev_rx, evt_tx, repaint);
}

fn drain(ev_rx: &Receiver<Event>, evt_tx: &Sender<Event>, repaint: &mut Box<dyn FnMut() + Send + 'static>) {
    while let Ok(ev) = ev_rx.try_recv() {
        let _ = evt_tx.send(ev);
        repaint();
    }
}
