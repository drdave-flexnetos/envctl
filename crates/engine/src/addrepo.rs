//! Phase 4: the staged build-from-source pipeline + a structurally-confined AI
//! agent orchestrator.
//!
//! Stages: acquire → [strategy transform] → detect → build (STREAMED) → locate →
//! shape installs. The executor runs install + register + verify afterward.
//!
//! Safety (enforced in code, not prose): refuses to run as root (euid 0); only
//! builds/transforms/installs with an explicit `allow_build` opt-in (a bare
//! add-repo is acquire+detect+PREVIEW); clones into a 0700 per-user dir; verifies
//! the origin URL on clone reuse; runs the build/agent in its own process group,
//! killed wholesale on timeout; the AI agent is invoked non-interactively, TTY-less,
//! confined to the clone dir, never with a skip-permissions flag, and never
//! auto-commits/pushes.
use crate::detect_build::{detect, BuildPlan};
use crate::event::{Event, EventSink, Stream};
use crate::model::{
    AddRepoSpec, AiAgent, BuildStrategy, OpResult, OpStatus, Refactor, RefactorGoal, RunSummary,
};
use crate::component::Phase;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct PipelineOutcome {
    pub clone_dir: PathBuf,
    pub resolved_sha: Option<String>,
    pub build_plan: BuildPlan,
    pub artifacts: Vec<PathBuf>,
    /// (install-name, artifact) pairs after rename/cherry-pick shaping.
    pub installs: Vec<(String, PathBuf)>,
}

const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(600);
const REFACTOR_TIMEOUT: Duration = Duration::from_secs(3600);
const BUILD_TIMEOUT: Duration = Duration::from_secs(3600);

/// Testable root check: yields a Refused OpResult when euid is root.
pub(crate) fn refuse_if_root_inner(id: &str, is_root: bool, dry_run: bool) -> Option<OpResult> {
    is_root.then(|| OpResult {
        component: id.into(),
        phase: Phase::Install,
        status: OpStatus::Refused,
        exit_code: None,
        duration_ms: 0,
        message: "refusing to clone/build/agent as root (euid 0) — run as your user".into(),
        dry_run,
    })
}

fn euid_is_root() -> bool {
    rustix::process::geteuid().is_root()
}

