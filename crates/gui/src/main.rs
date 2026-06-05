//! `envctl-gui` — native egui/eframe dashboard over the shared engine.
//!
//! ONE worker thread runs `run_event_loop`. The spawn closure captures only
//! `Send + 'static` values — an owned `Engine` clone, the mpsc endpoints, and a
//! `Box<dyn FnMut() + Send + 'static>` repaint hook built from a cloned
//! `egui::Context` (Arc-backed). `update()` drains events non-blocking via
//! `try_recv`, so the UI thread never blocks on engine work. This file is the
//! explicit proof the worker-closure bounds hold.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod theme;

use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column, TableBuilder};
use envctl_engine::{
    run_event_loop, AddRepoSpec, BuildStrategy, ComponentState, DashboardPlan, DashboardSpec,
    DriftItem, DriftKind, Engine, EngineCommand, EngineEvent, Event, OpStatus, Refactor,
    RefactorGoal, RenameRule, Severity, Stream, Telemetry, TelemetryControl,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1040.0, 720.0]),
        ..Default::default()
    };
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
    Graph,
    AddRepo,
    Mesh,
    Logs,
    Settings,
}

impl Screen {
    fn label(self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Components => "Components",
            Screen::Graph => "Graph",
            Screen::AddRepo => "Add Repo",
            Screen::Mesh => "Mesh",
            Screen::Logs => "Logs",
            Screen::Settings => "Settings",
        }
    }
}

/// One captured log line, with its originating stream so the console can color
/// stderr distinctly. (Owned `String` + `Copy` enum: trivially `Send`.)
struct LogLine {
    stream: Stream,
    text: String,
}

/// Recent GPU utilization for sparklines, keyed by GPU index. Owned `VecDeque`s
/// of plain numbers: `Send`, lives entirely on the UI thread.
const SPARK_LEN: usize = 60;

struct EnvctlApp {
    cmd_tx: Sender<EngineCommand>,
    evt_rx: Receiver<EngineEvent>,
    screen: Screen,
    header: String,
    components: Vec<ComponentState>,
    drift: Vec<DriftItem>,
    busy: HashSet<String>,
    log: VecDeque<LogLine>,
    log_cap: usize,
    telemetry: Option<Telemetry>,
    util_history: HashMap<u32, VecDeque<f32>>,
    dry_run_default: bool,
    filter: String,
    tel: TelemetryControl,
    // read-only engine clone for on-thread graph queries
    geng: Engine,
    graph_focus: String,
    // GPU summary (from the last EnvReport) for the DriverNotActive card
    gpu_present: bool,
    driver_loaded: bool,
    software_rendered: bool,
    gpu_count: usize,
    // add-repo form
    add_url: String,
    add_id: String,
    add_build: String,
    add_strategy: String,
    add_ref: String,
    add_bins: String,
    add_renames: String,
    add_patch: String,
    add_ai_goal: String,
    add_ai_instruction: String,
    add_build_flag: bool,
    // meta mission-control dashboard parity
    dash_plan: Option<DashboardPlan>,
    dash_panes_per_tab: usize,
    dash_status: String,
}

