//! Machine-local runtime state (kasetto §16): the last-run record + timings, kept
//! OUT of the committed manifest/lock. Lives under `$XDG_CACHE_HOME/envctl/<key>/`
//! keyed by a hash of the manifest dir, so multiple checkouts don't collide.
//! Best-effort: a read/write failure is never fatal.
use crate::component::Phase;
use crate::model::RunSummary;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimeState {
    #[serde(default)]
    pub last_run: Option<LastRun>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LastRun {
    pub verb: String,
    pub at: String, // RFC3339
    pub ok: bool,
    pub failed: usize,
    pub refused: usize,
    pub incomplete: usize,
    pub total: usize,
}

fn verb_of(phase: Phase) -> &'static str {
    match phase {
        Phase::Install => "install",
        Phase::Remove => "reset",
        Phase::Fix => "auto-fix",
        Phase::Detect => "auto-detect",
        Phase::Verify => "verify",
    }
}

fn cache_base() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_CACHE_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x);
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(".cache")
}

/// `$XDG_CACHE_HOME/envctl/<hash-of-manifest-dir>/state.json`.
fn state_path(manifest_dir: &Path) -> PathBuf {
    let canon = std::fs::canonicalize(manifest_dir).unwrap_or_else(|_| manifest_dir.to_path_buf());
    let mut h: u64 = 0xcbf29ce484222325;
    for b in canon.to_string_lossy().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    cache_base().join("envctl").join(format!("{h:016x}")).join("state.json")
}

pub fn load(manifest_dir: &Path) -> RuntimeState {
    std::fs::read_to_string(state_path(manifest_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Record a completed (non-dry-run) run. Best-effort; failures are ignored.
pub fn record_run(manifest_dir: &Path, phase: Phase, summary: &RunSummary) {
    let path = state_path(manifest_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let st = RuntimeState {
        last_run: Some(LastRun {
            verb: verb_of(phase).into(),
            at: chrono::Utc::now().to_rfc3339(),
            ok: summary.ok(),
            failed: summary.failed.len(),
            refused: summary.refused.len(),
            incomplete: summary.incomplete.len(),
            total: summary.results.len(),
        }),
    };
    if let Ok(text) = serde_json::to_string_pretty(&st) {
        let _ = std::fs::write(&path, text);
    }
}