/// THE staged pipeline. `dry_run` previews; `spec.allow_build` is the opt-in that
/// actually runs the transform/build/install. effective preview = dry_run || !allow_build.
pub fn run_pipeline(
    spec: &AddRepoSpec,
    repos_root: &Path,
    dry_run: bool,
    sink: &EventSink,
) -> anyhow::Result<(RunSummary, Option<PipelineOutcome>)> {
    let id = spec.id.as_str();
    let mut summary = RunSummary::default();

    // SAFETY: never as root.
    if let Some(res) = refuse_if_root_inner(id, euid_is_root(), dry_run) {
        summary.refused.push(id.into());
        sink.emit(Event::StepFinished { result: res.clone() });
        summary.results.push(res);
        sink.emit(Event::RunFinished { summary: summary.clone() });
        return Ok((summary, None));
    }

    // preview = dry-run OR no --build opt-in. A bare add-repo never runs upstream code.
    let preview = dry_run || !spec.allow_build;
    if !spec.allow_build && !dry_run {
        sink.emit(plainlog(id, "preview only — pass --build to acquire + build + install (and run any refactor agent)".into()));
    }

    macro_rules! stage {
        ($name:expr, $body:expr) => {{
            let t = Instant::now();
            sink.emit(Event::StepStarted { component: id.into(), phase: Phase::Install, index: 0, total: 0 });
            let r: Result<String, String> = $body;
            let (status, msg) = match &r {
                Ok(m) => (if preview { OpStatus::DryRun } else { OpStatus::Ok }, m.clone()),
                Err(e) => (OpStatus::Failed, e.clone()),
            };
            let res = OpResult {
                component: id.into(), phase: Phase::Install, status, exit_code: None,
                duration_ms: t.elapsed().as_millis(), message: format!("{}: {}", $name, msg), dry_run: preview,
            };
            sink.emit(Event::StepFinished { result: res.clone() });
            summary.results.push(res);
            if r.is_err() {
                summary.failed.push(format!("{id}/{}", $name));
                sink.emit(Event::RunFinished { summary: summary.clone() });
                return Ok((summary, None));
            }
            r.unwrap()
        }};
    }

    let clone_dir = repos_root.join(id);

    // 1. ACQUIRE (real even in preview? no — clone is a real fetch; only when building).
    let resolved_sha: Option<String> = if preview {
        stage!("acquire", { Ok(format!("[preview] would clone {} -> {}", spec.git_url, clone_dir.display())) });
        None
    } else {
        let sha = stage!("acquire", { acquire(spec, repos_root, &clone_dir, sink).map_err(|e| e.to_string()) });
        sha.is_empty().then(|| ()).map_or(Some(sha), |_| None)
    };

    // 2. STRATEGY TRANSFORM.
    if let BuildStrategy::Refactor { refactor } = &spec.strategy {
        stage!("refactor", { apply_refactor(refactor, &clone_dir, preview, sink).map(|_| "refactor applied".to_string()).map_err(|e| e.to_string()) });
    }

    // 3. DETECT (read-only; runs even in preview if the clone exists).
    let build_plan = if preview && !clone_dir.join(".git").is_dir() {
        // nothing cloned in preview: predict from overrides only.
        stage!("detect", { Ok(format!("[preview] build_system={:?} (predicted)", spec.build_system.unwrap_or(crate::model::BuildSystem::Auto))) });
        crate::detect_build::BuildPlan {
            system: spec.build_system.unwrap_or(crate::model::BuildSystem::Auto),
            build_cmd: if spec.build_cmd.is_empty() { "<detected at build time>".into() } else { spec.build_cmd.clone() },
            artifact_globs: if spec.artifacts.is_empty() { vec!["target/release/*".into()] } else { spec.artifacts.clone() },
        }
    } else {
        match detect(&clone_dir, spec) {
            Ok(p) => {
                let sys = p.system;
                let cmd = p.build_cmd.clone();
                stage!("detect", { Ok::<_, String>(format!("build_system={sys:?} cmd=`{cmd}`")) });
                p
            }
            Err(e) => {
                stage!("detect", { Err::<String, String>(e.to_string()) });
                unreachable!("stage! returns on Err");
            }
        }
    };

    // 4. BUILD.
    stage!("build", {
        if preview {
            Ok(format!("[preview] would run `{}` in {}", build_plan.build_cmd, clone_dir.display()))
        } else {
            stream_command(shell_in(&clone_dir, &build_plan.build_cmd), id, BUILD_TIMEOUT, sink)
                .map(|_| "build ok".into())
                .map_err(|e| e.to_string())
        }
    });

    // 5. LOCATE.
    let artifacts = locate_artifacts(&clone_dir, spec, &build_plan, preview);
    stage!("locate", {
        if artifacts.is_empty() && !preview {
            Err("no build artifacts matched (pass --artifact <glob>)".into())
        } else {
            Ok(format!("{} artifact(s): {}", artifacts.len(),
                artifacts.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")))
        }
    });

    // 6. SHAPE INSTALLS (cherry-pick / rename).
    let installs = shape_installs(&artifacts, &spec.strategy);
    stage!("install-plan", {
        if installs.is_empty() && !preview {
            Err("strategy filtered out every artifact (check --bin / --rename)".into())
        } else {
            Ok(installs.iter().map(|(n, p)| format!("{n} -> {}", p.display())).collect::<Vec<_>>().join(", "))
        }
    });

    Ok((summary, Some(PipelineOutcome { clone_dir, resolved_sha, build_plan, artifacts, installs })))
}

/// INTERACTIVE handoff: clone the repo, write the goal to `.envctl-task.md`, and
/// drop the user into an interactive agent session (claude/codex/…) IN the clone,
/// attached to the real terminal. Used when the user picks cherry-pick or a full
/// Rust port and wants to drive the refactor themselves rather than headless.
/// Returns after the agent session ends; the caller may then build with `--build`.
pub fn connect_agent(spec: &AddRepoSpec) -> anyhow::Result<()> {
    if euid_is_root() {
        anyhow::bail!("refusing to clone/agent as root (euid 0) — run as your user");
    }
    let (agent, prompt) = match &spec.strategy {
        BuildStrategy::Refactor { refactor: Refactor::Ai { agent, goal, instruction } } => {
            (resolve_agent(*agent), build_ai_prompt(*goal, instruction.as_deref()))
        }
        BuildStrategy::Refactor { refactor: Refactor::Patch { .. } } => {
            (resolve_agent(None), build_ai_prompt(RefactorGoal::Custom, None))
        }
        BuildStrategy::CherryPick { bins } => (
            resolve_agent(None),
            build_ai_prompt(RefactorGoal::CherryPickToCrate, Some(&format!("Focus on: {}", bins.join(", ")))),
        ),
        _ => (resolve_agent(None), build_ai_prompt(RefactorGoal::Custom, None)),
    };
    let agent = agent.ok_or_else(|| anyhow::anyhow!("no AI coding CLI found; {}", available_agents_msg()))?;

    let repos_root = {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        PathBuf::from(home).join(".local/share/envctl/repos")
    };
    ensure_private_dir(&repos_root)?;
    let clone_dir = repos_root.join(&spec.id);

    // acquire with INHERITED stdio so the user sees the clone (and any prompts).
    if !clone_dir.join(".git").is_dir() {
        let mut c = if spec.local_path.is_some() {
            let mut c = Command::new("git");
            c.args(["-c", "core.hooksPath=/dev/null", "-c", "protocol.file.allow=always", "clone", "--local", "--no-hardlinks"]);
            c.arg(spec.local_path.as_ref().unwrap());
            c
        } else {
            let mut c = git_hardened();
            c.arg("clone");
            if let Some(r) = &spec.git_ref {
                c.args(["--branch", r]);
            }
            c.arg("--").arg(&spec.git_url); // `--` => no git-arg injection
            c
        };
        c.arg(&clone_dir);
        let st = c.status()?;
        if !st.success() {
            anyhow::bail!("clone failed");
        }
    }
    ensure_private_dir(&clone_dir)?;

    std::fs::write(
        clone_dir.join(".envctl-task.md"),
        format!("# envctl refactor task\n\n{prompt}\n\nWhen finished, exit the agent. envctl will not commit or push.\n"),
    )?;

    println!("\nenvctl: connecting you to `{}` in {}", agent.bin(), clone_dir.display());
    println!("        the task is written to .envctl-task.md; collaborate, then exit the agent.");
    println!("        afterward, build it with:  envctl add-repo {} --id {} --build\n", spec.git_url, spec.id);

    // Interactive: inherit stdio so the agent attaches to the real terminal.
    let status = Command::new(agent.bin()).current_dir(&clone_dir).status()?;
    println!("\nenvctl: agent session ended (exit {:?}). Clone is at {}", status.code(), clone_dir.display());
    Ok(())
}

// ── acquire ─────────────────────────────────────────────────────────────────
fn acquire(spec: &AddRepoSpec, repos_root: &Path, clone_dir: &Path, sink: &EventSink) -> anyhow::Result<String> {
    ensure_private_dir(repos_root)?;

    if let Some(local) = &spec.local_path {
        if !clone_dir.join(".git").is_dir() {
            // Explicit local clone: the file protocol is intended here (the
            // file.allow=never hardening is only to block file:// SUBMODULES on a
            // remote clone). Hooks stay disabled.
            let mut c = Command::new("git");
            c.args(["-c", "core.hooksPath=/dev/null", "-c", "protocol.file.allow=always", "clone", "--local", "--no-hardlinks"])
                .arg(local)
                .arg(clone_dir);
            stream_command(c, &spec.id, ACQUIRE_TIMEOUT, sink)?;
        }
    } else if clone_dir.join(".git").is_dir() {
        // reuse: verify origin matches the requested URL before fetching (SAFETY).
        let origin = Command::new("git").arg("-C").arg(clone_dir).args(["remote", "get-url", "origin"]).output().ok()
            .filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        if origin.as_deref() != Some(spec.git_url.as_str()) {
            anyhow::bail!("refusing to reuse clone {}: origin is {:?}, not {}", clone_dir.display(), origin, spec.git_url);
        }
        let mut c = git_hardened();
        c.arg("-C").arg(clone_dir).args(["fetch", "--all", "--tags", "--prune"]);
        stream_command(c, &spec.id, ACQUIRE_TIMEOUT, sink)?;
    } else {
        let mut c = git_hardened();
        c.arg("clone");
        if spec.recurse_submodules {
            c.arg("--recurse-submodules");
        }
        if let Some(r) = &spec.git_ref {
            c.args(["--branch", r]);
        }
        c.arg("--").arg(&spec.git_url).arg(clone_dir); // `--` => no git-arg injection
        stream_command(c, &spec.id, ACQUIRE_TIMEOUT, sink)?;
    }
    ensure_private_dir(clone_dir)?; // 0700 the per-slug clone too

    if let Some(r) = &spec.git_ref {
        let mut c = git_hardened();
        c.arg("-C").arg(clone_dir).args(["checkout", r]);
        let _ = stream_command(c, &spec.id, ACQUIRE_TIMEOUT, sink); // best-effort
    }
    Ok(resolve_sha(clone_dir).unwrap_or_default())
}

/// git with file-protocol + hooks disabled (no surprise hook execution on clone).
fn git_hardened() -> Command {
    let mut c = Command::new("git");
    c.args(["-c", "protocol.file.allow=never", "-c", "core.hooksPath=/dev/null"]);
    c
}

fn resolve_sha(clone_dir: &Path) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(clone_dir).args(["rev-parse", "HEAD"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

// ── refactor (patch | AI) ────────────────────────────────────────────────────
fn apply_refactor(refactor: &Refactor, clone_dir: &Path, preview: bool, sink: &EventSink) -> anyhow::Result<()> {
    match refactor {
        Refactor::Patch { command } => {
            if preview {
                sink.emit(plainlog("refactor", format!("[preview] patch: bash -lc `{command}` in {}", clone_dir.display())));
                return Ok(());
            }
            stream_command(shell_in(clone_dir, command), "refactor", REFACTOR_TIMEOUT, sink).map(|_| ())
        }
        Refactor::Ai { agent, goal, instruction } => {
            let agent = resolve_agent(*agent)
                .ok_or_else(|| anyhow::anyhow!("no AI coding CLI found; tried {}", available_agents_msg()))?;
            // AUDIT-FIX: only Claude/Codex are structurally confined to the clone
            // (--add-dir / --cd). Refuse Gemini/Kimi headless — they have no
            // enforceable FS sandbox flag; use --connect for a supervised session.
            if matches!(agent, AiAgent::Gemini | AiAgent::Kimi) {
                anyhow::bail!(
                    "{} has no enforceable sandbox flag for a headless refactor — use claude/codex, or `--connect` for an interactive session",
                    agent.bin()
                );
            }
            let prompt = build_ai_prompt(*goal, instruction.as_deref());
            let cd = clone_dir.to_string_lossy().into_owned();
            let argv = agent.argv(&prompt, &cd);
            if preview {
                sink.emit(plainlog("refactor", format!(
                    "[preview] AI refactor:\n  agent: {}\n  argv:  {} {}\n  cwd:   {}\n  timeout: {}s\n  prompt:\n{}",
                    agent.bin(), agent.bin(), shellish(&argv), clone_dir.display(), REFACTOR_TIMEOUT.as_secs(), prompt
                )));
                return Ok(());
            }
            // Non-interactive, TTY-less, confined to the clone, never auto-commit.
            let mut c = Command::new(agent.bin());
            c.current_dir(clone_dir).args(&argv);
            stream_command(c, "refactor", REFACTOR_TIMEOUT, sink)?;
            // audit the change, then the pipeline re-detects + builds the result.
            if let Ok(out) = Command::new("git").arg("-C").arg(clone_dir).args(["diff", "--stat"]).output() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    sink.emit(plainlog("refactor", format!("  {line}")));
                }
            }
            Ok(())
        }
    }
}

fn resolve_agent(explicit: Option<AiAgent>) -> Option<AiAgent> {
    let probe = |a: AiAgent| agent_present(a).then_some(a);
    if let Some(a) = explicit {
        return probe(a);
    }
    AiAgent::preference().into_iter().find_map(probe)
}
fn agent_present(a: AiAgent) -> bool {
    which::which(a.bin()).is_ok()
        || Command::new("bash").args(["-lc", &format!("command -v {}", a.bin())])
            .stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}
fn available_agents_msg() -> String {
    let avail: Vec<&str> = AiAgent::preference().into_iter().filter(|a| agent_present(*a)).map(|a| a.bin()).collect();
    if avail.is_empty() { "claude/codex/gemini/kimi (none installed)".into() } else { format!("available: {}", avail.join(", ")) }
}

fn build_ai_prompt(goal: RefactorGoal, extra: Option<&str>) -> String {
    let base = match goal {
        RefactorGoal::PortToRust =>
            "Port this repository to an idiomatic, buildable Cargo project. Produce a Cargo.toml at the repo root and a src/ tree so that `cargo build --release` succeeds and the original CLI/binaries are preserved. Work only within this directory. Do not commit, push, run sudo, or edit files outside this directory.",
        RefactorGoal::CherryPickToCrate =>
            "Extract the core functionality into a single new Rust crate at the repo root (Cargo.toml + src/), buildable with `cargo build --release`. Work only within this directory. Do not commit, push, run sudo, or edit files outside this directory.",
        RefactorGoal::RenameForSynergy =>
            "Rename the primary binaries/symbols for naming consistency, keeping the build working; do not change the build system. Work only within this directory. Do not commit, push, run sudo, or edit files outside this directory.",
        RefactorGoal::Custom =>
            "Perform the requested refactor. Work only within this directory. Do not commit, push, run sudo, or edit files outside this directory.",
    };
    match extra {
        Some(e) if !e.trim().is_empty() => format!("{base}\n\nAdditional instructions:\n{e}"),
        _ => base.to_string(),
    }
}

// ── locate + strategy shaping ────────────────────────────────────────────────
fn locate_artifacts(clone_dir: &Path, spec: &AddRepoSpec, plan: &BuildPlan, preview: bool) -> Vec<PathBuf> {
    let globs: Vec<String> = if !spec.artifacts.is_empty() { spec.artifacts.clone() } else { plan.artifact_globs.clone() };
    let mut found = Vec::new();
    for g in &globs {
        let pat = clone_dir.join(g);
        if let Ok(paths) = glob::glob(&pat.to_string_lossy()) {
            for p in paths.flatten() {
                // executable files only, and NOT build-system source/helper scripts
                // that a catch-all `*`/`src/*` glob would otherwise sweep up (audit fix).
                if p.is_file() && is_executable(&p) && !is_source_helper(&p) {
                    found.push(p);
                }
            }
        }
    }
    if found.is_empty() && preview {
        if let Some(g) = globs.first() {
            found.push(clone_dir.join(g));
        }
    }
    found.sort();
    found.dedup();
    found
}

fn shape_installs(artifacts: &[PathBuf], strategy: &BuildStrategy) -> Vec<(String, PathBuf)> {
    let stem = |p: &Path| p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    match strategy {
        BuildStrategy::AsIs | BuildStrategy::Refactor { .. } => artifacts.iter().map(|p| (stem(p), p.clone())).collect(),
        BuildStrategy::CherryPick { bins } => artifacts.iter()
            .filter(|p| bins.iter().any(|b| b == &stem(p)))
            .map(|p| (stem(p), p.clone())).collect(),
        BuildStrategy::Rename { renames } => artifacts.iter().map(|p| {
            let s = stem(p);
            let name = renames.iter().find(|r| r.from == s).map(|r| r.to.clone()).unwrap_or(s);
            (name, p.clone())
        }).collect(),
    }
}

// ── streamed command (process-group-isolated, timeout-killed) ────────────────
fn stream_command(mut cmd: Command, comp: &str, timeout: Duration, sink: &EventSink) -> anyhow::Result<i32> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // Own process group so a wedged child tree (bash -> cargo/agent) is reaped wholesale.
    unsafe {
        cmd.pre_exec(|| {
            let _ = rustix::process::setsid();
            Ok(())
        });
    }
    let mut child: Child = cmd.spawn().map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    let pid = child.id();
    let tail = Arc::new(Mutex::new(Vec::<String>::new()));
    let h_out = child.stdout.take().map(|r| pump(r, comp.to_string(), Stream::Stdout, sink.clone(), None));
    let h_err = child.stderr.take().map(|r| pump(r, comp.to_string(), Stream::Stderr, sink.clone(), Some(tail.clone())));

    let (code, success, timed_out) = wait_timeout(&mut child, timeout, pid);
    if let Some(h) = h_out { let _ = h.join(); }
    if let Some(h) = h_err { let _ = h.join(); }

    if timed_out {
        anyhow::bail!("timed out after {}s", timeout.as_secs());
    }
    if success {
        Ok(code.unwrap_or(0))
    } else {
        let msg = tail.lock().map(|v| v.join("\n")).unwrap_or_default();
        anyhow::bail!("exit {:?}: {msg}", code)
    }
}

fn pump<R: std::io::Read + Send + 'static>(
    reader: R, comp: String, stream: Stream, sink: EventSink, tail: Option<Arc<Mutex<Vec<String>>>>,
) -> std::thread::JoinHandle<()> {
    use std::io::{BufRead, BufReader};
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
            sink.emit(Event::Log { component: comp.clone(), stream, line: line.clone() });
            if let Some(t) = &tail {
                if let Ok(mut v) = t.lock() {
                    v.push(line);
                    if v.len() > 40 { v.remove(0); }
                }
            }
        }
    })
}

