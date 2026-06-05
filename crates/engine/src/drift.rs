//! Drift = a pure diff between the detected `EnvReport` and the manifest's
//! desired state. Each `DriftItem` carries a severity + a `suggested_verb` so the
//! CLI/GUI can tell the user not just *what* is off but *what to run*. Pure and
//! deterministic — unit-tested against constructed reports.
use crate::model::{DriftItem, DriftKind, EnvReport, Registry, Severity};
use serde::{Deserialize, Serialize};

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
            // AUDIT-FIX (minor #16): when the whole-box DriverInactive block
            // below will already emit nvidia-open (GPUs present, driver not
            // live), suppress the generic Missing entry here so nvidia-open
            // appears exactly once with the driver-focused verb.
            let driver_inactive_owns =
                c.id == "nvidia-open" && report.gpu_present && !report.driver_loaded;
            if installable && !driver_inactive_owns {
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

/// Counts of drift items by severity. Pure, non-printing; the CLI/GUI render it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftSummary {
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub total: usize,
}

impl DriftSummary {
    /// Fold drift items into per-severity counts. Deterministic.
    pub fn from_items(items: &[DriftItem]) -> DriftSummary {
        let mut s = DriftSummary::default();
        for d in items {
            match d.severity {
                Severity::High => s.high += 1,
                Severity::Medium => s.medium += 1,
                Severity::Low => s.low += 1,
            }
            s.total += 1;
        }
        s
    }
}

impl std::fmt::Display for DriftSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Drift: {} high, {} medium, {} low ({} total)",
            self.high, self.medium, self.low, self.total
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(severity: Severity) -> DriftItem {
        DriftItem {
            component: "c".into(),
            kind: DriftKind::Missing,
            severity,
            suggested_verb: "envctl install c".into(),
            detail: "test".into(),
        }
    }

    #[test]
    fn summary_counts_by_severity() {
        let items = vec![
            item(Severity::High),
            item(Severity::High),
            item(Severity::Medium),
            item(Severity::Low),
            item(Severity::Low),
            item(Severity::Low),
        ];
        let s = DriftSummary::from_items(&items);
        assert_eq!(
            s,
            DriftSummary { high: 2, medium: 1, low: 3, total: 6 }
        );
    }

    #[test]
    fn summary_empty_is_zero() {
        let s = DriftSummary::from_items(&[]);
        assert_eq!(s, DriftSummary::default());
        assert_eq!(s.total, 0);
    }

    #[test]
    fn summary_display_wording() {
        let s = DriftSummary { high: 1, medium: 0, low: 2, total: 3 };
        assert_eq!(s.to_string(), "Drift: 1 high, 0 medium, 2 low (3 total)");
    }
}