impl EnvctlApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let (cmd_tx, cmd_rx) = channel::<EngineCommand>();
        let (evt_tx, evt_rx) = channel::<EngineEvent>();
        let ctx = cc.egui_ctx.clone(); // Arc-backed: Send + Sync + 'static

        // THE worker spawn. Every captured value is Send + 'static => the closure
        // is Send + 'static => std::thread::spawn accepts it.
        let engine = Engine::load_default().expect("manifest load");
        let geng = engine.clone(); // read-only clone for graph queries on the UI thread
        let tel = TelemetryControl::new();
        let tel_worker = tel.clone();
        std::thread::spawn(move || {
            let repaint: Box<dyn FnMut() + Send + 'static> =
                Box::new(move || ctx.request_repaint());
            run_event_loop(engine, cmd_rx, evt_tx, tel_worker, repaint);
        });

        let app = Self {
            cmd_tx,
            evt_rx,
            screen: Screen::Dashboard,
            header: "scanning…".into(),
            components: Vec::new(),
            drift: Vec::new(),
            busy: HashSet::new(),
            log: VecDeque::new(),
            log_cap: 8000,
            telemetry: None,
            util_history: HashMap::new(),
            dry_run_default: true,
            filter: String::new(),
            tel,
            geng,
            graph_focus: String::new(),
            gpu_present: false,
            driver_loaded: false,
            software_rendered: false,
            gpu_count: 0,
            add_url: String::new(),
            add_id: String::new(),
            add_build: String::new(),
            add_strategy: "as-is".into(),
            add_ref: String::new(),
            add_bins: String::new(),
            add_renames: String::new(),
            add_patch: String::new(),
            add_ai_goal: "port-to-rust".into(),
            add_ai_instruction: String::new(),
            add_build_flag: false,
            dash_plan: None,
            dash_panes_per_tab: 6,
            dash_status: String::new(),
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
                        if report.driver_loaded {
                            "loaded"
                        } else {
                            "not loaded"
                        },
                        detected,
                        report.components.len(),
                        report.drift.len()
                    );
                    // read Copy fields BEFORE moving report.components (partial-move guard)
                    self.gpu_present = report.gpu_present;
                    self.driver_loaded = report.driver_loaded;
                    self.software_rendered = report.software_rendered;
                    self.gpu_count = report.gpu_count;
                    self.components = report.components;
                    self.drift = report.drift;
                }
                Event::Log {
                    component,
                    stream,
                    line,
                } => self.push_log(stream, format!("[{component}] {line}")),
                Event::Telemetry(t) => {
                    for g in &t.gpus {
                        let buf = self.util_history.entry(g.index).or_default();
                        if buf.len() >= SPARK_LEN {
                            buf.pop_front();
                        }
                        buf.push_back(g.util_pct as f32);
                    }
                    // audit fix (minor): drop history for GPU indices no longer present
                    // so stale sparklines don't linger or reappear.
                    let live: HashSet<u32> = t.gpus.iter().map(|g| g.index).collect();
                    self.util_history.retain(|k, _| live.contains(k));
                    self.telemetry = Some(t);
                }
                Event::GuardRefused { component, reason } => {
                    self.push_log(Stream::Stderr, format!("⛔ REFUSED {component}: {reason}"))
                }
                Event::StepFinished { result } => {
                    self.busy.remove(&result.component);
                    if result.status != OpStatus::NoHook {
                        let stream = if matches!(
                            result.status,
                            OpStatus::Failed | OpStatus::Refused | OpStatus::Incomplete
                        ) {
                            Stream::Stderr
                        } else {
                            Stream::Stdout
                        };
                        self.push_log(
                            stream,
                            format!(
                                "{} {:?} -> {:?}",
                                result.component, result.phase, result.status
                            ),
                        );
                    }
                }
                Event::RunFinished { .. } => {
                    let _ = self.cmd_tx.send(EngineCommand::Detect); // refresh after a run
                }
                Event::Dashboard { plan } => {
                    self.dash_status = format!("rendered {} tabs", plan.tabs.len());
                    self.dash_plan = Some(plan);
                }
                Event::DashboardDeployed { outcome } => {
                    self.dash_status = if outcome.applied {
                        format!("deployed -> {}", outcome.target.display())
                    } else {
                        format!("dry-run: would write {}", outcome.target.display())
                    };
                    for note in &outcome.notes {
                        self.push_log(Stream::Stdout, format!("[dashboard] {note}"));
                    }
                }
                _ => {}
            }
        }
    }

    fn push_log(&mut self, stream: Stream, text: String) {
        if self.log.len() >= self.log_cap {
            self.log.pop_front();
        }
        self.log.push_back(LogLine { stream, text });
    }

    fn dispatch(&mut self, cmd: EngineCommand, busy_id: Option<String>) {
        if let Some(id) = busy_id {
            self.busy.insert(id);
        }
        let _ = self.cmd_tx.send(cmd);
    }

    /// The worst drift severity recorded for a component, if any.
    fn drift_for<'a>(&'a self, id: &str) -> Option<&'a DriftItem> {
        self.drift
            .iter()
            .filter(|d| d.component == id)
            .min_by_key(|d| match d.severity {
                Severity::High => 0,
                Severity::Medium => 1,
                Severity::Low => 2,
            })
    }
}

impl eframe::App for EnvctlApp {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        self.drain();

        egui::TopBottomPanel::top("nav")
            .frame(
                egui::Frame::none()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("◆ envctl")
                            .size(18.0)
                            .strong()
                            .color(theme::ACCENT),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("GPU control center")
                            .size(11.0)
                            .color(theme::TEXT_FAINT),
                    );
                    ui.add_space(14.0);

