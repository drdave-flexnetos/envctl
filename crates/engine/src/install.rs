//! Phase 4 INSTALL + WIRE-IN. Lands built artifacts into `~/.local/bin` under the
//! engine's gold-standard discipline:
//!   * symlink-default into the 0700 repo store;
//!   * REFUSE to shadow a system binary (a name that already resolves on PATH
//!     outside `~/.local/bin`) and HARD-refuse well-known names (sudo/git/bash…);
//!   * refuse-overwrite-unmanaged: a target is "ours" only if it's a symlink whose
//!     CANONICAL target is inside `~/.local/share/envctl/repos/<slug>/` — a foreign
//!     file/symlink is reported + skipped unless `force`, and force backs it up;
//!   * PATH ownership goes through the existing `wiring::apply` (owned block);
//!   * best-effort (one failure never aborts) + dry-run-previewable.
#![cfg(unix)]
use crate::event::{Event, EventSink, Stream};
use crate::model::Wiring;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ArtifactPlan {
    pub source: PathBuf,
    /// Name in ~/.local/bin (post-rename). `renamed` = the user explicitly chose it.
    pub install_name: String,
    pub renamed: bool,
}

#[derive(Clone, Debug)]
pub struct InstallPlan {
    pub id: String,
    pub slug: String,
    pub artifacts: Vec<ArtifactPlan>,
    /// Extra PATH dirs beyond ~/.local/bin.
    pub extra_path_entries: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InstallReport {
    pub notes: Vec<String>,
    pub failures: Vec<(String, String)>,
    pub installed_paths: Vec<String>,
    pub refused_unmanaged: Vec<String>,
}
impl InstallReport {
    fn note(&mut self, s: impl Into<String>) {
        self.notes.push(s.into());
    }
    fn fail(&mut self, kind: &str, e: impl std::fmt::Display) {
        self.failures.push((kind.into(), e.to_string()));
    }
}

const WELL_KNOWN: &[&str] = &[
    "sudo", "su", "git", "bash", "sh", "env", "python", "python3", "ls", "cp", "mv", "rm", "cargo",
    "rustc", "nix", "ssh", "gpg", "apt", "apt-get", "dpkg",
];

pub fn install_and_wire(plan: &InstallPlan, force: bool, dry_run: bool, sink: &EventSink) -> InstallReport {
    let mut rep = InstallReport::default();

    for a in &plan.artifacts {
        match install_artifact(plan, a, force, dry_run) {
            Ok(Some(path)) => {
                rep.note(format!(
                    "{} {} -> {}",
                    if dry_run { "[preview] would install" } else { "installed" },
                    a.source.display(),
                    path
                ));
                rep.installed_paths.push(path);
            }
            Ok(None) => {}
            Err(InstallErr::Shadow(name)) => {
                rep.fail("artifact", format!("refusing to install '{name}': it would shadow a system command (use --rename)"));
            }
            Err(InstallErr::Foreign(t)) => {
                rep.refused_unmanaged.push(t.clone());
                rep.note(format!("left foreign file at {t} intact (not envctl-managed) — pass --force to back up + replace"));
            }
            Err(InstallErr::Missing(s)) => rep.fail("artifact", format!("source not found: {s}")),
            Err(InstallErr::Unsafe(n)) => rep.fail("artifact", format!("refusing unsafe install name '{n}': must be a single path component (no '/', '..')")),
            Err(InstallErr::Io(e)) => rep.fail("artifact", e),
        }
    }

    // PATH ownership via the existing wiring engine (owned block; reset reverts it).
    let wiring = synth_wiring(plan);
    if dry_run {
        if !wiring.path_entries.is_empty() {
            rep.note(format!("[preview] would own PATH entries: {}", wiring.path_entries.join(", ")));
        }
    } else if !wiring.path_entries.is_empty() {
        let wrep = crate::wiring::apply(&wiring);
        for n in wrep.notes {
            rep.note(n);
        }
        for (k, e) in wrep.failures {
            rep.fail(&format!("wiring:{k}"), e);
        }
    }

    for n in &rep.notes {
        sink.emit(Event::Log { component: plan.id.clone(), stream: Stream::Stdout, line: n.clone() });
    }
    for (k, e) in &rep.failures {
        sink.emit(Event::Log { component: plan.id.clone(), stream: Stream::Stderr, line: format!("install ({k}) failed: {e}") });
    }
    rep
}

enum InstallErr {
    Shadow(String),
    Foreign(String),
    Missing(String),
    Unsafe(String),
    Io(std::io::Error),
}
impl From<std::io::Error> for InstallErr {
    fn from(e: std::io::Error) -> Self {
        InstallErr::Io(e)
    }
}

/// An install name must be exactly ONE path component — no separators, no `.`/`..`,
/// non-empty — so `local_bin().join(name)` can never escape ~/.local/bin.
fn is_safe_install_name(name: &str) -> bool {
    use std::path::Component;
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return false;
    }
    let mut comps = std::path::Path::new(name).components();
    matches!((comps.next(), comps.next()), (Some(Component::Normal(_)), None))
}

