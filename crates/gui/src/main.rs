//! `envctl-gui` — native egui/eframe dashboard over the shared engine.
//!
//! ONE worker thread runs `run_event_loop`. The spawn closure captures only
//! `Send + 'static` values — an owned `Engine` clone, the mpsc endpoints, and a
//! `Box<dyn FnMut() + Send + 'static>` repaint hook built from a cloned
//! `egui::Context` (Arc-backed). `update()` drains events non-blocking via
//! `try_recv`, so the UI thread never blocks on engine work. This file is the
//! explicit proof the worker-closure bounds hold.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use eframe::egui;
use envctl_engine::{
    run_event_loop, ComponentState, Engine, EngineCommand, EngineEvent, Event, OpStatus, Telemetry,
};
use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions::default();
    eframe::run_native(
        "envctl",
        opts,
        Box::new(|cc| Ok(Box::new(EnvctlApp::new(cc)))),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Dashboard,
    Components,
    AddRepo,
    Logs,
    Settings,
}

struct EnvctlApp {
    cmd_tx: Sender<EngineCommand>,
    evt_rx: Receiver<EngineEvent>,
    screen: Screen,
    header: String,
    components: Vec<ComponentState>,
    busy: HashSet<String>,
    log: VecDeque<String>,
    log_cap: usize,
    telemetry: Option<Telemetry>,
    dry_run_default: bool,
    // add-repo form
    add_url: String,
    add_id: String,
    add_build: String,
}

impl EnvctlApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (cmd_tx, cmd_rx) = channel::<EngineCommand>();
        let (evt_tx, evt_rx) = channel::<EngineEvent>();
        let ctx = cc.egui_ctx.clone(); // Arc-backed: Send + Sync + 'static

        // THE worker spawn. Every captured value is Send + 'static => the closure
        // is Send + 'static => std::thread::spawn accepts it.
        let engine = Engine::load_default().expect("manifest load");
        std::thread::spawn(move || {
            let repaint: Box<dyn FnMut() + Send + 'static> = Box::new(move || ctx.request_repaint());
            run_event_loop(engine, cmd_rx, evt_tx, repaint);
        });

        let app = Self {
            cmd_tx,
            evt_rx,
            screen: Screen::Dashboard,
            header: "scanning…".into(),
            components: Vec::new(),
            busy: HashSet::new(),
            log: VecDeque::new(),
            log_cap: 8000,
            telemetry: None,
            dry_run_default: true,
            add_url: String::new(),
            add_id: String::new(),
            add_build: "cargo install --path .".into(),
        };
        let _ = app.cmd_tx.send(EngineCommand::Detect);
        let _ = app.cmd_tx.send(EngineCommand::SampleTelemetry);
        app
    }

    fn drain(&mut self) {
        while let Ok(ev) = self.evt_rx.try_recv() {
            match ev {
                Event::Report { report } => {
                    let detected = report.components.iter().filter(|c| c.detected).count();
                    self.header = format!(
                        "{} GPU(s) · driver {} · {}/{} present · {} drift",
                        report.gpu_count,
                        if report.driver_loaded { "loaded" } else { "not loaded" },
                        detected,
                        report.components.len(),
                        report.drift.len()
                    );
                    self.components = report.components;
                }
                Event::Log { component, line, .. } => self.push_log(format!("[{component}] {line}")),
                Event::Telemetry(t) => self.telemetry = Some(t),
                Event::GuardRefused { component, reason } => {
                    self.push_log(format!("⛔ REFUSED {component}: {reason}"))
                }
                Event::StepFinished { result } => {
                    self.busy.remove(&result.component);
                    if result.status != OpStatus::NoHook {
                        self.push_log(format!(
                            "{} {:?} -> {:?}",
                            result.component, result.phase, result.status
                        ));
                    }
                }
                Event::RunFinished { .. } => {
                    let _ = self.cmd_tx.send(EngineCommand::Detect); // refresh after a run
                }
                _ => {}
            }
        }
    }

    fn push_log(&mut self, line: String) {
        if self.log.len() >= self.log_cap {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    fn dispatch(&mut self, cmd: EngineCommand, busy_id: Option<String>) {
        if let Some(id) = busy_id {
            self.busy.insert(id);
        }
        let _ = self.cmd_tx.send(cmd);
    }
}

impl eframe::App for EnvctlApp {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        self.drain();

        egui::TopBottomPanel::top("nav").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("envctl");
                ui.separator();
                ui.selectable_value(&mut self.screen, Screen::Dashboard, "Dashboard");
                ui.selectable_value(&mut self.screen, Screen::Components, "Components");
                ui.selectable_value(&mut self.screen, Screen::AddRepo, "Add Repo");
                ui.selectable_value(&mut self.screen, Screen::Logs, "Logs");
                ui.selectable_value(&mut self.screen, Screen::Settings, "Settings");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(&self.header);
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.screen {
            Screen::Dashboard => self.dashboard(ui),
            Screen::Components => self.components_screen(ui),
            Screen::AddRepo => self.add_repo_screen(ui),
            Screen::Logs => {
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for l in &self.log {
                            ui.monospace(l);
                        }
                    });
            }
            Screen::Settings => {
                ui.checkbox(&mut self.dry_run_default, "destructive ops dry-run by default");
                if ui.button("Re-detect").clicked() {
                    let _ = self.cmd_tx.send(EngineCommand::Detect);
                }
            }
        });

        // Live telemetry tick while the Dashboard is visible.
        if self.screen == Screen::Dashboard {
            let _ = self.cmd_tx.send(EngineCommand::SampleTelemetry);
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }
}

