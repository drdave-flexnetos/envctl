//! The fail-closed guard engine — the Rust port of `ubuntu-boot-repair.sh`'s
//! `resolve_verified()` + refusal chain (L73-105). Evaluated before any
//! destructive phase. The cardinal rule: **when uncertain, REFUSE.** A guard that
//! cannot resolve, re-verify, or prove its precondition returns `Some(reason)`
//! (→ `OpStatus::Refused`) — it NEVER silently passes. If `blkid`/`findmnt` are
//! missing or error, that is treated as "cannot prove safe" → refuse.
use crate::component::{Guard, Hook, HookRunner, Phase};
use crate::error::RunContext;
use crate::event::EventSink;
use crate::model::{OpResult, OpStatus};
use std::path::Path;
use std::process::Command;

/// `Some(reason)` if ANY guard refuses; `None` only if every guard affirmatively
/// passes.
pub fn check_guards(guards: &[Guard], runner: &dyn HookRunner, ctx: &RunContext) -> Option<String> {
    for g in guards {
        if let Some(reason) = check_one(g, runner, ctx) {
            return Some(reason);
        }
    }
    None
}

fn check_one(g: &Guard, runner: &dyn HookRunner, ctx: &RunContext) -> Option<String> {
    match g {
        Guard::PathExists { path } => {
            let p = expand_tilde(path);
            if Path::new(&p).exists() {
                None
            } else {
                Some(format!("refused: required path missing: {path}"))
            }
        }

        Guard::HookSucceeds { hook } => {
            let r = runner.run("<guard>", Phase::Detect, hook, false, &crate::event::EventSink::null());
            if r.status == OpStatus::Ok {
                None
            } else {
                Some(format!("refused: guard hook did not succeed ({})", r.message))
            }
        }

        // Resolve a device by UUID and RE-VERIFY it carries that UUID. Any
        // failure to resolve or re-verify => refuse (fail-closed).
        Guard::UuidResolves { uuid } => match resolve_dev(uuid) {
            Some(dev) if uuid_of(&dev).as_deref() == Some(uuid.as_str()) => None,
            Some(dev) => Some(format!(
                "refused: UUID {uuid} resolved to {dev} but re-verify did not match"
            )),
            None => Some(format!(
                "refused: UUID {uuid} did not resolve (blkid unavailable or unknown)"
            )),
        },

        // Refuse if this UUID/device IS the live/running root.
        Guard::NotLiveDevice { uuid } => {
            if ctx.live_root_uuid.as_deref() == Some(uuid.as_str()) {
                return Some(format!("refused: {uuid} is the LIVE root filesystem"));
            }
            match (resolve_dev(uuid), live_root_source()) {
                (Some(dev), Some(live)) if dev == live => {
                    Some(format!("refused: {uuid} resolves to the live device {live}"))
                }
                (None, _) => Some(format!(
                    "refused: {uuid} did not resolve — cannot prove it is not the live device"
                )),
                _ => None,
            }
        }

        // Refuse if the UUID is currently mounted anywhere (the "never umount
        // /home"). If we cannot run findmnt, we cannot prove it is unmounted → refuse.
        Guard::NotMounted { uuid } => {
            if findmnt_missing() {
                return Some(format!(
                    "refused: cannot run findmnt to prove {uuid} is unmounted"
                ));
            }
            if uuid_is_mounted(uuid) {
                Some(format!("refused: {uuid} is currently mounted"))
            } else {
                None
            }
        }
    }
}

// ---- helpers (each treats a tool failure as the conservative/refusing branch) --

fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    p.to_string()
}

/// `blkid -U <uuid>` → device path, or None.
fn resolve_dev(uuid: &str) -> Option<String> {
    let out = Command::new("blkid").args(["-U", uuid]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// `blkid -s UUID -o value <dev>` → its UUID, or None.
fn uuid_of(dev: &str) -> Option<String> {
    let out = Command::new("blkid")
        .args(["-s", "UUID", "-o", "value", dev])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// `findmnt -no SOURCE /` → the live root device.
fn live_root_source() -> Option<String> {
    let out = Command::new("findmnt")
        .args(["-no", "SOURCE", "/"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn findmnt_missing() -> bool {
    which::which("findmnt").is_err()
}

/// True if the UUID is mounted (checked by source-device AND by UUID, to catch
/// by-uuid / mapper / bind mounts — same belt-and-suspenders as boot-repair).
fn uuid_is_mounted(uuid: &str) -> bool {
    if let Some(dev) = resolve_dev(uuid) {
        if let Ok(out) = Command::new("findmnt").args(["-S", &dev]).output() {
            if out.status.success() {
                return true;
            }
        }
    }
    if let Ok(out) = Command::new("findmnt")
        .args(["--source", &format!("UUID={uuid}")])
        .output()
    {
        if out.status.success() {
            return true;
        }
    }
    false
}

/// Resolve the live root UUID once, for `RunContext` (findmnt / → blkid UUID).
pub fn resolve_live_root_uuid() -> Option<String> {
    live_root_source().and_then(|dev| uuid_of(&dev))
}

/// A no-op HookRunner (every hook → Failed). `verify_path_uuid` only exercises
/// UuidResolves/NotLiveDevice (which don't touch the runner), so this just
/// satisfies `check_one`'s signature.
struct NullRunner;
impl HookRunner for NullRunner {
    fn run(&self, comp: &str, phase: Phase, _h: &Hook, _d: bool, _s: &EventSink) -> OpResult {
        OpResult {
            component: comp.into(),
            phase,
            status: OpStatus::Failed,
            exit_code: None,
            duration_ms: 0,
            message: "null runner".into(),
            dry_run: false,
        }
    }
}

/// Fail-closed UUID re-verify for a `--purge` target: the path must exist, its
/// UUID must resolve + re-verify, it must NOT be the live root, and the mount
/// carrying the path must actually report the declared UUID. Returns
/// `Some(reason)` to REFUSE (never deletes on uncertainty).
pub fn verify_path_uuid(path: &str, uuid: &str, ctx: &RunContext) -> Option<String> {
    let p = expand_tilde(path);
    if !Path::new(&p).exists() {
        return Some(format!("refused: purge target missing: {path}"));
    }
    if let Some(r) = check_one(&Guard::UuidResolves { uuid: uuid.into() }, &NullRunner, ctx) {
        return Some(r);
    }
    if let Some(r) = check_one(&Guard::NotLiveDevice { uuid: uuid.into() }, &NullRunner, ctx) {
        return Some(r);
    }
    match mount_uuid_of(&p) {
        Some(f) if f == uuid => None,
        Some(f) => Some(format!("refused: {path} is on UUID {f}, not the declared {uuid}")),
        None => Some(format!("refused: cannot determine the fs UUID carrying {path}")),
    }
}

/// `findmnt -no SOURCE --target <path>` → device, then its UUID.
fn mount_uuid_of(path: &str) -> Option<String> {
    let out = Command::new("findmnt")
        .args(["-no", "SOURCE", "--target", path])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let dev = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if dev.is_empty() {
        None
    } else {
        uuid_of(&dev)
    }
}
