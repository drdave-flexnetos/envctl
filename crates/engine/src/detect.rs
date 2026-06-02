//! auto-detect: build an `EnvReport` read-only. NEVER writes.
//!
//! GPU detection is layered so it works on the documented first-boot reality
//! (software-rendered, no driver yet):
//!   Tier 0  PCI floor — scan /sys/bus/pci/devices for vendor 0x10de + display
//!           class 0x03xx. Authoritative count, works with NO driver.
//!   Tier 1  /proc/driver/nvidia/version — driver_loaded + version.
//!   Tier 2  nvidia-smi / nvcc — names, driver/CUDA versions (enrichment only).
//! `software_rendered = pci_sees_nvidia && !driver_loaded` → the GUI shows a
//! "reboot to load nvidia-open" hint instead of a false "no GPU".
use crate::component::{HookRunner, Phase};
use crate::event::{Event, EventSink};
use crate::model::{ComponentState, EnvReport, OpStatus, Registry, ToolState};
use std::path::Path;
use std::process::Command;

/// Tools probed for version (curated to ones that print-and-exit; avoids hangs).
const PROBE_TOOLS: &[&str] = &[
    "cargo", "rustc", "bun", "node", "nvcc", "nvidia-smi", "nix", "gh", "uv", "wasmer", "podman",
    "python3", "git", "curl",
];

pub fn run(reg: &Registry, runner: &dyn HookRunner, sink: &EventSink) -> anyhow::Result<EnvReport> {
    let mut report = EnvReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        ..Default::default()
    };

    // ---- host (sysinfo) ----
    let sys = sysinfo::System::new_all();
    report.kernel = sysinfo::System::kernel_version();
    report.os = sysinfo::System::long_os_version();
    report.cpu_model = sys.cpus().first().map(|c| c.brand().trim().to_string());
    report.cpu_threads = sys.cpus().len();
    report.mem_total_mb = sys.total_memory() / 1024 / 1024;

    // ---- GPU: Tier 0 PCI floor (driver-independent) ----
    report.gpu_count = pci_nvidia_count();
    report.gpu_present = report.gpu_count > 0;

    // ---- GPU: Tier 1 driver state ----
    report.driver_loaded = Path::new("/proc/driver/nvidia/version").exists();
    report.open_kernel_module = nvidia_open_module();
    report.software_rendered = report.gpu_present && !report.driver_loaded;

    // ---- GPU: Tier 2 enrichment (names/versions; best-effort) ----
    report.gpus = nvidia_smi_names();
    if report.gpus.is_empty() && report.gpu_present {
        report.gpus = lspci_nvidia_names();
    }
    report.driver_version = nvidia_smi_driver_version();
    report.cuda_version = nvcc_cuda_version();

    // ---- installed tool versions (which + --version) ----
    for t in PROBE_TOOLS {
        let path = which::which(t).ok().map(|p| p.display().to_string());
        let version = path.as_ref().and_then(|_| tool_version(t));
        if path.is_some() {
            report.tools.push(ToolState {
                name: (*t).to_string(),
                path,
                version,
            });
        }
    }

    // ---- per-component detect (+ verify if detected) + wiring presence ----
    for comp in reg.ordered() {
        let mut st = ComponentState {
            id: comp.id.clone(),
            name: comp.name.clone(),
            detected: false,
            healthy: None,
            wiring_present: wiring_present(comp),
            note: String::new(),
        };
        if comp.gpu_required && !report.gpu_present {
            st.note = "skipped: no GPU".into();
            report.components.push(st);
            continue;
        }
        if let Some(h) = comp.detect.as_ref() {
            st.detected = runner.run(&comp.id, Phase::Detect, h, false).status == OpStatus::Ok;
        }
        if st.detected {
            if let Some(h) = comp.verify.as_ref() {
                st.healthy =
                    Some(runner.run(&comp.id, Phase::Verify, h, false).status == OpStatus::Ok);
            }
        }
        report.components.push(st);
    }

    // Drift = diff(detected, desired) with suggested verbs.
    let drift = crate::drift::compute(&report, reg);
    report.drift = drift;

    sink.emit(Event::Report {
        report: report.clone(),
    });
    Ok(report)
}

// ---- GPU helpers ----

/// Count PCI functions with NVIDIA vendor 0x10de and a display class 0x03xx.
/// `pub(crate)` so the executor can resolve GPU presence for `RunContext`.
pub(crate) fn pci_nvidia_count() -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir("/sys/bus/pci/devices") {
        for e in rd.flatten() {
            let vendor = std::fs::read_to_string(e.path().join("vendor")).unwrap_or_default();
            let class = std::fs::read_to_string(e.path().join("class")).unwrap_or_default();
            if vendor.trim() == "0x10de" && class.trim().starts_with("0x03") {
                n += 1;
            }
        }
    }
    n
}

fn nvidia_open_module() -> bool {
    // The open kernel modules report a free license; the proprietary one does not.
    if let Ok(out) = Command::new("modinfo").args(["-F", "license", "nvidia"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
            return s.contains("mit") || s.contains("gpl");
        }
    }
    false
}

fn nvidia_smi_names() -> Vec<String> {
    run_capture("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"])
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

fn nvidia_smi_driver_version() -> Option<String> {
    run_capture(
        "nvidia-smi",
        &["--query-gpu=driver_version", "--format=csv,noheader"],
    )
    .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
    .filter(|s| !s.is_empty())
}

fn lspci_nvidia_names() -> Vec<String> {
    run_capture("bash", &["-lc", "lspci | grep -iE 'vga|3d' | grep -i nvidia"])
        .map(|s| {
            s.lines()
                .filter_map(|l| l.split_once(": ").map(|(_, n)| n.trim().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn nvcc_cuda_version() -> Option<String> {
    let out = run_capture("nvcc", &["--version"])?;
    // line: "Cuda compilation tools, release 13.3, V13.3.xx"
    out.split("release ")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
}

// ---- tool/version helpers ----

fn tool_version(tool: &str) -> Option<String> {
    let out = run_capture(tool, &["--version"]).or_else(|| run_capture(tool, &["-V"]))?;
    out.lines().next().map(|l| l.trim().to_string())
}

fn run_capture(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

/// True iff every shell_rc marker block this component owns is present in its file.
fn wiring_present(comp: &crate::component::Component) -> bool {
    if comp.wiring.shell_rc.is_empty() {
        return false;
    }
    comp.wiring.shell_rc.iter().all(|blk| {
        let file = match blk.file.strip_prefix("~/") {
            Some(rest) => match std::env::var("HOME") {
                Ok(h) => format!("{h}/{rest}"),
                Err(_) => return false,
            },
            None => blk.file.clone(),
        };
        // Suffix-agnostic: the wizard writes the same blocks as
        // "BEGIN <marker> (added by yazelix-setup.sh)"; envctl writes
        // "(added by envctl)". Match the marker regardless of who wrote it.
        std::fs::read_to_string(&file)
            .map(|t| t.contains(&format!("BEGIN {}", blk.marker)))
            .unwrap_or(false)
    })
}