impl EnvctlApp {
    fn dashboard(&mut self, ui: &mut egui::Ui) {
        ui.heading("System");
        if let Some(t) = self.telemetry.clone() {
            if let (Some(used), Some(total)) = (t.mem_used_mb, t.mem_total_mb) {
                ui.label(format!("memory  {used} / {total} MiB"));
                ui.add(egui::ProgressBar::new(used as f32 / total.max(1) as f32));
            }
            if let Some(la) = t.load_avg {
                ui.label(format!("load avg (1m)  {la:.2}"));
            }
            ui.separator();
            ui.heading("GPUs");
            if t.gpus.is_empty() {
                ui.label("no live GPU telemetry (driver inactive — install/REBOOT nvidia-open)");
            }
            for g in &t.gpus {
                ui.label(format!(
                    "[{}] {}  ·  {}°C  ·  {} / {} MiB{}",
                    g.index,
                    g.name,
                    g.temp_c,
                    g.mem_used_mb,
                    g.mem_total_mb,
                    g.power_w.map(|p| format!("  ·  {p} W")).unwrap_or_default()
                ));
                ui.add(egui::ProgressBar::new(g.util_pct as f32 / 100.0).text(format!("{}%", g.util_pct)));
            }
        } else {
            ui.label("sampling…");
        }
    }

    fn components_screen(&mut self, ui: &mut egui::Ui) {
        if ui.button("Install all missing").clicked() {
            let missing: Vec<String> = self
                .components
                .iter()
                .filter(|c| !c.detected)
                .map(|c| c.id.clone())
                .collect();
            for id in &missing {
                self.busy.insert(id.clone());
            }
            self.dispatch(EngineCommand::Install { targets: missing, dry_run: false }, None);
        }
        ui.separator();
        for c in self.components.clone() {
            ui.horizontal(|ui| {
                let mark = if c.detected { "✓" } else { "·" };
                ui.monospace(format!("{mark} {:<16}", c.id));
                ui.label(&c.name);
                if self.busy.contains(&c.id) {
                    ui.add(egui::Spinner::new());
                } else {
                    if !c.detected && ui.button("Install").clicked() {
                        self.dispatch(
                            EngineCommand::Install { targets: vec![c.id.clone()], dry_run: false },
                            Some(c.id.clone()),
                        );
                    }
                    if c.detected && ui.button("Fix").clicked() {
                        self.dispatch(
                            EngineCommand::Fix { targets: vec![c.id.clone()], dry_run: !self.dry_run_default },
                            Some(c.id.clone()),
                        );
                    }
                }
            });
        }
    }

    fn add_repo_screen(&mut self, ui: &mut egui::Ui) {
        ui.heading("Add a repo as a managed component");
        egui::Grid::new("addrepo").num_columns(2).show(ui, |ui| {
            ui.label("git url");
            ui.text_edit_singleline(&mut self.add_url);
            ui.end_row();
            ui.label("id");
            ui.text_edit_singleline(&mut self.add_id);
            ui.end_row();
            ui.label("build cmd");
            ui.text_edit_singleline(&mut self.add_build);
            ui.end_row();
        });
        ui.horizontal(|ui| {
            let ready = !self.add_url.trim().is_empty() && !self.add_id.trim().is_empty();
            if ui.add_enabled(ready, egui::Button::new("Validate (dry-run)")).clicked() {
                self.dispatch(self.add_repo_cmd(true), None);
                self.screen = Screen::Logs;
            }
            if ui.add_enabled(ready, egui::Button::new("Register")).clicked() {
                self.dispatch(self.add_repo_cmd(false), None);
                self.screen = Screen::Logs;
            }
        });
        ui.label("Register writes a component drop-in; build it from the Components tab (Install).");
    }

    fn add_repo_cmd(&self, dry_run: bool) -> EngineCommand {
        EngineCommand::AddRepo {
            spec: envctl_engine::AddRepoSpec {
                id: self.add_id.trim().to_string(),
                git_url: self.add_url.trim().to_string(),
                git_ref: None,
                build_cmd: self.add_build.trim().to_string(),
                bin_dir: None,
                verify_cmd: None,
            },
            dry_run,
        }
    }
}
