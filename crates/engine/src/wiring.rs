//! Idempotent `apply()`/`revert()` of declarative `Wiring`. The single owner of
//! guarded `BEGIN/END` shell-rc marker blocks (the wizard's idiom) — it backs up
//! before clobber and, on revert, excises ONLY the block it owns, leaving all
//! other lines (incl. blocks the engine didn't write) intact.
//!
//! Phase 0 implements the shell_rc path (the highest-value, reversible one).
//! `path_entries` are realized via a shell_rc export the manifest declares;
//! desktop_entries / systemd_user / system-scope kinds (/etc/nix, /etc/apt,
//! /etc/cdi, update-alternatives) are Phase 3 (see ROADMAP.md).
use crate::model::{ShellRcBlock, Wiring};
use std::path::Path;

pub fn apply(w: &Wiring) -> anyhow::Result<()> {
    for blk in &w.shell_rc {
        apply_shell_rc(blk)?;
    }
    Ok(())
}

pub fn revert(w: &Wiring) -> anyhow::Result<()> {
    for blk in &w.shell_rc {
        revert_shell_rc(blk)?;
    }
    Ok(())
}

fn markers(marker: &str) -> (String, String) {
    (
        format!("# >>> BEGIN {marker} (added by envctl) >>>"),
        format!("# <<< END {marker} <<<"),
    )
}

fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    p.to_string()
}

fn apply_shell_rc(blk: &ShellRcBlock) -> anyhow::Result<()> {
    let file = expand_tilde(&blk.file);
    let (begin, end) = markers(&blk.marker);
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if existing.contains(&begin) {
        return Ok(()); // idempotent: already present
    }
    if Path::new(&file).exists() {
        let _ = std::fs::copy(&file, format!("{file}.bak.{}", now_epoch()));
    }
    let block = format!("\n{begin}\n{}\n{end}\n", blk.content.trim_end());
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?;
    f.write_all(block.as_bytes())?;
    Ok(())
}

fn revert_shell_rc(blk: &ShellRcBlock) -> anyhow::Result<()> {
    let file = expand_tilde(&blk.file);
    let (begin, end) = markers(&blk.marker);
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Ok(());
    };
    if !text.contains(&begin) {
        return Ok(());
    }
    let _ = std::fs::copy(&file, format!("{file}.bak.{}", now_epoch()));
    let mut out = String::new();
    let mut skip = false;
    for line in text.lines() {
        if line.contains(&begin) {
            skip = true;
            continue;
        }
        if line.contains(&end) {
            skip = false;
            continue;
        }
        if !skip {
            out.push_str(line);
            out.push('\n');
        }
    }
    std::fs::write(&file, out)?;
    Ok(())
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
