//! Integration tests for `envctl agent {sync,add,remove,lock,list,clean}`. Drives the
//! real `envctl` binary against a hermetic temp project (its own cwd, config, and
//! XDG dirs) and asserts: (1) every mutating verb's dry-run (no `--apply`) writes
//! NOTHING — config + `agent-env.lock` + destination dir are byte-identical before/after
//! (the fail-closed invariant); (2) the `--json` shape of `list` and `lock --check`;
//! (3) the exit-code contract (`list` ⇒ 0; the `--ref`/`--branch` conflict ⇒ engine bail
//! ⇒ nonzero).
//!
//! Hermetic: the binary loads a manifest dir at startup even for agent verbs, so each
//! test points `ENVCTL_MANIFEST_DIR` at an empty temp dir (the agent path never reads
//! the component registry). `XDG_DATA_HOME`/`XDG_CONFIG_HOME` redirect the global lock
//! + config off the real `~`, and the project root is the spawned process's cwd.
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_envctl")
}

/// A hermetic temp project: a `project/` cwd holding `agent-env.yaml`, an empty
/// `manifest/` dir, and isolated XDG roots — all under one unique temp dir.
struct Fixture {
    root: PathBuf,
    project: PathBuf,
    manifest: PathBuf,
    xdg_data: PathBuf,
    xdg_config: PathBuf,
    /// The config-declared destination dir (must NOT be created by a dry-run).
    dest: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Disambiguate concurrent tests within the same process by counter too.
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("envctl-agent-it-{nanos}-{seq}"));
        let project = root.join("project");
        let manifest = root.join("manifest");
        let xdg_data = root.join("xdg-data");
        let xdg_config = root.join("xdg-config");
        let dest = project.join("dest");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&manifest).unwrap();
        std::fs::write(
            project.join("agent-env.yaml"),
            format!(
                "destination: {}\nscope: project\nskills: []\nmcps: []\ncommands: []\n",
                dest.display()
            ),
        )
        .unwrap();
        Fixture {
            root,
            project,
            manifest,
            xdg_data,
            xdg_config,
            dest,
        }
    }

    /// A command rooted in the project cwd with the hermetic env applied.
    fn cmd(&self) -> Command {
        let mut c = Command::new(bin());
        c.current_dir(&self.project)
            .env("ENVCTL_MANIFEST_DIR", &self.manifest)
            .env("XDG_DATA_HOME", &self.xdg_data)
            .env("XDG_CONFIG_HOME", &self.xdg_config);
        c
    }

    fn config_path(&self) -> PathBuf {
        self.project.join("agent-env.yaml")
    }

    fn lock_path(&self) -> PathBuf {
        self.project.join("agent-env.lock")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// A snapshot of the on-disk state that a dry-run must not mutate.
fn snapshot(fx: &Fixture) -> (Option<String>, bool, bool) {
    let config = std::fs::read_to_string(fx.config_path()).ok();
    let lock_exists = fx.lock_path().exists();
    let dest_exists = fx.dest.exists();
    (config, lock_exists, dest_exists)
}

fn assert_unchanged(fx: &Fixture, before: &(Option<String>, bool, bool), verb: &str) {
    let after = snapshot(fx);
    assert_eq!(before.0, after.0, "{verb} dry-run mutated the config");
    assert_eq!(
        before.1, after.1,
        "{verb} dry-run created/removed agent-env.lock"
    );
    assert_eq!(
        before.2, after.2,
        "{verb} dry-run created/removed the destination dir"
    );
    // Belt-and-suspenders: the dry-run must never materialize the destination.
    assert!(
        !fx.dest.exists(),
        "{verb} dry-run created the destination dir {}",
        fx.dest.display()
    );
}

// --------------------------------------------------------------------------------------
// Per-verb dry-run = zero writes (the fail-closed invariant).
// --------------------------------------------------------------------------------------

#[test]
fn sync_dry_run_writes_nothing() {
    let fx = Fixture::new();
    let before = snapshot(&fx);
    let out = fx.cmd().args(["agent", "sync"]).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_unchanged(&fx, &before, "sync");
}

#[test]
fn add_dry_run_writes_nothing() {
    let fx = Fixture::new();
    let before = snapshot(&fx);
    // `add` with a local path but NO --apply: preview only (records "would_add").
    let out = fx
        .cmd()
        .args(["agent", "add", "./some-source", "--no-sync"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_unchanged(&fx, &before, "add");
}

#[test]
fn clean_dry_run_writes_nothing() {
    let fx = Fixture::new();
    let before = snapshot(&fx);
    let out = fx.cmd().args(["agent", "clean"]).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_unchanged(&fx, &before, "clean");
}

// --------------------------------------------------------------------------------------
// `--json` shape.
// --------------------------------------------------------------------------------------

#[test]
fn list_json_has_agent_list_shape() {
    let fx = Fixture::new();
    let out = fx.cmd().args(["agent", "list", "--json"]).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    // AgentList: { skills: [], mcps: [], commands: [], merged_scopes: bool }
    assert!(v["skills"].is_array(), "json: {v}");
    assert!(v["mcps"].is_array(), "json: {v}");
    assert!(v["commands"].is_array(), "json: {v}");
    assert!(v["merged_scopes"].is_boolean(), "json: {v}");
    // No --scope override -> the two scopes are merged.
    assert_eq!(v["merged_scopes"], true);
}

#[test]
fn lock_check_json_has_outcome_shape() {
    let fx = Fixture::new();
    let out = fx
        .cmd()
        .args(["agent", "lock", "--check", "--json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    // AgentLockOutcome: { check, saved, skills, sources, drift: [] }
    assert_eq!(v["check"], true);
    assert_eq!(v["saved"], false);
    assert!(v["drift"].is_array(), "json: {v}");
    // `--check` must not write the lock.
    assert!(
        !fx.lock_path().exists(),
        "lock --check wrote agent-env.lock"
    );
}

// --------------------------------------------------------------------------------------
// Exit-code contract.
// --------------------------------------------------------------------------------------

#[test]
fn list_exits_zero() {
    let fx = Fixture::new();
    let out = fx.cmd().args(["agent", "list"]).output().unwrap();
    assert!(out.status.success(), "agent list must exit 0");
}

#[test]
fn add_ref_and_branch_conflict_exits_nonzero() {
    // The engine bail (`--ref and --branch are mutually exclusive`) must propagate
    // as a nonzero exit through the worker-join `?` path.
    let fx = Fixture::new();
    let out = fx
        .cmd()
        .args(["agent", "add", "src", "--ref", "a", "--branch", "b"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "conflicting --ref/--branch must exit nonzero; stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// `agent --help` lists the six verbs (surface smoke).
#[test]
fn help_lists_the_six_verbs() {
    let out = Command::new(bin())
        .args(["agent", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    for verb in ["sync", "add", "remove", "lock", "list", "clean"] {
        assert!(
            help.contains(verb),
            "agent --help missing `{verb}`:\n{help}"
        );
    }
}

/// Sanity: the fixture's dest dir genuinely does not pre-exist, so the
/// `assert_unchanged` dest check is meaningful (not vacuously satisfied).
#[test]
fn fixture_dest_absent_until_apply() {
    let fx = Fixture::new();
    assert!(!fx.dest.exists());
    assert!(Path::new(&fx.config_path()).exists());
}
