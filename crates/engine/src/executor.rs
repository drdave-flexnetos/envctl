//! The best-effort run loop. Maps a `RunPlan` to an ordered target list (topo
//! forward for install/fix, reverse for remove), runs each phase, accumulates a
//! `RunSummary`, and emits `RunStarted`/`StepStarted`/`StepFinished`/`RunFinished`
//! so the CLI and GUI render identically. Also `add_repo()` — the hardened
//! drop-in writer.
//!
//! Best-effort = one component failing never aborts the run (mirrors the wizard's
//! `run()`), but a failed component's dependents are `SkippedBlocked`, and an
//! `install` whose `detect` already passes is `Skipped` (idempotent).
use crate::component::{Component, HookRunner, Phase};
use crate::error::{run_phase, RunContext};
use crate::event::{Event, EventSink, Stream};
use crate::model::{AddRepoSpec, OpResult, OpStatus, Registry, RunPlan, RunSummary};
use std::collections::HashSet;
use std::path::Path;

/// Resolve run-wide identities ONCE (no TOCTOU). GPU presence via the PCI floor
/// (driver-independent); live-root UUID via findmnt+blkid for the guard engine.
fn resolve_context() -> RunContext {
    RunContext {
        gpu_present: crate::detect::pci_nvidia_count() > 0,
        live_root_uuid: crate::guard::resolve_live_root_uuid(),
    }
}

pub fn run(
    reg: &Registry,
    runner: &dyn HookRunner,
    plan: RunPlan,
    sink: &EventSink,
) -> anyhow::Result<RunSummary> {
    let ctx = resolve_context();

    // Forward order, or reverse for Remove (tear down dependents first).
    let mut order: Vec<&Component> = if plan.targets.is_empty() {
        reg.ordered().collect()
    } else {
        let mut v: Vec<&Component> = Vec::new();
        for id in &plan.targets {
            for c in reg.closure(id)? {
                if !v.iter().any(|x| x.id == c.id) {
                    v.push(c);
                }
            }
        }
        v
    };
    if plan.phase == Phase::Remove {
        order.reverse();
    }

    let total = order.len();
    sink.emit(Event::RunStarted {
        phase: plan.phase,
        total,
        dry_run: plan.dry_run,
    });

    let mut summary = RunSummary::default();
    let mut failed_ids: HashSet<String> = HashSet::new();

    for (i, comp) in order.iter().enumerate() {
        sink.emit(Event::StepStarted {
            component: comp.id.clone(),
            phase: plan.phase,
            index: i,
            total,
        });

        // Dependency gate (forward phases): a dependency that already failed
        // this run blocks its dependents instead of running them on rubble.
        if matches!(plan.phase, Phase::Install | Phase::Fix)
            && comp.requires.iter().any(|d| failed_ids.contains(d))
        {
            let res = mkres(comp, plan.phase, OpStatus::SkippedBlocked, "dependency failed", plan.dry_run);
            summary.skipped_blocked.push(comp.id.clone());
            finish(sink, &mut summary, res);
            continue;
        }

        // Idempotent install: skip-if-already-detected (never re-run curl|bash).
        if plan.phase == Phase::Install && !plan.dry_run {
            if let Some(h) = comp.detect.as_ref() {
                if runner.run(&comp.id, Phase::Detect, h, false).status == OpStatus::Ok {
                    let res = mkres(comp, plan.phase, OpStatus::Skipped, "already present", false);
                    finish(sink, &mut summary, res);
                    continue;
                }
            }
        }

        let res = run_phase(sink, comp, plan.phase, runner, plan.dry_run, &ctx);
        match res.status {
            OpStatus::Failed => {
                summary.failed.push(comp.id.clone());
                failed_ids.insert(comp.id.clone());
            }
            OpStatus::Refused => summary.refused.push(comp.id.clone()),
            OpStatus::SkippedBlocked => summary.skipped_blocked.push(comp.id.clone()),
            _ => {}
        }
        finish(sink, &mut summary, res);
    }

    sink.emit(Event::RunFinished {
        summary: summary.clone(),
    });
    Ok(summary)
}

fn mkres(comp: &Component, phase: Phase, status: OpStatus, msg: &str, dry_run: bool) -> OpResult {
    OpResult {
        component: comp.id.clone(),
        phase,
        status,
        exit_code: None,
        duration_ms: 0,
        message: msg.into(),
        dry_run,
    }
}

