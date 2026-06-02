//! Drift = a pure diff between the detected `EnvReport` and the manifest's
//! desired state. Each `DriftItem` carries a severity + a `suggested_verb` so the
//! CLI/GUI can tell the user not just *what* is off but *what to run*. Pure and
//! deterministic — unit-tested against constructed reports.
use crate::model::{DriftItem, DriftKind, EnvReport, Registry, Severity};

pub fn compute(report: &EnvReport, reg: &Registry) -> Vec<DriftItem> {
    let mut items = Vec::new();

    for c in &report.components {
        // GPU-skipped components on a non-NVIDIA box aren't drift — they're N/A.
        if c.note.contains("no GPU") {
            continue;
        }
        let comp = reg.get(&c.id);
        let is_meta = c.id.starts_with("group-") || c.id.starts_with("meta-");

        if !c.detected {
            // Only flag as Missing if there's actually an install path (an
            // install hook or owned wiring). Pure "operation" components
            // (e.g. boot-repair ops, which only have fix) are not "missing".
            // AUDIT-FIX (#4): also count system-scope wiring footprints
            // (path_entries/apt_repos/nix_conf_lines/cdi_specs/alternatives), not
            // just shell_rc — a component whose only footprint is system wiring was
            // previously invisible to drift.
            let installable = comp
                .map(|x| {
                    let w = &x.wiring;
                    x.install.is_some()
                        || !w.shell_rc.is_empty()
                        || !w.path_entries.is_empty()
                        || !w.apt_repos.is_empty()
                        || !w.nix_conf_lines.is_empty()
                        || !w.cdi_specs.is_empty()
                        || !w.alternatives.is_empty()
                })
                .unwrap_or(false);
            if installable {
                items.push(DriftItem {
                    component: c.id.clone(),
                    kind: DriftKind::Missing,
                    severity: if is_meta { Severity::Low } else { Severity::Medium },
                    suggested_verb: format!("envctl install {}", c.id),
                    detail: "declared but not installed".into(),
                });
            }
            continue;
        }

        if c.healthy == Some(false) {
            // Defense-in-depth: NEVER auto-suggest `--apply` for a destructive
            // component — its fix is a confirm-gated, dry-run-first operation.
            let destructive = comp.map(|x| x.destructive).unwrap_or(false);
            let (verb, detail) = if destructive {
                (
                    format!("envctl auto-fix {}   (dry-run first; review, then --apply)", c.id),
                    "verify failed — destructive fix, run dry-run and confirm before --apply",
                )
            } else {
                (format!("envctl auto-fix {} --apply", c.id), "installed but verify failed")
            };
            items.push(DriftItem {
                component: c.id.clone(),
                kind: DriftKind::Unhealthy,
                severity: Severity::High,
                suggested_verb: verb,
                detail: detail.into(),
            });
        }

        let declares_wiring = comp.map(|x| !x.wiring.shell_rc.is_empty()).unwrap_or(false);
        if declares_wiring && !c.wiring_present {
            items.push(DriftItem {
                component: c.id.clone(),
                kind: DriftKind::WiringMissing,
                severity: Severity::Low,
                suggested_verb: format!("envctl install {}", c.id),
                detail: "detected, but envctl's shell-rc wiring block is absent".into(),
            });
        }
    }

    // Whole-box GPU precondition: cards present but driver not live.
    if report.gpu_present && !report.driver_loaded {
        items.push(DriftItem {
            component: "nvidia-open".into(),
            kind: DriftKind::DriverInactive,
            severity: Severity::High,
            suggested_verb: "envctl install nvidia-open  (then REBOOT)".into(),
            detail: "GPUs present but the kernel driver is not loaded (software-rendered)".into(),
        });
    }

    // Highest severity first for display.
    items.sort_by_key(|d| match d.severity {
        Severity::High => 0,
        Severity::Medium => 1,
        Severity::Low => 2,
    });
    items
}
