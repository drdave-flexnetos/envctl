//! Concrete `HookRunner` impls. `ProcessRunner` spawns the wrapped bash via
//! `std::process`; `DryRunRunner` returns `DryRun` without executing. Both are
//! `Send + Sync` (no interior mutability).
//!
//! Phase 2: action phases (Install/Fix/Remove) now LINE-STREAM stdout/stderr as
//! `Event::Log` (so the CLI/GUI show progress live during a long apt/nix/CUDA run)
//! AND tee every line to `~/.local/state/envctl/envctl.log` (the analogue of
//! `~/yazelix-setup.log`, survives a crash). Read-only phases (Detect/Verify)
//! capture quietly — only the exit code matters, and leaking their output would
//! corrupt the CLI table / `--json`. Every hook is bounded by a per-phase timeout
//! (the process is killed on expiry) so a stuck installer can't wedge the worker.
use crate::component::{Hook, HookRunner, Phase};
use crate::event::{Event, EventSink, Stream};
use crate::model::{OpResult, OpStatus};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct ProcessRunner;

impl HookRunner for ProcessRunner {
    fn run(&self, comp: &str, phase: Phase, hook: &Hook, dry_run: bool, sink: &EventSink) -> OpResult {
        if dry_run {
            return mk(comp, phase, OpStatus::DryRun, None, "dry-run");
        }

        // Action phases stream + tee; read-only probes capture quietly.
        let streaming = matches!(phase, Phase::Install | Phase::Fix | Phase::Remove);

        let mut cmd = build_command(hook);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return mk(comp, phase, OpStatus::Failed, None, &format!("spawn failed: {e}")),
        };

        let log = Arc::new(Mutex::new(if streaming { open_run_log() } else { None }));
        let tail = Arc::new(Mutex::new(Vec::<String>::new())); // last stderr lines for the message

        let h_out = child.stdout.take().map(|r| {
            pump(r, comp.to_string(), Stream::Stdout, streaming, sink.clone(), log.clone(), None)
        });
        let h_err = child.stderr.take().map(|r| {
            pump(r, comp.to_string(), Stream::Stderr, streaming, sink.clone(), log.clone(), Some(tail.clone()))
        });

        let (code, success, timed_out) = wait_timeout(&mut child, timeout_for(phase));
        if let Some(h) = h_out {
            let _ = h.join();
        }
        if let Some(h) = h_err {
            let _ = h.join();
        }

        if timed_out {
            return mk(comp, phase, OpStatus::Failed, code, &format!("timed out after {}s", timeout_for(phase).as_secs()));
        }
        if success {
            mk(comp, phase, OpStatus::Ok, code, "")
        } else {
            let msg = tail.lock().map(|v| v.join("\n")).unwrap_or_default();
            mk(comp, phase, OpStatus::Failed, code, truncate(&msg, 4000))
        }
    }
}

fn mk(comp: &str, phase: Phase, status: OpStatus, exit_code: Option<i32>, message: &str) -> OpResult {
    OpResult {
        component: comp.into(),
        phase,
        status,
        exit_code,
        duration_ms: 0,
        message: message.into(),
        dry_run: status == OpStatus::DryRun,
    }
}

fn timeout_for(phase: Phase) -> Duration {
    match phase {
        Phase::Detect | Phase::Verify => Duration::from_secs(60),
        Phase::Install => Duration::from_secs(1800), // big apt/nix/CUDA builds
        Phase::Fix | Phase::Remove => Duration::from_secs(900),
    }
}

/// Reader thread: line-stream a child stream. Emits `Event::Log` + tees to the
/// run log for action phases; always keeps a tail of stderr for the failure msg.
/// Uses lossy UTF-8 so non-UTF-8 build output can't kill the thread.
fn pump<R: Read + Send + 'static>(
    reader: R,
    comp: String,
    stream: Stream,
    streaming: bool,
    sink: EventSink,
    log: Arc<Mutex<Option<File>>>,
    tail: Option<Arc<Mutex<Vec<String>>>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut br = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match br.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();
            if streaming {
                sink.emit(Event::Log { component: comp.clone(), stream, line: line.clone() });
                if let Ok(mut g) = log.lock() {
                    if let Some(f) = g.as_mut() {
                        let _ = writeln!(f, "[{comp}] {line}");
                    }
                }
            }
            if let Some(t) = &tail {
                if let Ok(mut v) = t.lock() {
                    v.push(line);
                    if v.len() > 40 {
                        v.remove(0);
                    }
                }
            }
        }
    })
}

/// Poll the child to completion or kill it past the deadline.
fn wait_timeout(child: &mut Child, dur: Duration) -> (Option<i32>, bool, bool) {
    let deadline = Instant::now() + dur;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return (st.code(), st.success(), false),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let st = child.wait().ok();
                    return (st.and_then(|s| s.code()), false, true);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return (None, false, false),
        }
    }
}

fn open_run_log() -> Option<File> {
    let home = std::env::var("HOME").ok()?;
    let dir = std::path::Path::new(&home).join(".local/state/envctl");
    std::fs::create_dir_all(&dir).ok()?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("envctl.log"))
        .ok()
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[s.len() - max..]
    }
}

/// Translate a Hook into a ready-to-spawn `Command` (no shell for `Command`;
/// `bash -lc` for `Script`; `bash <path>` for `ShippedScript`). needs_sudo uses
/// `sudo -n` (non-interactive): with a pre-warmed credential it runs silently;
/// without one it fails fast instead of hanging on a TTY-less password prompt.
fn build_command(hook: &Hook) -> Command {
    match hook {
        Hook::Command { command, args, env, needs_sudo } => {
            let mut c = sudo_or(command, *needs_sudo);
            c.args(args);
            for (k, v) in env {
                c.env(k, v);
            }
            c
        }
        Hook::Script { script, path, env, needs_sudo, login_shell } => {
            let shell_flag = if *login_shell { "-lc" } else { "-c" };
            // The `bash -lc` command string is the inline script, or — when a
            // `path` is given — the path itself (bash executes it).
            let body = match path {
                Some(p) => p.clone(),
                None => script.clone(),
            };
            let mut c = if *needs_sudo {
                let mut s = Command::new("sudo");
                s.arg("-n").arg("bash").arg(shell_flag);
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
        Hook::ShippedScript { path, args, needs_sudo } => {
            let mut c = if *needs_sudo {
                let mut s = Command::new("sudo");
                s.arg("-n").arg("bash").arg(path);
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

fn sudo_or(command: &str, needs_sudo: bool) -> Command {
    if needs_sudo {
        let mut s = Command::new("sudo");
        s.arg("-n").arg(command);
        s
    } else {
        Command::new(command)
    }
}

/// Never executes; reports every hook as `DryRun`. Used by tests + previews.
pub struct DryRunRunner;

impl HookRunner for DryRunRunner {
    fn run(&self, comp: &str, phase: Phase, _h: &Hook, _d: bool, _sink: &EventSink) -> OpResult {
        mk(comp, phase, OpStatus::DryRun, None, "dry-run")
    }
}