                    for s in [
                        Screen::Dashboard,
                        Screen::Components,
                        Screen::Graph,
                        Screen::AddRepo,
                        Screen::Mesh,
                        Screen::Logs,
                        Screen::Settings,
                    ] {
                        self.nav_tab(ui, s);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(&self.header)
                                .size(12.0)
                                .color(theme::TEXT_MUTED),
                        );
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| match self.screen {
                Screen::Dashboard => self.dashboard(ui),
                Screen::Components => self.components_screen(ui),
                Screen::Graph => self.graph_screen(ui),
                Screen::AddRepo => self.add_repo_screen(ui),
                Screen::Mesh => self.mesh_screen(ui),
                Screen::Logs => self.logs_screen(ui),
                Screen::Settings => self.settings_screen(ui),
            });

        // The dedicated sampler thread emits Telemetry on its own cadence; the GUI
        // just sets the cadence (fast on Dashboard, slow elsewhere) + repaints.
        if self.screen == Screen::Dashboard {
            let cadence = if ctx.input(|i| i.focused) { 1000 } else { 3000 };
            self.tel.set_cadence(cadence);
            ctx.request_repaint_after(std::time::Duration::from_millis(cadence));
        } else {
            self.tel.set_cadence(10000);
        }
    }

    // Audit fix: on window close, tell the worker loop to shut down so it calls
    // ctrl.stop(); otherwise the telemetry sampler thread leaks and keeps
    // spawning nvidia-smi forever.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.cmd_tx.send(EngineCommand::Shutdown);
    }
}

impl EnvctlApp {
    fn nav_tab(&mut self, ui: &mut egui::Ui, s: Screen) {
        let active = self.screen == s;
        let text = if active {
            RichText::new(s.label()).color(theme::ACCENT_TEXT).strong()
        } else {
            RichText::new(s.label()).color(theme::TEXT_MUTED)
        };
        let btn = egui::Button::new(text)
            .fill(if active {
                theme::ACCENT
            } else {
                Color32::TRANSPARENT
            })
            .stroke(egui::Stroke::NONE)
            .rounding(egui::Rounding::same(7.0));
        if ui.add(btn).clicked() {
            self.screen = s;
        }
    }

