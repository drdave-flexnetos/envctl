//! Concrete `HookRunner` impls. `ProcessRunner` spawns the wrapped bash via
//! `std::process`; `DryRunRunner` returns `DryRun` without executing. Both are
//! `Send + Sync` (no interior mutability), which is what lets a `Box<dyn
//! HookRunner>` sit inside the `Send + Sync + 'static` Engine.
//!
//! NOTE (Phase 2): this skeleton uses `Command::status()`, which INHERITS stdio
//! — it does not yet line-stream stdout/stderr as `Event::Log`, nor tee to an
//! on-disk run log, nor enforce a per-hook timeout / `catch_unwind`. Those are
//! the Phase-2 deliverables (see ROADMAP.md). Behavior is otherwise correct.
use crate::component::{Hook, HookRunner, Phase};
use crate::model::{OpResult, OpStatus};
use std::process::Command;

#[derive(Default)]
pub struct ProcessRunner;

impl HookRunner for ProcessRunner {
    fn run(&self, comp: &str, phase: Phase, hook: &Hook, dry_run: bool) -> OpResult {
        if dry_run {
            return OpResult {
                component: comp.into(),
                phase,
                status: OpStatus::DryRun,
                exit_code: None,
                duration_ms: 0,
                message: "dry-run".into(),
                dry_run,
            };
        }

        let mut cmd = build_command(hook);

        // Read-only probes (Detect/Verify) CAPTURE stdout/stderr — we only need
        // the exit code, and leaking `command -v cargo` / `--version` output would
        // corrupt the CLI's table and the `--json` stream. Action phases
        // (Install/Fix/Remove) keep inherited stdio so the user sees progress
        // (live Event::Log streaming + on-disk tee is the Phase-2 upgrade).
        let capture = matches!(phase, Phase::Detect | Phase::Verify);

        let outcome = if capture {
            cmd.output().map(|o| (o.status, String::from_utf8_lossy(&o.stderr).trim().to_string()))
        } else {
            cmd.status().map(|s| (s, String::new()))
        };

        match outcome {
            Ok((st, stderr)) => {
                let status = if st.success() {
                    OpStatus::Ok
                } else {
                    OpStatus::Failed
                };
                OpResult {
                    component: comp.into(),
                    phase,
                    status,
                    exit_code: st.code(),
                    duration_ms: 0,
                    message: if status == OpStatus::Failed { stderr } else { String::new() },
                    dry_run,
                }
            }
            Err(e) => OpResult {
                component: comp.into(),
                phase,
                status: OpStatus::Failed,
                exit_code: None,
                duration_ms: 0,
                message: format!("spawn failed: {e}"),
                dry_run,
            },
        }
    }
}

/// Translate a Hook into a ready-to-spawn `Command` (no shell for `Command`;
/// `bash -lc` for `Script`; `bash <path>` for `ShippedScript`).
fn build_command(hook: &Hook) -> Command {
    match hook {
        Hook::Command {
            command,
            args,
            env,
            needs_sudo,
        } => {
            let mut c = if *needs_sudo {
                let mut s = Command::new("sudo");
                s.arg(command);
                s
            } else {
                Command::new(command)
            };
            c.args(args);
            for (k, v) in env {
                c.env(k, v);
            }
            c
        }

        Hook::Script {
            script,
            path,
            env,
            needs_sudo,
            login_shell,
        } => {
            let shell_flag = if *login_shell { "-lc" } else { "-c" };
            // The command string for `bash -lc` is either the inline script or,
            // when a `path` is given, the path itself (bash executes it). The old
            // `"$0"` indirection was wrong (it left $0 unbound) — fixed here.
            let body = match path {
                Some(p) => p.clone(),
                None => script.clone(),
            };
            let mut c = if *needs_sudo {
                let mut s = Command::new("sudo");
                s.arg("bash").arg(shell_flag);
                s
            } else {
                let mut s = Command::new("bash");
                s.arg(shell_flag);
                s
            };
            c.arg(body);
            for (k, v) in env {
                c.env(k, v);
            }
            c
        }

        Hook::ShippedScript {
            path,
            args,
            needs_sudo,
        } => {
            let mut c = if *needs_sudo {
                let mut s = Command::new("sudo");
                s.arg("bash").arg(path);
                s
            } else {
                let mut s = Command::new("bash");
                s.arg(path);
                s
            };
            c.args(args);
            c
        }
    }
}

/// Never executes; reports every hook as `DryRun`. Used by tests + previews.
pub struct DryRunRunner;

impl HookRunner for DryRunRunner {
    fn run(&self, comp: &str, phase: Phase, _h: &Hook, _d: bool) -> OpResult {
        OpResult {
            component: comp.into(),
            phase,
            status: OpStatus::DryRun,
            exit_code: None,
            duration_ms: 0,
            message: "dry-run".into(),
            dry_run: true,
        }
    }
}
