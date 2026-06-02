//! `envctl.lock` — a committed, content-hashed manifest-of-record (kasetto §2,
//! adapted). Each component gets an OS-invariant content hash of its manifest
//! spec (canonical JSON → FNV-1a, no extra deps). The lock gives:
//!   * reproducibility — the exact component set + spec hashes are pinned;
//!   * a CI gate — `envctl lock --check` fails (nonzero) if the manifest drifted
//!     from the lock (a component was added/removed/changed without re-locking).
//! Deterministic + diff-friendly (BTreeMap → stable TOML). No run-specific data
//! lives here (timings/last-run belong in machine-local cache).
use crate::component::Component;
use crate::model::Registry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const LOCK_VERSION: u8 = 1;
pub const LOCK_FILENAME: &str = "envctl.lock";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LockFile {
    #[serde(default = "default_version")]
    pub version: u8,
    #[serde(default)]
    pub components: BTreeMap<String, LockEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LockEntry {
    /// OS-invariant content hash of the component's manifest spec.
    #[serde(default)]
    pub content_hash: String,
    /// Sorted `requires` (so an edge change shows as a diff).
    #[serde(default)]
    pub requires: Vec<String>,
    /// Concretely resolved revision (git SHA for add-repo; empty otherwise).
    #[serde(default)]
    pub resolved: String,
}

impl Default for LockFile {
    fn default() -> Self {
        LockFile { version: LOCK_VERSION, components: BTreeMap::new() }
    }
}
fn default_version() -> u8 {
    LOCK_VERSION
}

pub fn lock_path(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join(LOCK_FILENAME)
}

impl LockFile {
    /// Load `<manifest_dir>/envctl.lock`. A missing lock is NOT an error (returns
    /// an empty lock); only a present-but-corrupt one errors.
    pub fn load(manifest_dir: &Path) -> anyhow::Result<LockFile> {
        let path = lock_path(manifest_dir);
        match std::fs::read_to_string(&path) {
            Ok(t) => toml::from_str(&t).map_err(|e| anyhow::anyhow!("corrupt lock {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LockFile::default()),
            Err(e) => Err(anyhow::anyhow!("reading lock {}: {e}", path.display())),
        }
    }

    /// Atomically write the lock (temp + rename), re-stamping the version.
    pub fn save(&mut self, manifest_dir: &Path) -> anyhow::Result<()> {
        self.version = LOCK_VERSION;
        std::fs::create_dir_all(manifest_dir)?;
        let target = lock_path(manifest_dir);
        let text = toml::to_string_pretty(self)?;
        let tmp = manifest_dir.join(format!(".{LOCK_FILENAME}.tmp.{}", std::process::id()));
        std::fs::write(&tmp, text.as_bytes())?;
        std::fs::rename(&tmp, &target).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            anyhow::anyhow!("writing lock {}: {e}", target.display())
        })?;
        Ok(())
    }
}

/// Build a lock from the current manifest: content-hash each component's spec.
pub fn generate(reg: &Registry) -> LockFile {
    let mut lf = LockFile::default();
    for c in reg.ordered() {
        let mut requires = c.requires.clone();
        requires.sort();
        lf.components.insert(
            c.id.clone(),
            LockEntry { content_hash: component_hash(c), requires, resolved: String::new() },
        );
    }
    lf
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LockDriftKind {
    Added,
    Removed,
    Changed,
}

/// Diff the manifest's current state against the committed lock.
pub fn diff(reg: &Registry, lock: &LockFile) -> Vec<(String, LockDriftKind)> {
    let cur = generate(reg);
    let mut out = Vec::new();
    for (id, e) in &cur.components {
        match lock.components.get(id) {
            None => out.push((id.clone(), LockDriftKind::Added)),
            Some(le) if le.content_hash != e.content_hash => out.push((id.clone(), LockDriftKind::Changed)),
            _ => {}
        }
    }
    for id in lock.components.keys() {
        if !cur.components.contains_key(id) {
            out.push((id.clone(), LockDriftKind::Removed));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// OS-invariant content hash of a component's manifest spec.
pub fn component_hash(c: &Component) -> String {
    let v = serde_json::to_value(c).unwrap_or(serde_json::Value::Null);
    fnv1a_hex(canonical(&v).as_bytes())
}

/// Canonical, key-sorted serialization of a JSON value (so authoring-order /
/// map-iteration-order never changes the hash). Arrays keep order (manifest is
/// the source of truth); object keys are sorted.
fn canonical(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys.iter().map(|k| format!("{k:?}:{}", canonical(&m[*k]))).collect();
            format!("{{{}}}", parts.join(","))
        }
        serde_json::Value::Array(a) => {
            format!("[{}]", a.iter().map(canonical).collect::<Vec<String>>().join(","))
        }
        serde_json::Value::String(s) => format!("{s:?}"),
        other => other.to_string(),
    }
}

fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn reg() -> Registry {
        Registry::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../manifest")).unwrap()
    }
    #[test]
    fn generate_is_deterministic_and_diff_clean() {
        let r = reg();
        let a = generate(&r);
        let b = generate(&r);
        // same manifest -> identical hashes (OS-invariant determinism)
        assert_eq!(a.components, b.components);
        assert!(a.components.len() == r.len() && !a.components.is_empty());
        // a lock generated from the manifest has NO drift against it
        assert!(diff(&r, &a).is_empty());
    }
    #[test]
    fn diff_detects_changed_and_removed() {
        let r = reg();
        let mut lock = generate(&r);
        // tamper one hash -> Changed; drop one -> Removed
        let bun = lock.components.get_mut("bun").unwrap();
        bun.content_hash = "deadbeef".into();
        lock.components.remove("rustup");
        let d = diff(&r, &lock);
        assert!(d.iter().any(|(id, k)| id == "bun" && *k == LockDriftKind::Changed));
        assert!(d.iter().any(|(id, k)| id == "rustup" && *k == LockDriftKind::Added));
    }
}