    // ── Dashboard ───────────────────────────────────────────────────────────
    fn dashboard(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let t = self.telemetry.clone();

            // DriverNotActive: GPUs present but the kernel driver isn't loaded.
            if self.gpu_present && (!self.driver_loaded || self.software_rendered) {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.colored_label(
                        theme::WARN,
                        format!("⟳  {} NVIDIA GPU(s) present but the driver is not loaded — install nvidia-open and REBOOT to light them up.", self.gpu_count),
                    );
                });
                ui.add_space(8.0);
            }

            ui.label(theme::section("SYSTEM"));
            ui.add_space(4.0);
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                match &t {
                    Some(t) => {
                        ui.horizontal(|ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width() * 0.55, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    if let (Some(used), Some(total)) =
                                        (t.mem_used_mb, t.mem_total_mb)
                                    {
                                        let frac = used as f32 / total.max(1) as f32;
                                        ui.label(
                                            RichText::new(format!(
                                                "Memory   {used} / {total} MiB"
                                            ))
                                            .color(theme::TEXT),
                                        );
                                        ui.add(
                                            egui::ProgressBar::new(frac)
                                                .fill(theme::load_color(frac))
                                                .desired_height(10.0)
                                                .rounding(egui::Rounding::same(5.0)),
                                        );
                                    } else {
                                        ui.colored_label(theme::TEXT_FAINT, "memory: n/a");
                                    }
                                },
                            );
                            ui.add_space(16.0);
                            ui.vertical(|ui| {
                                let la = t.load_avg.unwrap_or(0.0);
                                ui.label(RichText::new("Load avg (1m)").color(theme::TEXT_MUTED));
                                ui.label(
                                    RichText::new(format!("{la:.2}"))
                                        .size(20.0)
                                        .strong()
                                        .color(theme::TEXT),
                                );
                            });
                        });
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().color(theme::ACCENT));
                            ui.colored_label(theme::TEXT_MUTED, "sampling system telemetry…");
                        });
                    }
                }
            });

            ui.add_space(12.0);
            ui.label(theme::section("GPUs"));
            ui.add_space(4.0);

            match &t {
                Some(t) if !t.gpus.is_empty() => {
                    for g in &t.gpus {
                        self.gpu_card(ui, g);
                    }
                }
                Some(_) => {
                    theme::card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.colored_label(theme::WARN, "⚠");
                            ui.colored_label(
                                theme::TEXT_MUTED,
                                "No live GPU telemetry — driver inactive. Install/REBOOT nvidia-open.",
                            );
                        });
                    });
                }
                None => {
                    theme::card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().color(theme::ACCENT));
                            ui.colored_label(theme::TEXT_MUTED, "sampling GPUs…");
                        });
                    });
                }
            }
        });
    }

    fn gpu_card(&self, ui: &mut egui::Ui, g: &envctl_engine::GpuSample) {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("GPU {}", g.index))
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.label(RichText::new(&g.name).color(theme::TEXT).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // sparkline of recent utilization
                    if let Some(hist) = self.util_history.get(&g.index) {
                        self.sparkline(ui, hist);
                    }
                });
            });
            ui.add_space(8.0);

            // Utilization
            let util = g.util_pct as f32 / 100.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Util").color(theme::TEXT_MUTED).size(12.0));
                ui.add(
                    egui::ProgressBar::new(util)
                        .text(RichText::new(format!("{}%", g.util_pct)).color(theme::TEXT))
                        .fill(theme::load_color(util))
                        .desired_height(14.0)
                        .rounding(egui::Rounding::same(6.0)),
                );
            });
            ui.add_space(4.0);

            // VRAM
            let vram = g.mem_used_mb as f32 / g.mem_total_mb.max(1) as f32;
            ui.horizontal(|ui| {
                ui.label(RichText::new("VRAM").color(theme::TEXT_MUTED).size(12.0));
                ui.add(
                    egui::ProgressBar::new(vram)
                        .text(
                            RichText::new(format!("{} / {} MiB", g.mem_used_mb, g.mem_total_mb))
                                .color(theme::TEXT),
                        )
                        .fill(theme::load_color(vram))
                        .desired_height(14.0)
                        .rounding(egui::Rounding::same(6.0)),
                );
            });
            ui.add_space(8.0);

            // temp + power stat chips
            ui.horizontal(|ui| {
                let temp_col = theme::load_color(g.temp_c as f32 / 90.0);
                stat_chip(ui, "TEMP", &format!("{}°C", g.temp_c), temp_col);
                match g.power_w {
                    Some(p) => stat_chip(ui, "POWER", &format!("{p} W"), theme::INFO),
                    None => stat_chip(ui, "POWER", "n/a", theme::TEXT_FAINT),
                }
            });
        });
    }

    /// Paint a small utilization sparkline from a 0..=100 history buffer.
    fn sparkline(&self, ui: &mut egui::Ui, hist: &VecDeque<f32>) {
        let (rect, _resp) = ui.allocate_exact_size(egui::vec2(120.0, 28.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, egui::Rounding::same(5.0), theme::BG);

        if hist.len() < 2 {
            return;
        }
        let n = hist.len();
        let pad = 3.0;
        let w = rect.width() - pad * 2.0;
        let h = rect.height() - pad * 2.0;
        let pts: Vec<egui::Pos2> = hist
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = rect.left() + pad + (i as f32 / (n - 1) as f32) * w;
                let y = rect.top() + pad + (1.0 - (v / 100.0).clamp(0.0, 1.0)) * h;
                egui::pos2(x, y)
            })
            .collect();
        let last = hist.back().copied().unwrap_or(0.0) / 100.0;
        let col = theme::load_color(last);
        painter.add(egui::Shape::line(pts.clone(), egui::Stroke::new(1.6, col)));
        if let Some(p) = pts.last() {
            painter.circle_filled(*p, 2.2, col);
        }
    }

    // ── Components ────────────────────────────────────────────────────────────
    fn components_screen(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Components").heading());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let any_missing = self.components.iter().any(|c| !c.detected);
                let install =
                    egui::Button::new(RichText::new("Install all missing").color(if any_missing {
                        theme::ACCENT_TEXT
                    } else {
                        theme::TEXT_FAINT
                    }))
                    .fill(if any_missing {
                        theme::ACCENT
                    } else {
                        theme::SURFACE
                    });
                if ui.add_enabled(any_missing, install).clicked() {
                    let missing: Vec<String> = self
                        .components
                        .iter()
                        .filter(|c| !c.detected)
                        .map(|c| c.id.clone())
                        .collect();
                    for id in &missing {
                        self.busy.insert(id.clone());
                    }
                    self.dispatch(
                        EngineCommand::Install {
                            targets: missing,
                            dry_run: false,
                        },
                        None,
                    );
                }
            });
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("🔍").color(theme::TEXT_MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("filter components…")
                    .desired_width(260.0),
            );
            if !self.filter.is_empty() && ui.button("✕").clicked() {
                self.filter.clear();
            }
        });
        ui.add_space(8.0);

        let needle = self.filter.trim().to_lowercase();
        let rows: Vec<ComponentState> = self
            .components
            .iter()
            .filter(|c| {
                needle.is_empty()
                    || c.id.to_lowercase().contains(&needle)
                    || c.name.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();

        if rows.is_empty() {
            ui.colored_label(theme::TEXT_FAINT, "no matching components");
            return;
        }

        // Snapshot the per-row presentation so we don't borrow self in the table closure.
        struct RowView {
            id: String,
            name: String,
            detected: bool,
            busy: bool,
            dot: Color32,
            status_text: String,
            health: String,
            health_col: Color32,
        }
        let views: Vec<RowView> = rows
            .iter()
            .map(|c| {
                let d = self.drift_for(&c.id);
                let (dot, status_text) = pill_for(c, d);
                let (health_col, health) = health_label(c, d);
                RowView {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    detected: c.detected,
                    busy: self.busy.contains(&c.id),
                    dot,
                    status_text,
                    health,
                    health_col,
                }
            })
            .collect();

        let mut to_install: Option<String> = None;
        let mut to_fix: Option<String> = None;

        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(120.0)) // status pill
            .column(Column::exact(150.0)) // id
            .column(Column::remainder().at_least(140.0)) // name
            .column(Column::exact(180.0)) // health/drift
            .column(Column::exact(150.0)) // actions
            .header(24.0, |mut h| {
                for title in ["STATUS", "ID", "NAME", "HEALTH", ""] {
                    h.col(|ui| {
                        ui.label(
                            RichText::new(title)
                                .size(11.0)
                                .strong()
                                .color(theme::TEXT_MUTED),
                        );
                    });
                }
            })
            .body(|mut body| {
                for v in &views {
                    body.row(34.0, |mut row| {
                        row.col(|ui| {
                            ui.label(RichText::new("●").color(v.dot));
                            ui.label(RichText::new(&v.status_text).size(12.0).color(v.dot));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(&v.id).monospace().color(theme::TEXT));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(&v.name).color(theme::TEXT_MUTED));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(&v.health).size(12.0).color(v.health_col));
                        });
                        row.col(|ui| {
                            if v.busy {
                                ui.add(egui::Spinner::new().color(theme::ACCENT));
                                ui.colored_label(theme::TEXT_FAINT, "working…");
                            } else if !v.detected {
                                let b = egui::Button::new(
                                    RichText::new("Install").color(theme::ACCENT_TEXT),
                                )
                                .fill(theme::ACCENT);
                                if ui.add(b).clicked() {
                                    to_install = Some(v.id.clone());
                                }
                            } else {
                                if ui.button("Fix").clicked() {
                                    to_fix = Some(v.id.clone());
                                }
                            }
                        });
                    });
                }
            });

        if let Some(id) = to_install {
            self.dispatch(
                EngineCommand::Install {
                    targets: vec![id.clone()],
                    dry_run: false,
                },
                Some(id),
            );
        }
        if let Some(id) = to_fix {
            // Audit fix: dry_run_default==true means "dry-run by default", so the
            // checked box must map directly to dry_run (was inverted, running Fix
            // for real by default and defeating the only GUI safety guard).
            self.dispatch(
                EngineCommand::Fix {
                    targets: vec![id.clone()],
                    dry_run: self.dry_run_default,
                },
                Some(id),
            );
        }
    }

    // ── Graph ─────────────────────────────────────────────────────────────────
    fn graph_screen(&mut self, ui: &mut egui::Ui) {
        use envctl_engine::graph;
        // Gather everything OWNED up front so the combo can borrow &mut self.graph_focus
        // without aliasing the registry borrow (immediate-mode: 1-frame lag is fine).
        let g = graph::analyze(self.geng.registry());
        let ids: Vec<String> = self.geng.registry().ids().cloned().collect();
        let focus = self.graph_focus.clone();
        let im = if focus.is_empty() {
            None
        } else {
            graph::impact(self.geng.registry(), &focus)
        };
        let paths = if focus.is_empty() {
            Vec::new()
        } else {
            graph::dependency_paths(self.geng.registry(), &focus)
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(theme::section("DEPENDENCY GRAPH"));
            ui.add_space(4.0);
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(format!(
                    "{} components · {} edges · {} groups",
                    g.nodes,
                    g.edges,
                    g.groups.len()
                ));
                ui.label(format!(
                    "{} roots · {} leaves · {} orphans",
                    g.roots.len(),
                    g.leaves.len(),
                    g.orphans.len()
                ));
                if let Some((id, n)) = &g.max_dependents {
                    ui.colored_label(theme::TEXT_MUTED, format!("most depended-on: {id} ({n})"));
                }
                ui.add_space(6.0);
                ui.label(RichText::new("critical path").color(theme::ACCENT_TEXT));
                ui.monospace(g.critical_path.join("  →  "));
            });

            ui.add_space(10.0);
            ui.label(theme::section("IMPACT — pick a component"));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let sel = if focus.is_empty() {
                    "(select)".to_string()
                } else {
                    focus.clone()
                };
                egui::ComboBox::from_id_salt("graph_focus")
                    .selected_text(sel)
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        for id in &ids {
                            ui.selectable_value(&mut self.graph_focus, id.clone(), id.as_str());
                        }
                    });
            });

            if let Some(im) = &im {
                ui.add_space(6.0);
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(format!("install {}", im.component)).color(theme::HEALTHY),
                    );
                    ui.monospace(format!(
                        "pulls in ({}): {}",
                        im.install_closure.len(),
                        im.install_closure.join("  →  ")
                    ));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("reset {} --cascade", im.component))
                            .color(theme::WARN),
                    );
                    ui.monospace(format!(
                        "also removes ({}): {}",
                        im.cascade_removes.len(),
                        if im.cascade_removes.is_empty() {
                            "(none)".into()
                        } else {
                            im.cascade_removes.join(", ")
                        }
                    ));
                    ui.add_space(6.0);
                    ui.label(RichText::new("why it's needed (root → it)").color(theme::TEXT_MUTED));
                    for p in &paths {
                        ui.monospace(format!("  {}", p.join("  →  ")));
                    }
                });
            }
        });
    }

    // ── Add Repo ──────────────────────────────────────────────────────────────
    fn add_repo_screen(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Add a repo as a managed component").heading());
        ui.add_space(10.0);

        theme::inset().show(ui, |ui| {
            ui.set_max_width(620.0);
            egui::Grid::new("addrepo")
                .num_columns(2)
                .spacing([14.0, 12.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Git URL").color(theme::TEXT_MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.add_url)
                            .hint_text("https://github.com/owner/repo")
                            .desired_width(380.0),
                    );
                    ui.end_row();

                    ui.label(RichText::new("ID").color(theme::TEXT_MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.add_id)
                            .hint_text("short-unique-id")
                            .desired_width(380.0),
                    );
                    ui.end_row();

                    ui.label(RichText::new("Ref").color(theme::TEXT_MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.add_ref).hint_text("branch/tag/sha (optional)").desired_width(380.0));
                    ui.end_row();

                    ui.label(RichText::new("Build cmd").color(theme::TEXT_MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.add_build).hint_text("(blank = auto-detect)").desired_width(380.0));
                    ui.end_row();

                    ui.label(RichText::new("Strategy").color(theme::TEXT_MUTED));
                    egui::ComboBox::from_id_salt("strategy")
                        .selected_text(&self.add_strategy)
                        .show_ui(ui, |ui| {
                            for s in ["as-is", "cherry-pick", "rename", "refactor"] {
                                ui.selectable_value(&mut self.add_strategy, s.to_string(), s);
                            }
                        });
                    ui.end_row();
                });

            // strategy-specific fields
            ui.add_space(8.0);
            match self.add_strategy.as_str() {
                "cherry-pick" => {
                    ui.label(RichText::new("Bins (comma-separated file-stems)").color(theme::TEXT_MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.add_bins).hint_text("rg, foo").desired_width(420.0));
                }
                "rename" => {
                    ui.label(RichText::new("Renames (old=new, comma-separated)").color(theme::TEXT_MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.add_renames).hint_text("rg=rgx").desired_width(420.0));
                }
                "refactor" => {
                    ui.label(RichText::new("Patch cmd (leave blank for AI refactor)").color(theme::TEXT_MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.add_patch).desired_width(420.0));
                    if self.add_patch.trim().is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("AI goal").color(theme::TEXT_MUTED));
                            egui::ComboBox::from_id_salt("ai_goal")
                                .selected_text(&self.add_ai_goal)
                                .show_ui(ui, |ui| {
                                    for g in ["port-to-rust", "cherry-pick-to-crate", "rename-for-synergy", "custom"] {
                                        ui.selectable_value(&mut self.add_ai_goal, g.to_string(), g);
                                    }
                                });
                        });
                        ui.add(egui::TextEdit::singleline(&mut self.add_ai_instruction).hint_text("extra instruction (optional)").desired_width(420.0));
                        ui.colored_label(theme::WARN, "envctl invokes the agent NON-INTERACTIVELY in the clone; it never auto-commits or pushes.");
                    }
                }
                _ => {}
            }

            ui.add_space(10.0);
            ui.checkbox(&mut self.add_build_flag, "Build now (run the upstream build / AI agent + install) — off = preview only");

            ui.add_space(12.0);
            let ready = !self.add_url.trim().is_empty() && !self.add_id.trim().is_empty();
            ui.horizontal(|ui| {
                if ui.add_enabled(ready, egui::Button::new("Validate (dry-run)")).clicked() {
                    self.dispatch(self.add_repo_cmd(true), None);
                    self.screen = Screen::Logs;
                }
                let label = if self.add_build_flag { "Build + Register" } else { "Register (preview)" };
                let reg = egui::Button::new(RichText::new(label).color(if ready { theme::ACCENT_TEXT } else { theme::TEXT_FAINT }))
                    .fill(if ready { theme::ACCENT } else { theme::SURFACE });
                if ui.add_enabled(ready, reg).clicked() {
                    self.dispatch(self.add_repo_cmd(false), None);
                    self.screen = Screen::Logs;
                }
            });
        });

        ui.add_space(10.0);
        ui.colored_label(
            theme::TEXT_FAINT,
            "Acquire + detect + preview by default. 'Build now' clones, builds from source, installs into ~/.local/bin, and registers a managed drop-in.",
        );
    }

    fn add_repo_cmd(&self, dry_run: bool) -> EngineCommand {
        let opt = |s: &str| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };
        let strategy = match self.add_strategy.as_str() {
            "cherry-pick" => BuildStrategy::CherryPick {
                bins: split_csv(&self.add_bins),
            },
            "rename" => BuildStrategy::Rename {
                renames: split_csv(&self.add_renames)
                    .into_iter()
                    .filter_map(|p| {
                        p.split_once('=').map(|(a, b)| RenameRule {
                            from: a.trim().into(),
                            to: b.trim().into(),
                        })
                    })
                    .collect(),
            },
            "refactor" => BuildStrategy::Refactor {
                refactor: if let Some(cmd) = opt(&self.add_patch) {
                    Refactor::Patch { command: cmd }
                } else {
                    Refactor::Ai {
                        agent: None,
                        goal: match self.add_ai_goal.as_str() {
                            "port-to-rust" => RefactorGoal::PortToRust,
                            "cherry-pick-to-crate" => RefactorGoal::CherryPickToCrate,
                            "rename-for-synergy" => RefactorGoal::RenameForSynergy,
                            _ => RefactorGoal::Custom,
                        },
                        instruction: opt(&self.add_ai_instruction),
                    }
                },
            },
            _ => BuildStrategy::AsIs,
        };
        EngineCommand::AddRepo {
            spec: AddRepoSpec {
                id: self.add_id.trim().to_string(),
                git_url: self.add_url.trim().to_string(),
                git_ref: opt(&self.add_ref),
                build_cmd: self.add_build.trim().to_string(),
                strategy,
                allow_build: self.add_build_flag,
                ..Default::default()
            },
            dry_run,
        }
    }

    // ── Logs ──────────────────────────────────────────────────────────────────
    fn logs_screen(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Console").heading());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    self.log.clear();
                }
                ui.colored_label(theme::TEXT_FAINT, format!("{} lines", self.log.len()));
            });
        });
        ui.add_space(6.0);

        egui::Frame::none()
            .fill(theme::BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .rounding(egui::Rounding::same(8.0))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.log.is_empty() {
                            ui.colored_label(theme::TEXT_FAINT, "no output yet");
                        }
                        for l in &self.log {
                            ui.label(
                                RichText::new(&l.text)
                                    .monospace()
                                    .size(12.5)
                                    .color(log_color(l)),
                            );
                        }
                    });
            });
    }

    // ── Settings ──────────────────────────────────────────────────────────────
    /// meta mission-control dashboard parity: render the zellij layout from
    /// `.meta.yaml` (read-only) and deploy it (gated by the dry-run toggle, like
    /// the other mutations). Drives the IDENTICAL Engine API the CLI uses via
    /// EngineCommand::Dashboard / DeployDashboard.
    fn mesh_screen(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Mesh — meta mission-control").heading());
        ui.add_space(6.0);
        ui.colored_label(
            theme::TEXT_FAINT,
            "Render a zellij dashboard layout from .meta.yaml (tabs by tag, pane-per-repo).",
        );
        ui.add_space(10.0);

        let start = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let spec = DashboardSpec {
            panes_per_tab: self.dash_panes_per_tab.max(1),
            ..DashboardSpec::default()
        };

        ui.horizontal(|ui| {
            ui.label("Panes per tab:");
            ui.add(egui::DragValue::new(&mut self.dash_panes_per_tab).range(1..=24));
        });
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("Render").color(theme::ACCENT_TEXT))
                        .fill(theme::ACCENT),
                )
                .clicked()
            {
                let _ = self.cmd_tx.send(EngineCommand::Dashboard {
                    start: start.clone(),
                    meta_file: None,
                    spec: spec.clone(),
                });
            }
            // Deploy is a mutation: dry-run unless the dry-run-default toggle is OFF
            // (mirrors how Fix gates its --apply in this GUI).
            let dry_run = self.dry_run_default;
            let label = if dry_run {
                "Deploy (dry-run)"
            } else {
                "Deploy (apply)"
            };
            if ui.button(label).clicked() {
                let _ = self.cmd_tx.send(EngineCommand::DeployDashboard {
                    start: start.clone(),
                    meta_file: None,
                    spec: spec.clone(),
                    dry_run,
                    force: false,
                });
            }
        });

        if !self.dash_status.is_empty() {
            ui.add_space(8.0);
            ui.colored_label(theme::TEXT_MUTED, &self.dash_status);
        }

        if let Some(plan) = &self.dash_plan {
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);
            ui.label(RichText::new(format!("{} ({} tabs)", plan.name, plan.tabs.len())).strong());
            ui.colored_label(
                theme::TEXT_FAINT,
                format!("target: {}", plan.target.display()),
            );
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut plan.kdl.as_str())
                            .code_editor()
                            .desired_width(f32::INFINITY),
                    );
                });
        }
    }

    fn settings_screen(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Settings").heading());
        ui.add_space(10.0);
        theme::inset().show(ui, |ui| {
            ui.set_max_width(560.0);
            ui.checkbox(
                &mut self.dry_run_default,
                "Destructive ops dry-run by default",
            );
            ui.colored_label(
                theme::TEXT_FAINT,
                "When on, Fix runs in dry-run mode unless explicitly forced.",
            );
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(14.0);
            if ui
                .add(
                    egui::Button::new(RichText::new("Re-detect").color(theme::ACCENT_TEXT))
                        .fill(theme::ACCENT),
                )
                .clicked()
            {
                let _ = self.cmd_tx.send(EngineCommand::Detect);
            }
            ui.colored_label(
                theme::TEXT_FAINT,
                "Re-scan the environment and refresh drift.",
            );
        });
    }
}