fn install_artifact(plan: &InstallPlan, a: &ArtifactPlan, force: bool, dry_run: bool) -> Result<Option<String>, InstallErr> {
    // AUDIT-FIX (blocker): the install name lands in `local_bin().join(name)`. A
    // rename/cherry-pick name like `../../.config/evil` would plant (or, with
    // --force, overwrite) a managed symlink OUTSIDE ~/.local/bin. Refuse anything
    // that is not exactly one safe path component, at the sink — independent of
    // upstream slug validation.
    if !is_safe_install_name(&a.install_name) {
        return Err(InstallErr::Unsafe(a.install_name.clone()));
    }
    // AUDIT-FIX (blocker): the WELL_KNOWN hard-refusal is UNCONDITIONAL — even a
    // user `--rename foo=git` may NOT take a critical command name (renaming TO
    // `sudo`/`git`/`bash` is exactly the shadow attack). Validate the name BEFORE
    // touching the filesystem.
    if WELL_KNOWN.contains(&a.install_name.as_str()) {
        return Err(InstallErr::Shadow(a.install_name.clone()));
    }
    if !a.source.exists() {
        return Err(InstallErr::Missing(a.source.display().to_string()));
    }
    // The SOFT PATH-shadow check (a name that merely collides with some other
    // $PATH binary) may be bypassed by an explicit rename.
    if !a.renamed && shadows_system(&a.install_name) {
        return Err(InstallErr::Shadow(a.install_name.clone()));
    }
    let dir = local_bin();
    let target = dir.join(&a.install_name);
    let target_s = target.display().to_string();

    if target.symlink_metadata().is_ok() {
        if is_managed_symlink(&target, &plan.slug) {
            if !dry_run {
                let _ = std::fs::remove_file(&target); // ours — replace
            }
        } else {
            // foreign file/symlink: refuse unless --force, then back up.
            if !force {
                return Err(InstallErr::Foreign(target_s));
            }
            if dry_run {
                return Ok(Some(format!("{target_s} (would back up the existing file first)")));
            }
            let bak = format!("{target_s}.bak.{}", now_epoch());
            std::fs::rename(&target, &bak)?;
        }
    }

    if dry_run {
        return Ok(Some(target_s));
    }
    ensure_private_bin(&dir)?;
    let src = std::fs::canonicalize(&a.source).unwrap_or_else(|_| a.source.clone());
    if target.symlink_metadata().is_ok() {
        let _ = std::fs::remove_file(&target);
    }
    symlink(&src, &target)?;
    Ok(Some(target_s))
}

/// True if `name` resolves on the system PATH OUTSIDE ~/.local/bin, or is a
/// hard-refused well-known command.
fn shadows_system(name: &str) -> bool {
    if WELL_KNOWN.contains(&name) {
        return true;
    }
    let lb = local_bin();
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            if dir.is_empty() {
                continue;
            }
            let p = Path::new(dir);
            if p == lb {
                continue;
            }
            if p.join(name).exists() {
                return true;
            }
        }
    }
    false
}

/// A target is OURS iff it's a symlink whose CANONICAL target is inside the repo
/// store for this slug. Unreadable/dangling => foreign (never substring-match).
fn is_managed_symlink(target: &Path, slug: &str) -> bool {
    if !target.is_symlink() {
        return false;
    }
    let store = match std::fs::canonicalize(repo_store().join(slug)) {
        Ok(s) => s,
        Err(_) => return false, // store gone => can't prove ownership => foreign
    };
    match std::fs::canonicalize(target) {
        Ok(real) => real.starts_with(&store),
        Err(_) => false,
    }
}

fn synth_wiring(plan: &InstallPlan) -> Wiring {
    let mut path_entries: Vec<String> = vec!["~/.local/bin".into()];
    path_entries.extend(plan.extra_path_entries.iter().cloned());
    path_entries.dedup();
    Wiring { path_entries, ..Default::default() }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()))
}
fn local_bin() -> PathBuf {
    home().join(".local/bin")
}
fn repo_store() -> PathBuf {
    home().join(".local/share/envctl/repos")
}
fn ensure_private_bin(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}
fn now_epoch() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn well_known_names_are_shadowed() {
        assert!(shadows_system("sudo"));
        assert!(shadows_system("git"));
        // a random unlikely name does not shadow
        assert!(!shadows_system("zzqx-envctl-unlikely-9000"));
    }
    #[test]
    fn managed_symlink_needs_canonical_containment() {
        // a path that doesn't exist is never ours
        assert!(!is_managed_symlink(Path::new("/no/such/envctl/link"), "slug"));
    }
    #[test]
    fn rename_cannot_shadow_a_well_known_command() {
        // even with renamed=true (an explicit --rename), installing AS `git` is a
        // hard refusal — the source need not even exist, the name is rejected first.
        let plan = InstallPlan { id: "x".into(), slug: "x".into(), artifacts: vec![], extra_path_entries: vec![] };
        let a = ArtifactPlan { source: Path::new("/no/such/built/foo-cli").to_path_buf(), install_name: "git".into(), renamed: true };
        match install_artifact(&plan, &a, false, true) {
            Err(InstallErr::Shadow(n)) => assert_eq!(n, "git"),
            other => panic!("expected Shadow(git), got {:?}", match other { Ok(_) => "Ok".to_string(), Err(InstallErr::Unsafe(s)) => format!("Unsafe({s})"), Err(InstallErr::Missing(s)) => format!("Missing({s})"), Err(InstallErr::Foreign(s)) => format!("Foreign({s})"), Err(InstallErr::Io(e)) => format!("Io({e})"), Err(InstallErr::Shadow(s)) => format!("Shadow({s})") }),
        }
    }
    #[test]
    fn install_name_must_be_a_single_component() {
        // safe: ordinary bin names
        assert!(is_safe_install_name("ripgrep"));
        assert!(is_safe_install_name("my-tool_v2.bin"));
        // unsafe: traversal / separators / dot-dirs / empty — would escape ~/.local/bin
        assert!(!is_safe_install_name("../evil"));
        assert!(!is_safe_install_name("../../.config/evil"));
        assert!(!is_safe_install_name("sub/dir"));
        assert!(!is_safe_install_name(".."));
        assert!(!is_safe_install_name("."));
        assert!(!is_safe_install_name(""));
        assert!(!is_safe_install_name("/abs"));
    }
}