fn finish(sink: &EventSink, summary: &mut RunSummary, res: OpResult) {
    sink.emit(Event::StepFinished { result: res.clone() });
    summary.results.push(res);
}

// ---------------------------------------------------------------------------
// add-repo (hardened drop-in writer). Phase-0 scope: validate + register a
// component drop-in atomically (with backup); the build itself runs on the next
// explicit `envctl install <id>`. The full 9-stage build pipeline is Phase 4.
// ---------------------------------------------------------------------------
pub fn add_repo(
    manifest_dir: &Path,
    reg: &Registry,
    _runner: &dyn HookRunner,
    spec: AddRepoSpec,
    dry_run: bool,
    sink: &EventSink,
) -> anyhow::Result<RunSummary> {
    let id = spec.id.trim();
    // 1. strict slug validation (safe as a bare TOML key AND a filename component)
    if !is_valid_slug(id) {
        anyhow::bail!(
            "invalid component id '{id}': use [a-z0-9] start, then [a-z0-9._-] (no spaces/slashes)"
        );
    }
    // 2. refuse collision with an existing/built-in component
    if reg.get(id).is_some() {
        anyhow::bail!("component id '{id}' already exists — refusing to shadow it (pick another --id)");
    }
    // 3. reject inputs that could break out of the TOML literal we emit
    for (label, val) in [("git_url", &spec.git_url), ("build_cmd", &spec.build_cmd)] {
        if val.contains("'''") {
            anyhow::bail!("{label} may not contain ''' (would break the generated manifest)");
        }
    }

    let toml_text = synth_component_toml(id, &spec);

    if dry_run {
        sink.emit(Event::Log {
            component: id.into(),
            stream: Stream::Stdout,
            line: format!("[dry-run] would write components.d/{id}.toml:\n{toml_text}"),
        });
        return Ok(RunSummary::default());
    }

    // 4. atomic temp+rename, with a timestamped backup of any existing drop-in
    let dir = manifest_dir.join("components.d");
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{id}.toml"));
    if target.exists() {
        let bak = dir.join(format!("{id}.toml.bak.{}", now_epoch()));
        std::fs::copy(&target, &bak)?;
        sink.emit(Event::Log {
            component: id.into(),
            stream: Stream::Stdout,
            line: format!("backed up existing drop-in -> {}", bak.display()),
        });
    }
    let tmp = dir.join(format!(".{id}.toml.tmp"));
    std::fs::write(&tmp, &toml_text)?;
    std::fs::rename(&tmp, &target)?;

    sink.emit(Event::Log {
        component: id.into(),
        stream: Stream::Stdout,
        line: format!(
            "registered component '{id}' -> {}\n  build it with:  envctl install {id}",
            target.display()
        ),
    });
    Ok(RunSummary::default())
}

fn is_valid_slug(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Build the drop-in. `id` is a validated slug (safe bare key). `git_url` is
/// single-quoted for bash; `build_cmd` is the user's own build command, embedded
/// in a TOML literal string (both pre-checked for `'''`).
fn synth_component_toml(id: &str, spec: &AddRepoSpec) -> String {
    let url_sq = sh_single_quote(&spec.git_url);
    format!(
        "# generated by `envctl add-repo` — edit freely or re-run add-repo --force\n\
         [[component]]\n\
         id = \"{id}\"\n\
         name = \"{id} (add-repo)\"\n\
         description = \"Built from source via envctl add-repo.\"\n\
         \n\
         [component.detect]\n\
         kind = \"command\"\n\
         command = \"bash\"\n\
         args = [\"-lc\", \"command -v {id}\"]\n\
         \n\
         [component.install]\n\
         kind = \"script\"\n\
         login_shell = true\n\
         script = '''\n\
         set -e\n\
         install -d -m 700 \"$HOME/.local/share/envctl/repos\"\n\
         SRC=\"$HOME/.local/share/envctl/repos/{id}\"\n\
         if [ -d \"$SRC/.git\" ]; then git -C \"$SRC\" pull --ff-only; else git clone {url} \"$SRC\"; fi\n\
         cd \"$SRC\"\n\
         {build}\n\
         '''\n",
        id = id,
        url = url_sq,
        build = spec.build_cmd,
    )
}

/// POSIX single-quote: wrap in '...' and replace each ' with '\'' .
fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