// ── small free helpers (no &self borrow) ──────────────────────────────────────

/// A compact "LABEL value" stat chip on a faint surface.
fn stat_chip(ui: &mut egui::Ui, label: &str, value: &str, value_col: Color32) {
    egui::Frame::none()
        .fill(theme::BG)
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(10.0).color(theme::TEXT_FAINT));
                ui.label(RichText::new(value).strong().color(value_col));
            });
        });
}

/// Status pill color + short text for a component, factoring in drift severity.
fn pill_for(c: &ComponentState, drift: Option<&DriftItem>) -> (Color32, String) {
    if !c.detected {
        let col = drift.map(|d| sev_color(d.severity)).unwrap_or(theme::WARN);
        return (col, "missing".into());
    }
    if let Some(d) = drift {
        return (sev_color(d.severity), "drift".into());
    }
    match c.healthy {
        Some(false) => (theme::DANGER, "unhealthy".into()),
        Some(true) => (theme::HEALTHY, "healthy".into()),
        None => (theme::HEALTHY, "present".into()),
    }
}

/// A human label for the health/drift cell.
fn health_label(c: &ComponentState, drift: Option<&DriftItem>) -> (Color32, String) {
    // returns (text, color) swapped intentionally? -> keep (color, text)
    if let Some(d) = drift {
        let kind = match d.kind {
            DriftKind::Missing => "missing",
            DriftKind::Unhealthy => "unhealthy",
            DriftKind::WiringMissing => "wiring missing",
            DriftKind::DriverInactive => "driver inactive",
        };
        return (
            sev_color(d.severity),
            format!("{kind} · {}", d.suggested_verb),
        );
    }
    if !c.detected {
        return (theme::WARN, "not installed".into());
    }
    match c.healthy {
        Some(false) => (theme::DANGER, "verify failed".into()),
        Some(true) => (theme::HEALTHY, "ok".into()),
        None => (theme::TEXT_MUTED, "—".into()),
    }
}

fn sev_color(sev: Severity) -> Color32 {
    match sev {
        Severity::High => theme::DANGER,
        Severity::Medium => theme::WARN,
        Severity::Low => theme::INFO,
    }
}

/// Color a console line by stream then by a cheap level heuristic.
fn log_color(l: &LogLine) -> Color32 {
    if l.stream == Stream::Stderr {
        return theme::DANGER;
    }
    let lower = l.text.to_lowercase();
    if lower.contains("refused") || lower.contains("error") || lower.contains("fail") {
        theme::DANGER
    } else if lower.contains("warn") || lower.contains("skip") {
        theme::WARN
    } else if lower.contains("-> ok") || lower.contains("ok\"") || lower.contains("done") {
        theme::HEALTHY
    } else {
        theme::TEXT
    }
}

/// Split a comma/whitespace-separated list into trimmed non-empty tokens.
fn split_csv(s: &str) -> Vec<String> {
    s.split([',', ' ', '\n'])
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(|x| x.to_string())
        .collect()
}