fn wait_timeout(child: &mut Child, dur: Duration, pid: u32) -> (Option<i32>, bool, bool) {
    let deadline = Instant::now() + dur;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return (st.code(), st.success(), false),
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_group(pid);
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

/// Kill the child's whole process group (it is the group leader via setsid).
fn kill_group(pid: u32) {
    if let Some(p) = rustix::process::Pid::from_raw(pid as i32) {
        let _ = rustix::process::kill_process_group(p, rustix::process::Signal::Kill);
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────
fn shell_in(dir: &Path, script: &str) -> Command {
    let mut c = Command::new("bash");
    c.arg("-lc").arg(script).current_dir(dir);
    c
}
fn plainlog(comp: &str, line: String) -> Event {
    Event::Log { component: comp.into(), stream: Stream::Stdout, line }
}
fn shellish(argv: &[String]) -> String {
    argv.iter().map(|a| if a.contains(' ') || a.contains('\n') { format!("'{}'", a.replace('\'', "'\\''")) } else { a.clone() }).collect::<Vec<_>>().join(" ")
}
fn ensure_private_dir(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if !p.exists() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p)?;
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}
#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// A build-system source/helper file that a catch-all glob would wrongly install.
fn is_source_helper(p: &Path) -> bool {
    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
    const DENY: &[&str] = &[
        "configure", "config.guess", "config.sub", "config.status", "install-sh",
        "compile", "missing", "depcomp", "libtool", "bootstrap", "autogen.sh", "ltmain.sh",
    ];
    if DENY.contains(&name) {
        return true;
    }
    matches!(
        p.extension().and_then(|s| s.to_str()),
        Some("sh" | "py" | "pl" | "rb" | "bash" | "in" | "am" | "ac" | "m4" | "guess" | "sub")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_refusal_is_testable() {
        assert!(refuse_if_root_inner("x", false, false).is_none());
        let r = refuse_if_root_inner("x", true, false).unwrap();
        assert_eq!(r.status, OpStatus::Refused);
    }
    #[test]
    fn ai_argv_is_confined_never_yolo() {
        let v = AiAgent::Claude.argv("do the thing", "/tmp/clone");
        assert!(v.iter().any(|s| s == "--add-dir") && v.iter().any(|s| s == "/tmp/clone"));
        assert!(v.iter().any(|s| s == "--permission-mode"));
        assert!(!v.iter().any(|s| s.contains("dangerously") || s == "--yolo" || s == "-y"));
        let c = AiAgent::Codex.argv("x", "/tmp/clone");
        assert!(c.iter().any(|s| s == "--cd"));
    }
}
