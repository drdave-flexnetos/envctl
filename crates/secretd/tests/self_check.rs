//! Integration coverage for the non-serving `secretd --self-check` pre-flight (the envctl manifest
//! `verify` predicate). Runs the COMPILED binary (`CARGO_BIN_EXE_secretd`) so the real CLI surface
//! and EXIT CODES are exercised — exit 0 = healthy, non-zero = fail-closed. The harness points HOME +
//! the XDG base-dir vars at a per-test scratch dir so the checks never touch the developer's real
//! `~/.config/env-ctl` and never collide with a running daemon.
use std::path::{Path, PathBuf};
use std::process::Command;

/// A clean, unique scratch HOME under cargo's per-test temp dir.
fn scratch(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("run")).unwrap();
    dir
}

/// `secretd --self-check` with the environment scrubbed to the scratch HOME/XDG roots.
fn self_check_cmd(home: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_secretd"));
    c.arg("--self-check")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"));
    c
}

#[test]
fn self_check_passes_on_default_inmem() {
    let home = scratch("self_check_ok");
    let out = self_check_cmd(&home).output().expect("run secretd --self-check");
    assert!(
        out.status.success(),
        "self-check should pass on a clean default (in-memory) config; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn self_check_fails_closed_on_unsafe_libsql_url() {
    let home = scratch("self_check_bad_url");
    // A non-loopback PLAINTEXT libSQL URL is refused (FS-S7): the daemon would fail to start, so the
    // self-check must exit non-zero rather than report a false "healthy".
    let out = self_check_cmd(&home)
        .env("SECRETD_STORE_BACKEND", "libsql")
        .env("SECRETD_LIBSQL_URL", "http://db.turso.io:8080")
        .output()
        .expect("run secretd --self-check");
    assert!(
        !out.status.success(),
        "self-check must fail closed on an unsafe (non-loopback) libSQL URL"
    );
}
