//! Pure, **Engine-free** verb drivers — the reusable orchestration helpers ported from
//! kasetto v3.2.0 `src/commands/{sync/{skills,commands,mcps},lock,list,clean,add}.rs`
//! (TASK-0013, Risk-1 split).
//!
//! These are the non-printing, non-`Engine` halves of the agent-asset verbs: they fetch /
//! hash / merge / prune and fill a [`SyncResult`] (counters + per-asset [`Action`] log),
//! but they emit no `Event`s and never `println!`. The envctl engine (`crates/engine/src/agent`)
//! wraps each one, turns the returned `actions` into `Event::AgentAction`s, and owns the
//! preview-vs-apply / Locked-zero-network policy by passing the right [`DriverCtx`].
//!
//! Faithfulness: the per-source fetch decision, the `--locked` fail-closed guard, and the
//! **never-prune-on-failure** rule (`remove_stale` only when `summary.failed == 0`) are ported
//! line-for-line. The MCP merge stays additive (`merge_mcp_config`) and `clean` only removes
//! lock-tracked MCP servers — pre-existing global servers are never touched.
//!
//! The kasetto `LockFile`/`State`/`SkillEntry` split collapses here onto the single
//! [`AgentLockFile`] (its `skills` map of [`AgentLockEntry`] *is* the old `State.skills`),
//! and the spinner/`ui` layer is dropped.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent::{
    all_mcp_project_targets, all_mcp_settings_targets, CommandTarget, McpSettingsTarget,
};
use crate::command::{apply_command, destination_path};
use crate::config::{
    CommandEntry, CommandsField, Config, McpEntry, McpsField, Scope, SkillTarget, SkillsField,
    SourceSpec,
};
use crate::config_edit::{Pin, Section, Selector, SourceItem};
use crate::dirs::{dirs_agent_env_config, dirs_home};
use crate::fsops::{
    copy_dir, relativize_dest, resolve_command_targets, resolve_dest, resolve_destinations,
    resolve_mcp_settings_targets, scope_root, select_targets, BrokenSkill,
};
use crate::hash::{hash_dir, hash_file};
use crate::lock::{AgentLockEntry, AgentLockFile, AssetEntry, LockMode};
use crate::mcp::{merge_mcp_config, remove_mcp_server, servers_present_in_settings};
use crate::profile::{format_updated_ago, read_skill_profile, read_skill_profile_from_dir};
use crate::report::{Action, InstalledSkill, Summary};
use crate::source::{
    derive_browse_url, discover_commands, discover_mcps, materialize_source, resolve_command_entry,
    resolve_mcp_entry, BrowseDerived,
};
use crate::sync::{
    command_action_label, command_asset_id, mcp_action_label, mcp_asset_id, remove_stale,
    skill_key, StaleEntry,
};
use crate::util::{now_unix, now_unix_str};
use crate::{err, Result};

/// The mutable bookkeeping threaded through a sync run (the lock, summary, and action log).
/// The kasetto `RuntimeState` updated-at timestamps live alongside the lock here.
pub struct SyncResult {
    pub summary: Summary,
    pub actions: Vec<Action>,
}

impl SyncResult {
    fn new() -> Self {
        SyncResult {
            summary: Summary::default(),
            actions: Vec::new(),
        }
    }
}

/// Immutable per-run context for the sync driver (the engine builds this from the resolved
/// config + scope + the apply/lock-mode policy). Mirrors kasetto's `SyncContext` minus the
/// `ui`/`animate`/`plain`/`json` presentation flags (the engine emits Events instead).
pub struct DriverCtx<'a> {
    pub cfg: &'a Config,
    pub cfg_dir: &'a Path,
    pub destinations: &'a [PathBuf],
    pub scope_root: PathBuf,
    pub scope: Scope,
    /// `false` performs real writes; `true` is preview-only (no fetch-driven writes).
    pub dry_run: bool,
    /// `--update`: re-resolve moving refs and rewrite locked hashes.
    pub update: bool,
    /// `--update <name>...`: when non-empty, only sources providing these names are re-resolved.
    pub update_only: Vec<String>,
    /// `--locked`/`--frozen`: never fetch; error if the lock cannot satisfy the config.
    pub locked: bool,
}

impl DriverCtx<'_> {
    /// Build a `DriverCtx` from the resolved config + the lock mode + apply flag.
    pub fn from_mode<'a>(
        cfg: &'a Config,
        cfg_dir: &'a Path,
        destinations: &'a [PathBuf],
        scope_root: PathBuf,
        scope: Scope,
        apply: bool,
        lock_mode: &LockMode,
    ) -> DriverCtx<'a> {
        let (update, update_only, locked) = match lock_mode {
            LockMode::Plain => (false, Vec::new(), false),
            LockMode::Update(names) => (true, names.clone(), false),
            LockMode::Locked => (false, Vec::new(), true),
        };
        DriverCtx {
            cfg,
            cfg_dir,
            destinations,
            scope_root,
            scope,
            dry_run: !apply,
            update,
            update_only,
            locked,
        }
    }

    fn update_active_for_source(&self, desired: &[String]) -> bool {
        if !self.update {
            return false;
        }
        if self.update_only.is_empty() {
            return true;
        }
        desired.iter().any(|s| self.update_only.contains(s))
    }
}

/// A loaded runtime/updated-at memo for set/forget during a sync. Replaces the threading
/// of the `RuntimeState` into the kasetto per-kind helpers.
#[derive(Default)]
pub struct UpdatedAt(pub BTreeMap<String, String>);

impl UpdatedAt {
    fn set(&mut self, id: &str, ts: String) {
        self.0.insert(id.to_string(), ts);
    }
    fn forget(&mut self, id: &str) {
        self.0.remove(id);
    }
}

/// Drive a full sync (skills → commands → MCPs) against the lock, in place.
///
/// Returns the per-run [`SyncResult`]. The lock and `updated_at` memo are mutated when
/// `ctx.dry_run` is false; in preview mode nothing on disk or in the lock changes.
pub fn sync(ctx: &DriverCtx, lock: &mut AgentLockFile, updated: &mut UpdatedAt) -> SyncResult {
    let mut res = SyncResult::new();
    sync_skills(ctx, lock, updated, &mut res);
    sync_commands(ctx, lock, &mut res);
    sync_mcps(ctx, lock, &mut res);
    res
}

// ===================================================================================
// Skills (kasetto commands/sync/skills.rs)
// ===================================================================================

#[derive(Default)]
struct HashCache(HashMap<PathBuf, Option<String>>);

impl HashCache {
    fn get(&mut self, dir: &Path) -> Option<String> {
        self.0
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                if dir.exists() {
                    hash_dir(dir).ok()
                } else {
                    None
                }
            })
            .clone()
    }
    fn set(&mut self, dir: PathBuf, hash: String) {
        self.0.insert(dir, Some(hash));
    }
    fn invalidate(&mut self, dir: &Path) {
        self.0.insert(dir.to_path_buf(), None);
    }
}

struct DestStatus {
    all_match: bool,
    good: Option<PathBuf>,
}

fn dest_status(
    ctx: &DriverCtx,
    cache: &mut HashCache,
    skill_name: &str,
    expected_hash: &str,
) -> DestStatus {
    let mut all_match = true;
    let mut good = None;
    for agent_dest in ctx.destinations {
        let dir = agent_dest.join(skill_name);
        if cache.get(&dir).as_deref() == Some(expected_hash) {
            if good.is_none() {
                good = Some(dir);
            }
        } else {
            all_match = false;
        }
    }
    DestStatus { all_match, good }
}

fn sync_skills(
    ctx: &DriverCtx,
    lock: &mut AgentLockFile,
    updated: &mut UpdatedAt,
    res: &mut SyncResult,
) {
    let mut desired_keys = HashSet::new();
    let mut cache = HashCache::default();

    for (i, src) in ctx.cfg.skills.iter().enumerate() {
        let desired = desired_skill_names(src, lock);

        if ctx.locked {
            if let Err(e) = ensure_locked_satisfiable(src, &desired, lock) {
                res.summary.failed += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: None,
                    status: "locked_error".into(),
                    error: Some(e.to_string()),
                });
                continue;
            }
        }

        let update_active = ctx.update_active_for_source(&desired);
        let fetch = update_active || needs_fetch_skills(ctx, &mut cache, src, &desired, lock);

        if fetch && ctx.locked {
            res.summary.failed += 1;
            res.actions.push(Action {
                source: Some(src.source.clone()),
                skill: None,
                status: "locked_error".into(),
                error: Some(
                    "lock requires a fetch to satisfy this source, but --locked forbids fetching"
                        .into(),
                ),
            });
            continue;
        }

        if fetch {
            sync_skill_source_via_fetch(
                ctx,
                lock,
                updated,
                res,
                &mut cache,
                &mut desired_keys,
                src,
                i,
            );
        } else {
            sync_skill_source_from_lock(
                ctx,
                lock,
                updated,
                res,
                &mut cache,
                &mut desired_keys,
                src,
                &desired,
            );
        }
    }

    if res.summary.failed == 0 {
        remove_stale_skills(ctx, lock, updated, res, &desired_keys);
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_skill_source_via_fetch(
    ctx: &DriverCtx,
    lock: &mut AgentLockFile,
    updated: &mut UpdatedAt,
    res: &mut SyncResult,
    cache: &mut HashCache,
    desired_keys: &mut HashSet<String>,
    src: &SourceSpec,
    i: usize,
) {
    let stage = std::env::temp_dir().join(format!("envctl-agent-{}-{}", now_unix(), i));
    match materialize_source(src, ctx.cfg_dir, &stage) {
        Ok(materialized) => {
            match select_targets(
                &src.skills,
                &materialized.available,
                &materialized.source_root,
            ) {
                Ok((targets, broken_skills)) => {
                    record_broken_skills(&src.source, broken_skills, res);
                    for (skill_name, skill_path) in targets {
                        if let Err(e) = process_single_skill(
                            ctx,
                            lock,
                            updated,
                            res,
                            cache,
                            desired_keys,
                            &src.source,
                            &materialized.source_revision,
                            &skill_name,
                            &skill_path,
                        ) {
                            res.summary.failed += 1;
                            res.actions.push(Action {
                                source: Some(src.source.clone()),
                                skill: Some(skill_name),
                                status: "source_error".into(),
                                error: Some(e.to_string()),
                            });
                        }
                    }
                }
                Err(e) => {
                    res.summary.failed += 1;
                    res.actions.push(Action {
                        source: Some(src.source.clone()),
                        skill: None,
                        status: "source_error".into(),
                        error: Some(e.to_string()),
                    });
                }
            }
            if let Some(cleanup_dir) = materialized.cleanup_dir {
                let _ = fs::remove_dir_all(cleanup_dir);
            }
        }
        Err(e) => {
            res.summary.failed += 1;
            res.actions.push(Action {
                source: Some(src.source.clone()),
                skill: None,
                status: "source_error".into(),
                error: Some(e.to_string()),
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_skill_source_from_lock(
    ctx: &DriverCtx,
    lock: &mut AgentLockFile,
    updated: &mut UpdatedAt,
    res: &mut SyncResult,
    cache: &mut HashCache,
    desired_keys: &mut HashSet<String>,
    src: &SourceSpec,
    desired: &[String],
) {
    for skill_name in desired {
        let key = skill_key(&src.source, skill_name);
        desired_keys.insert(key.clone());
        let Some(entry) = lock.skills.get(&key).cloned() else {
            continue;
        };
        if let Err(e) = process_locked_skill(ctx, updated, res, cache, &entry, skill_name) {
            res.summary.failed += 1;
            res.actions.push(Action {
                source: Some(src.source.clone()),
                skill: Some(skill_name.clone()),
                status: "source_error".into(),
                error: Some(e.to_string()),
            });
        }
    }
}

fn record_broken_skills(source: &str, broken_skills: Vec<BrokenSkill>, res: &mut SyncResult) {
    for broken in broken_skills {
        res.summary.broken += 1;
        res.actions.push(Action {
            source: Some(source.to_string()),
            skill: Some(broken.name),
            status: "broken".into(),
            error: Some(broken.reason),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn process_single_skill(
    ctx: &DriverCtx,
    lock: &mut AgentLockFile,
    updated: &mut UpdatedAt,
    res: &mut SyncResult,
    cache: &mut HashCache,
    desired_keys: &mut HashSet<String>,
    source: &str,
    source_revision: &str,
    skill_name: &str,
    skill_path: &Path,
) -> Result<()> {
    let destination = &ctx.destinations[0];
    let (_, profile_description) = read_skill_profile_from_dir(skill_path, skill_name);
    let key = skill_key(source, skill_name);
    desired_keys.insert(key.clone());
    let hash = hash_dir(skill_path)?;
    let dest = destination.join(skill_name);

    let is_unchanged = lock
        .skills
        .get(&key)
        .map(|prev| prev.hash == hash && dest_status(ctx, cache, skill_name, &prev.hash).all_match)
        .unwrap_or(false);

    if is_unchanged {
        if !ctx.dry_run {
            if let Some(entry) = lock.skills.get_mut(&key) {
                entry.description = profile_description.clone();
            }
        }
        res.summary.unchanged += 1;
        res.actions.push(Action {
            source: Some(source.to_string()),
            skill: Some(skill_name.to_string()),
            status: "unchanged".into(),
            error: None,
        });
        return Ok(());
    }

    if ctx.dry_run {
        let status = if lock.skills.contains_key(&key) {
            res.summary.updated += 1;
            "would_update"
        } else {
            res.summary.installed += 1;
            "would_install"
        };
        res.actions.push(Action {
            source: Some(source.to_string()),
            skill: Some(skill_name.to_string()),
            status: status.into(),
            error: None,
        });
        return Ok(());
    }

    for agent_dest in ctx.destinations {
        let dst = agent_dest.join(skill_name);
        cache.invalidate(&dst);
        copy_dir(skill_path, &dst)?;
        cache.set(dst, hash.clone());
    }
    let status = if lock.skills.contains_key(&key) {
        res.summary.updated += 1;
        "updated"
    } else {
        res.summary.installed += 1;
        "installed"
    };
    updated.set(&key, now_unix_str());
    lock.skills.insert(
        key,
        AgentLockEntry {
            destination: relativize_dest(&dest, &ctx.scope_root),
            hash,
            skill: skill_name.to_string(),
            description: profile_description.clone(),
            source: source.to_string(),
            source_revision: source_revision.to_string(),
            scope: Some(ctx.scope),
        },
    );
    res.actions.push(Action {
        source: Some(source.to_string()),
        skill: Some(skill_name.to_string()),
        status: status.into(),
        error: None,
    });
    Ok(())
}

fn process_locked_skill(
    ctx: &DriverCtx,
    updated: &mut UpdatedAt,
    res: &mut SyncResult,
    cache: &mut HashCache,
    entry: &AgentLockEntry,
    skill_name: &str,
) -> Result<()> {
    let key = skill_key(&entry.source, skill_name);
    let DestStatus { all_match, good } = dest_status(ctx, cache, skill_name, &entry.hash);

    if all_match {
        res.summary.unchanged += 1;
        res.actions.push(Action {
            source: Some(entry.source.clone()),
            skill: Some(skill_name.to_string()),
            status: "unchanged".into(),
            error: None,
        });
        return Ok(());
    }

    if ctx.dry_run {
        res.summary.updated += 1;
        res.actions.push(Action {
            source: Some(entry.source.clone()),
            skill: Some(skill_name.to_string()),
            status: "would_update".into(),
            error: None,
        });
        return Ok(());
    }

    let Some(src_dir) = good else {
        return Err(err(format!(
            "no good local copy of `{skill_name}` to repair from"
        )));
    };
    for agent_dest in ctx.destinations {
        let dst = agent_dest.join(skill_name);
        if dst != src_dir {
            cache.invalidate(&dst);
            copy_dir(&src_dir, &dst)?;
            cache.set(dst, entry.hash.clone());
        }
    }
    updated.set(&key, now_unix_str());
    res.summary.updated += 1;
    res.actions.push(Action {
        source: Some(entry.source.clone()),
        skill: Some(skill_name.to_string()),
        status: "updated".into(),
        error: None,
    });
    Ok(())
}

fn desired_skill_names(src: &SourceSpec, lock: &AgentLockFile) -> Vec<String> {
    match &src.skills {
        SkillsField::List(items) => items
            .iter()
            .map(|it| match it {
                SkillTarget::Name(n) => n.clone(),
                SkillTarget::Obj { name, .. } => name.clone(),
            })
            .collect(),
        SkillsField::Wildcard(_) => lock
            .skills
            .values()
            .filter(|e| e.source == src.source)
            .map(|e| e.skill.clone())
            .collect(),
    }
}

fn ensure_locked_satisfiable(
    src: &SourceSpec,
    desired: &[String],
    lock: &AgentLockFile,
) -> Result<()> {
    match &src.skills {
        SkillsField::List(_) => {
            for name in desired {
                let key = skill_key(&src.source, name);
                if !lock.skills.contains_key(&key) {
                    return Err(err(format!(
                        "--locked: skill `{name}` from `{}` is not in the lock",
                        src.source
                    )));
                }
            }
            Ok(())
        }
        SkillsField::Wildcard(_) => {
            let present = lock.skills.values().any(|e| e.source == src.source);
            if present {
                Ok(())
            } else {
                Err(err(format!(
                    "--locked: source `{}` has no entries in the lock",
                    src.source
                )))
            }
        }
    }
}

fn needs_fetch_skills(
    ctx: &DriverCtx,
    cache: &mut HashCache,
    src: &SourceSpec,
    desired: &[String],
    lock: &AgentLockFile,
) -> bool {
    if matches!(src.skills, SkillsField::Wildcard(_))
        && !lock.skills.values().any(|e| e.source == src.source)
    {
        return true;
    }
    let expected_revision = src.expected_revision();
    for skill_name in desired {
        let key = skill_key(&src.source, skill_name);
        let Some(entry) = lock.skills.get(&key) else {
            return true;
        };
        if !entry.source_revision.is_empty() && entry.source_revision != expected_revision {
            return true;
        }
        let status = dest_status(ctx, cache, skill_name, &entry.hash);
        if !status.all_match && status.good.is_none() {
            return true;
        }
    }
    false
}

fn remove_stale_skills(
    ctx: &DriverCtx,
    lock: &mut AgentLockFile,
    updated: &mut UpdatedAt,
    res: &mut SyncResult,
    desired_keys: &HashSet<String>,
) {
    let snapshot: Vec<(String, String, String, String)> = lock
        .skills
        .iter()
        .map(|(k, e)| {
            (
                k.clone(),
                e.source.clone(),
                e.skill.clone(),
                e.destination.clone(),
            )
        })
        .collect();
    let dest_by_id: HashMap<String, String> = snapshot
        .iter()
        .map(|(k, _, _, d)| (k.clone(), d.clone()))
        .collect();
    let candidates: Vec<StaleEntry> = snapshot
        .into_iter()
        .map(|(id, source, name, _)| StaleEntry {
            id,
            action_source: Some(source),
            action_skill: name,
        })
        .collect();

    let scope_root = ctx.scope_root.clone();
    remove_stale(
        ctx.dry_run,
        &mut res.summary,
        &mut res.actions,
        desired_keys,
        candidates,
        |id| {
            if let Some(dest) = dest_by_id.get(id) {
                let abs = resolve_dest(dest, &scope_root);
                let _ = fs::remove_dir_all(&abs);
            }
            lock.skills.remove(id);
            updated.forget(id);
        },
    );
}

// ===================================================================================
// Commands (kasetto commands/sync/commands.rs)
// ===================================================================================

struct PendingCommand {
    source: String,
    name: String,
    src_path: PathBuf,
    hash: String,
    asset_id: String,
    is_new: bool,
    source_revision: String,
}

fn sync_commands(ctx: &DriverCtx, lock: &mut AgentLockFile, res: &mut SyncResult) {
    let targets = match resolve_command_targets(ctx.cfg, ctx.scope, ctx.cfg_dir) {
        Ok(t) => t,
        Err(e) => {
            res.summary.failed += 1;
            res.actions.push(Action {
                source: None,
                skill: Some("command".into()),
                status: "source_error".into(),
                error: Some(e.to_string()),
            });
            return;
        }
    };

    if ctx.cfg.commands.is_empty() {
        remove_stale_commands(ctx, lock, res, &HashSet::new());
        return;
    }
    if targets.is_empty() {
        return;
    }

    let mut desired_ids = HashSet::new();
    let mut pending: Vec<PendingCommand> = Vec::new();
    let mut cleanup_dirs: Vec<PathBuf> = Vec::new();

    for (i, src) in ctx.cfg.commands.iter().enumerate() {
        let desired_names = desired_command_names(src, lock);

        if ctx.locked {
            if let Err(e) = ensure_locked_satisfiable_commands(src, &desired_names, lock) {
                res.summary.failed += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: None,
                    status: "locked_error".into(),
                    error: Some(e.to_string()),
                });
                continue;
            }
        }

        let update_active = ctx.update_active_for_source(&desired_names);
        let fetch = update_active || needs_fetch_commands(src, &desired_names, lock, &targets);

        if fetch && ctx.locked {
            res.summary.failed += 1;
            res.actions.push(Action {
                source: Some(src.source.clone()),
                skill: None,
                status: "locked_error".into(),
                error: Some(
                    "lock requires a fetch to satisfy this source, but --locked forbids fetching"
                        .into(),
                ),
            });
            continue;
        }

        if !fetch {
            for name in &desired_names {
                let asset_id = command_asset_id(&src.source, name);
                desired_ids.insert(asset_id);
                res.summary.unchanged += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: Some(command_action_label(name)),
                    status: "unchanged".into(),
                    error: None,
                });
            }
            continue;
        }

        let stage = std::env::temp_dir().join(format!("envctl-agent-cmd-{}-{}", now_unix(), i));
        let materialized = match materialize_source(&src.as_source_spec(), ctx.cfg_dir, &stage) {
            Ok(m) => m,
            Err(e) => {
                res.summary.failed += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: None,
                    status: "source_error".into(),
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        let root = materialized
            .cleanup_dir
            .as_deref()
            .unwrap_or(&materialized.source_root);

        let selected: Vec<(String, PathBuf)> = match &src.commands {
            CommandsField::Wildcard(s) if s == "*" => match discover_commands(root) {
                Ok(map) => map.into_iter().collect(),
                Err(e) => {
                    res.summary.broken += 1;
                    res.actions.push(Action {
                        source: Some(src.source.clone()),
                        skill: Some("command".into()),
                        status: "broken".into(),
                        error: Some(e.to_string()),
                    });
                    if let Some(d) = materialized.cleanup_dir {
                        cleanup_dirs.push(d);
                    }
                    continue;
                }
            },
            CommandsField::Wildcard(s) => {
                res.summary.broken += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: Some("command".into()),
                    status: "broken".into(),
                    error: Some(format!(
                        "invalid commands value \"{s}\": expected \"*\" or a list"
                    )),
                });
                if let Some(d) = materialized.cleanup_dir {
                    cleanup_dirs.push(d);
                }
                continue;
            }
            CommandsField::List(entries) => {
                let mut out = Vec::new();
                for entry in entries {
                    let entry_name = match entry {
                        CommandEntry::Name(n) => n.clone(),
                        CommandEntry::Obj { name, .. } => name.clone(),
                    };
                    match resolve_command_entry(root, entry) {
                        Ok(pair) => out.push(pair),
                        Err(e) => {
                            res.summary.broken += 1;
                            res.actions.push(Action {
                                source: Some(src.source.clone()),
                                skill: Some(entry_name),
                                status: "broken".into(),
                                error: Some(e.to_string()),
                            });
                        }
                    }
                }
                out
            }
        };

        if selected.is_empty() && matches!(&src.commands, CommandsField::Wildcard(s) if s == "*") {
            res.summary.broken += 1;
            res.actions.push(Action {
                source: Some(src.source.clone()),
                skill: Some("command".into()),
                status: "broken".into(),
                error: Some("no commands found in source (expected commands/*.md)".into()),
            });
        }

        for (name, src_path) in selected {
            let asset_id = command_asset_id(&src.source, &name);
            desired_ids.insert(asset_id.clone());
            let hash = match hash_file(&src_path) {
                Ok(h) => h,
                Err(e) => {
                    res.summary.broken += 1;
                    res.actions.push(Action {
                        source: Some(src.source.clone()),
                        skill: Some(command_action_label(&name)),
                        status: "broken".into(),
                        error: Some(e.to_string()),
                    });
                    continue;
                }
            };

            let expected_paths: Vec<PathBuf> =
                targets.iter().map(|t| destination_path(t, &name)).collect();
            let existing = lock.get_tracked_asset("command", &asset_id);
            let is_unchanged = existing
                .as_ref()
                .map(|(h, _)| h == &hash && expected_paths.iter().all(|p| p.exists()))
                .unwrap_or(false);

            if is_unchanged {
                res.summary.unchanged += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: Some(command_action_label(&name)),
                    status: "unchanged".into(),
                    error: None,
                });
            } else {
                pending.push(PendingCommand {
                    source: src.source.clone(),
                    name,
                    src_path,
                    hash,
                    asset_id,
                    is_new: existing.is_none(),
                    source_revision: materialized.source_revision.clone(),
                });
            }
        }

        if let Some(d) = materialized.cleanup_dir {
            cleanup_dirs.push(d);
        }
    }

    apply_pending_commands(ctx, lock, res, &targets, &pending);
    for d in cleanup_dirs {
        let _ = fs::remove_dir_all(d);
    }

    if res.summary.failed == 0 {
        remove_stale_commands(ctx, lock, res, &desired_ids);
    }
}

fn desired_command_names(
    src: &crate::config::CommandSourceSpec,
    lock: &AgentLockFile,
) -> Vec<String> {
    match &src.commands {
        CommandsField::List(entries) => entries
            .iter()
            .map(|e| match e {
                CommandEntry::Name(n) => n.clone(),
                CommandEntry::Obj { name, .. } => name.clone(),
            })
            .collect(),
        CommandsField::Wildcard(s) if s == "*" => lock
            .assets
            .values()
            .filter(|a| a.kind == "command" && a.source == src.source)
            .map(|a| a.name.clone())
            .collect(),
        CommandsField::Wildcard(_) => Vec::new(),
    }
}

fn needs_fetch_commands(
    src: &crate::config::CommandSourceSpec,
    desired: &[String],
    lock: &AgentLockFile,
    targets: &[CommandTarget],
) -> bool {
    if matches!(&src.commands, CommandsField::Wildcard(s) if s == "*")
        && !lock
            .assets
            .values()
            .any(|a| a.kind == "command" && a.source == src.source)
    {
        return true;
    }
    let expected_revision = src.as_source_spec().expected_revision();
    for name in desired {
        let asset_id = command_asset_id(&src.source, name);
        let Some(asset) = lock.assets.get(&asset_id).filter(|a| a.kind == "command") else {
            return true;
        };
        if !asset.source_revision.is_empty() && asset.source_revision != expected_revision {
            return true;
        }
        let any_missing = targets.iter().any(|t| !destination_path(t, name).exists());
        if any_missing {
            return true;
        }
    }
    false
}

fn ensure_locked_satisfiable_commands(
    src: &crate::config::CommandSourceSpec,
    desired: &[String],
    lock: &AgentLockFile,
) -> Result<()> {
    match &src.commands {
        CommandsField::List(_) => {
            for name in desired {
                let asset_id = command_asset_id(&src.source, name);
                if lock.get_tracked_asset("command", &asset_id).is_none() {
                    return Err(err(format!(
                        "--locked: command `{name}` from `{}` is not in the lock",
                        src.source
                    )));
                }
            }
            Ok(())
        }
        CommandsField::Wildcard(_) => {
            let present = lock
                .assets
                .values()
                .any(|a| a.kind == "command" && a.source == src.source);
            if present {
                Ok(())
            } else {
                Err(err(format!(
                    "--locked: source `{}` has no command entries in the lock",
                    src.source
                )))
            }
        }
    }
}

fn apply_pending_commands(
    ctx: &DriverCtx,
    lock: &mut AgentLockFile,
    res: &mut SyncResult,
    targets: &[CommandTarget],
    pending: &[PendingCommand],
) {
    for p in pending {
        let status = if !p.is_new {
            if ctx.dry_run {
                "would_update"
            } else {
                "updated"
            }
        } else if ctx.dry_run {
            "would_install"
        } else {
            "installed"
        };

        if !ctx.dry_run {
            let mut written: Vec<String> = Vec::new();
            let mut failed = false;
            for target in targets {
                match apply_command(&p.src_path, target, &p.name) {
                    Ok(dest) => written.push(relativize_dest(&dest, &ctx.scope_root)),
                    Err(e) => {
                        res.summary.failed += 1;
                        res.actions.push(Action {
                            source: Some(p.source.clone()),
                            skill: Some(command_action_label(&p.name)),
                            status: "source_error".into(),
                            error: Some(format!(
                                "failed to apply command `{}` to {}: {e}",
                                p.name,
                                target.path.display()
                            )),
                        });
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                continue;
            }
            let dest_csv = written.join(",");
            lock.save_tracked_asset(
                &p.asset_id,
                AssetEntry {
                    kind: "command".into(),
                    name: p.name.clone(),
                    hash: p.hash.clone(),
                    source: p.source.clone(),
                    destination: dest_csv,
                    source_revision: p.source_revision.clone(),
                },
            );
        }

        if status.contains("install") {
            res.summary.installed += 1;
        } else {
            res.summary.updated += 1;
        }
        res.actions.push(Action {
            source: Some(p.source.clone()),
            skill: Some(command_action_label(&p.name)),
            status: status.into(),
            error: None,
        });
    }
}

fn remove_stale_commands(
    ctx: &DriverCtx,
    lock: &mut AgentLockFile,
    res: &mut SyncResult,
    desired_ids: &HashSet<String>,
) {
    let existing: Vec<(String, String)> = lock
        .list_tracked_asset_ids("command")
        .iter()
        .map(|(id, dest)| (id.to_string(), dest.to_string()))
        .collect();
    let dest_by_id: HashMap<String, String> = existing.iter().cloned().collect();
    let candidates: Vec<StaleEntry> = existing
        .into_iter()
        .map(|(id, _)| {
            let name = lock
                .assets
                .get(&id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| id.rsplit("::").next().unwrap_or(&id).to_string());
            StaleEntry {
                id,
                action_source: None,
                action_skill: command_action_label(&name),
            }
        })
        .collect();

    let scope_root = ctx.scope_root.clone();
    remove_stale(
        ctx.dry_run,
        &mut res.summary,
        &mut res.actions,
        desired_ids,
        candidates,
        |id| {
            if let Some(dest_csv) = dest_by_id.get(id) {
                for p in dest_csv.split(',').filter(|s| !s.is_empty()) {
                    let path = resolve_dest(p, &scope_root);
                    if path.exists() && path.is_file() {
                        let _ = fs::remove_file(path);
                    }
                }
            }
            lock.remove_tracked_asset(id);
        },
    );
}

// ===================================================================================
// MCPs (kasetto commands/sync/mcps.rs)
// ===================================================================================

struct PendingMcp {
    source: String,
    file_name: String,
    mcp_path: PathBuf,
    hash: String,
    server_names: Vec<String>,
    asset_id: String,
    is_new: bool,
    source_revision: String,
}

fn file_name_str(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

fn sync_mcps(ctx: &DriverCtx, lock: &mut AgentLockFile, res: &mut SyncResult) {
    let mut desired_mcp_ids = HashSet::new();
    let mcp_settings_list = match resolve_mcp_settings_targets(ctx.cfg, ctx.scope, ctx.cfg_dir) {
        Ok(t) => t,
        Err(e) => {
            res.summary.failed += 1;
            res.actions.push(Action {
                source: None,
                skill: Some("mcp".into()),
                status: "source_error".into(),
                error: Some(e.to_string()),
            });
            return;
        }
    };

    if mcp_settings_list.is_empty() {
        let has_orphans = lock.assets.values().any(|a| a.kind == "mcp");
        if has_orphans {
            let fallback_targets: Vec<McpSettingsTarget> = match ctx.scope {
                Scope::Project => all_mcp_project_targets(&ctx.scope_root),
                Scope::Global => match (dirs_home(), dirs_agent_env_config()) {
                    (Ok(home), Ok(cfg_dir)) => all_mcp_settings_targets(&home, &cfg_dir),
                    _ => Vec::new(),
                },
            };
            remove_stale_mcps(ctx, lock, res, &desired_mcp_ids, &fallback_targets);
        }
        return;
    }

    let mut pending: Vec<PendingMcp> = Vec::new();
    let mut cleanup_dirs: Vec<PathBuf> = Vec::new();

    for (i, src) in ctx.cfg.mcps.iter().enumerate() {
        let desired_file_names = desired_mcp_file_names(src, lock);

        if ctx.locked {
            if let Err(e) = ensure_locked_satisfiable_mcps(src, &desired_file_names, lock) {
                res.summary.failed += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: None,
                    status: "locked_error".into(),
                    error: Some(e.to_string()),
                });
                continue;
            }
        }

        let update_names: Vec<String> = desired_file_names
            .iter()
            .flat_map(|f| {
                std::iter::once(f.clone()).chain(f.strip_suffix(".json").map(str::to_string))
            })
            .collect();
        let update_active = ctx.update_active_for_source(&update_names);
        let fetch =
            update_active || needs_fetch_mcps(src, &desired_file_names, lock, &mcp_settings_list);

        if fetch && ctx.locked {
            res.summary.failed += 1;
            res.actions.push(Action {
                source: Some(src.source.clone()),
                skill: None,
                status: "locked_error".into(),
                error: Some(
                    "lock requires a fetch to satisfy this source, but --locked forbids fetching"
                        .into(),
                ),
            });
            continue;
        }

        if !fetch {
            for file_name in &desired_file_names {
                let asset_id = mcp_asset_id(&src.source, file_name);
                desired_mcp_ids.insert(asset_id);
                res.summary.unchanged += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: Some(mcp_action_label(file_name)),
                    status: "unchanged".into(),
                    error: None,
                });
            }
            continue;
        }

        let stage = std::env::temp_dir().join(format!("envctl-agent-mcp-{}-{}", now_unix(), i));
        let materialized = match materialize_source(&src.as_source_spec(), ctx.cfg_dir, &stage) {
            Ok(m) => m,
            Err(e) => {
                res.summary.failed += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: None,
                    status: "source_error".into(),
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        let root = materialized
            .cleanup_dir
            .as_deref()
            .unwrap_or_else(|| Path::new(&src.source));
        let resolve_result: Result<Vec<PathBuf>> = match &src.mcps {
            McpsField::Wildcard(s) if s == "*" => discover_mcps(root),
            McpsField::Wildcard(s) => Err(err(format!(
                "invalid mcps value \"{s}\": expected \"*\" or a list"
            ))),
            McpsField::List(entries) => {
                let mut paths = Vec::new();
                for entry in entries {
                    let name = match entry {
                        McpEntry::Name(n) => n.clone(),
                        McpEntry::Obj { name, .. } => name.clone(),
                    };
                    match resolve_mcp_entry(root, entry) {
                        Ok(p) => paths.push(p),
                        Err(e) => {
                            res.summary.broken += 1;
                            res.actions.push(Action {
                                source: Some(src.source.clone()),
                                skill: Some(name),
                                status: "broken".into(),
                                error: Some(e.to_string()),
                            });
                        }
                    }
                }
                Ok(paths)
            }
        };
        let mcps = match resolve_result {
            Ok(paths) => paths,
            Err(e) => {
                res.summary.broken += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: Some("mcp".into()),
                    status: "broken".into(),
                    error: Some(e.to_string()),
                });
                if let Some(d) = materialized.cleanup_dir {
                    let _ = fs::remove_dir_all(d);
                }
                continue;
            }
        };
        if mcps.is_empty() {
            res.summary.broken += 1;
            res.actions.push(Action {
                source: Some(src.source.clone()),
                skill: Some("mcp".into()),
                status: "broken".into(),
                error: Some(
                    "no MCP JSON files found in source (expected .mcp.json, mcp.json, or mcps/*.json)"
                        .into(),
                ),
            });
            if let Some(d) = materialized.cleanup_dir {
                let _ = fs::remove_dir_all(d);
            }
            continue;
        }
        for mcp_path in &mcps {
            let file_name = file_name_str(mcp_path);
            let file_name_for_err = file_name.clone();
            let r: Result<()> = (|| {
                let hash = hash_file(mcp_path)?;
                let mcp_text = fs::read_to_string(mcp_path)?;
                let mcp_val: serde_json::Value = serde_json::from_str(&mcp_text)?;
                let server_names: Vec<String> = mcp_val
                    .get("mcpServers")
                    .and_then(|v| v.as_object())
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();

                let asset_id = mcp_asset_id(&src.source, &file_name);
                desired_mcp_ids.insert(asset_id.clone());

                let existing = lock.get_tracked_asset("mcp", &asset_id);
                let is_unchanged = existing
                    .as_ref()
                    .map(|(h, _)| {
                        h == &hash
                            && mcp_settings_list
                                .iter()
                                .all(|target| servers_present_in_settings(&server_names, target))
                    })
                    .unwrap_or(false);

                if is_unchanged {
                    res.summary.unchanged += 1;
                    res.actions.push(Action {
                        source: Some(src.source.clone()),
                        skill: Some(mcp_action_label(&file_name)),
                        status: "unchanged".into(),
                        error: None,
                    });
                } else {
                    pending.push(PendingMcp {
                        source: src.source.clone(),
                        file_name,
                        mcp_path: mcp_path.clone(),
                        hash,
                        server_names,
                        asset_id,
                        is_new: existing.is_none(),
                        source_revision: materialized.source_revision.clone(),
                    });
                }
                Ok(())
            })();
            if let Err(e) = r {
                res.summary.broken += 1;
                res.actions.push(Action {
                    source: Some(src.source.clone()),
                    skill: Some(mcp_action_label(&file_name_for_err)),
                    status: "broken".into(),
                    error: Some(e.to_string()),
                });
            }
        }
        if let Some(d) = materialized.cleanup_dir {
            cleanup_dirs.push(d);
        }
    }

    apply_pending_mcps(ctx, lock, res, &mcp_settings_list, &pending);
    for d in &cleanup_dirs {
        let _ = fs::remove_dir_all(d);
    }

    if res.summary.failed == 0 {
        remove_stale_mcps(ctx, lock, res, &desired_mcp_ids, &mcp_settings_list);
    }
}

fn desired_mcp_file_names(src: &crate::config::McpSourceSpec, lock: &AgentLockFile) -> Vec<String> {
    match &src.mcps {
        McpsField::List(entries) => entries
            .iter()
            .map(|e| {
                let name = match e {
                    McpEntry::Name(n) => n.clone(),
                    McpEntry::Obj { name, .. } => name.clone(),
                };
                format!("{name}.json")
            })
            .collect(),
        McpsField::Wildcard(s) if s == "*" => lock
            .assets
            .values()
            .filter(|a| a.kind == "mcp" && a.source == src.source)
            .map(|a| a.name.clone())
            .collect(),
        McpsField::Wildcard(_) => Vec::new(),
    }
}

fn needs_fetch_mcps(
    src: &crate::config::McpSourceSpec,
    desired_file_names: &[String],
    lock: &AgentLockFile,
    mcp_settings_list: &[McpSettingsTarget],
) -> bool {
    if matches!(&src.mcps, McpsField::Wildcard(s) if s == "*")
        && !lock
            .assets
            .values()
            .any(|a| a.kind == "mcp" && a.source == src.source)
    {
        return true;
    }
    let expected_revision = src.as_source_spec().expected_revision();
    for file_name in desired_file_names {
        let asset_id = mcp_asset_id(&src.source, file_name);
        let Some(asset) = lock.assets.get(&asset_id).filter(|a| a.kind == "mcp") else {
            return true;
        };
        if !asset.source_revision.is_empty() && asset.source_revision != expected_revision {
            return true;
        }
        let server_names: Vec<String> = asset
            .destination
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let all_present = mcp_settings_list
            .iter()
            .all(|target| servers_present_in_settings(&server_names, target));
        if !all_present {
            return true;
        }
    }
    false
}

fn ensure_locked_satisfiable_mcps(
    src: &crate::config::McpSourceSpec,
    desired_file_names: &[String],
    lock: &AgentLockFile,
) -> Result<()> {
    match &src.mcps {
        McpsField::List(_) => {
            for file_name in desired_file_names {
                let asset_id = mcp_asset_id(&src.source, file_name);
                if lock.get_tracked_asset("mcp", &asset_id).is_none() {
                    return Err(err(format!(
                        "--locked: MCP `{file_name}` from `{}` is not in the lock",
                        src.source
                    )));
                }
            }
            Ok(())
        }
        McpsField::Wildcard(_) => {
            let present = lock
                .assets
                .values()
                .any(|a| a.kind == "mcp" && a.source == src.source);
            if present {
                Ok(())
            } else {
                Err(err(format!(
                    "--locked: source `{}` has no MCP entries in the lock",
                    src.source
                )))
            }
        }
    }
}

fn apply_pending_mcps(
    ctx: &DriverCtx,
    lock: &mut AgentLockFile,
    res: &mut SyncResult,
    mcp_settings_list: &[McpSettingsTarget],
    pending: &[PendingMcp],
) {
    for p in pending {
        let status = if !p.is_new {
            if ctx.dry_run {
                "would_update"
            } else {
                "updated"
            }
        } else if ctx.dry_run {
            "would_install"
        } else {
            "installed"
        };

        if !ctx.dry_run {
            let mut failed = false;
            for target in mcp_settings_list {
                if let Err(e) = merge_mcp_config(&p.mcp_path, target) {
                    res.summary.failed += 1;
                    res.actions.push(Action {
                        source: Some(p.source.clone()),
                        skill: Some(mcp_action_label(&p.file_name)),
                        status: "source_error".into(),
                        error: Some(e.to_string()),
                    });
                    failed = true;
                    break;
                }
            }
            if failed {
                continue;
            }
            let servers_csv = p.server_names.join(",");
            lock.save_tracked_asset(
                &p.asset_id,
                AssetEntry {
                    kind: "mcp".into(),
                    name: p.file_name.clone(),
                    hash: p.hash.clone(),
                    source: p.source.clone(),
                    destination: servers_csv,
                    source_revision: p.source_revision.clone(),
                },
            );
        }

        if status.contains("install") {
            res.summary.installed += 1;
        } else {
            res.summary.updated += 1;
        }
        res.actions.push(Action {
            source: Some(p.source.clone()),
            skill: Some(mcp_action_label(&p.file_name)),
            status: status.into(),
            error: None,
        });
    }
}

fn remove_stale_mcps(
    ctx: &DriverCtx,
    lock: &mut AgentLockFile,
    res: &mut SyncResult,
    desired_mcp_ids: &HashSet<String>,
    mcp_settings_list: &[McpSettingsTarget],
) {
    let existing_mcps: Vec<(String, String)> = lock
        .list_tracked_asset_ids("mcp")
        .into_iter()
        .map(|(id, dest)| (id.to_owned(), dest.to_owned()))
        .collect();
    let servers_by_id: HashMap<String, String> = existing_mcps.iter().cloned().collect();
    let candidates: Vec<StaleEntry> = existing_mcps
        .iter()
        .map(|(id, _)| {
            let mcp_name = id.rsplit("::").next().unwrap_or(id);
            StaleEntry {
                id: id.clone(),
                action_source: None,
                action_skill: mcp_action_label(mcp_name),
            }
        })
        .collect();

    remove_stale(
        ctx.dry_run,
        &mut res.summary,
        &mut res.actions,
        desired_mcp_ids,
        candidates,
        |id| {
            if let Some(servers_csv) = servers_by_id.get(id) {
                for target in mcp_settings_list {
                    for server_name in servers_csv.split(',').filter(|s| !s.is_empty()) {
                        let _ = remove_mcp_server(server_name, target);
                    }
                }
            }
            lock.remove_tracked_asset(id);
        },
    );
}

// ===================================================================================
// lock --check / lock rebuild (kasetto commands/lock.rs)
// ===================================================================================

/// Rebuild the skills section of `lock` from a fresh resolve and refresh asset revisions.
/// `upgrade_only` (empty = all) restricts which sources are re-resolved (others carry over).
/// Returns the rebuilt lock; any source error aborts (Err) before mutation is observable.
pub fn rebuild_lock(
    cfg: &Config,
    cfg_dir: &Path,
    scope: Scope,
    prev: &AgentLockFile,
    upgrade_only: &[String],
) -> Result<AgentLockFile> {
    let destinations = resolve_destinations(cfg_dir, cfg, scope)?;
    let root = scope_root(scope, cfg_dir)?;
    let prev_skills = &prev.skills;

    let upgrade_active = |source_url: &str| -> bool {
        if upgrade_only.is_empty() {
            return true;
        }
        prev_skills
            .values()
            .any(|e| e.source == source_url && upgrade_only.contains(&e.skill))
    };

    let mut new_skills: BTreeMap<String, AgentLockEntry> = BTreeMap::new();
    for (i, src) in cfg.skills.iter().enumerate() {
        if !upgrade_active(&src.source) {
            for (id, entry) in prev_skills.iter().filter(|(_, e)| e.source == src.source) {
                new_skills.insert(id.clone(), entry.clone());
            }
            continue;
        }
        let stage = std::env::temp_dir().join(format!("envctl-agent-lock-{}-{}", now_unix(), i));
        let materialized = materialize_source(src, cfg_dir, &stage)?;
        let select = select_targets(
            &src.skills,
            &materialized.available,
            &materialized.source_root,
        );

        let result = select.and_then(|(targets, broken)| {
            if let Some(b) = broken.first() {
                return Err(err(format!(
                    "skill `{}` not found in {}",
                    b.name, src.source
                )));
            }
            for (name, dir) in targets {
                let hash = hash_dir(&dir)?;
                let dest = destinations[0].join(&name);
                let (_, description) = read_skill_profile_from_dir(&dir, &name);
                new_skills.insert(
                    skill_key(&src.source, &name),
                    AgentLockEntry {
                        destination: relativize_dest(&dest, &root),
                        hash,
                        skill: name.clone(),
                        description,
                        source: src.source.clone(),
                        source_revision: materialized.source_revision.clone(),
                        scope: Some(scope),
                    },
                );
            }
            Ok(())
        });

        if let Some(cleanup) = materialized.cleanup_dir {
            let _ = fs::remove_dir_all(cleanup);
        }
        result?;
    }

    let mut next = prev.clone();
    next.skills = new_skills;
    refresh_asset_revisions(&mut next, cfg);
    Ok(next)
}

fn refresh_asset_revisions(lock: &mut AgentLockFile, cfg: &Config) {
    let mut rev_by_source: HashMap<String, String> = HashMap::new();
    for m in &cfg.mcps {
        rev_by_source.insert(m.source.clone(), m.as_source_spec().expected_revision());
    }
    for c in &cfg.commands {
        rev_by_source.insert(c.source.clone(), c.as_source_spec().expected_revision());
    }
    for asset in lock.assets.values_mut() {
        if let Some(rev) = rev_by_source.get(&asset.source) {
            asset.source_revision = rev.clone();
        }
    }
}

// ===================================================================================
// list (kasetto commands/list.rs)
// ===================================================================================

/// A `list`-view row for an installed non-skill asset (MCP server or command).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AssetRow {
    pub name: String,
    pub scope: Scope,
    pub pack_file: String,
    pub source: String,
}

/// The merged `list` view: installed skills + MCP servers + commands. When `scope_override`
/// is `None` the global and project locks are merged (kasetto's default `list` behavior);
/// otherwise only the named scope's lock is read. Read-only.
#[allow(clippy::type_complexity)]
pub fn load_skills_mcps_commands(
    scope_override: Option<Scope>,
    project_root: &Path,
    load_lock: &dyn Fn(Scope, &Path) -> Result<AgentLockFile>,
    load_updated: &dyn Fn(Scope, &Path) -> BTreeMap<String, String>,
) -> Result<(Vec<InstalledSkill>, Vec<AssetRow>, Vec<AssetRow>)> {
    if let Some(s) = scope_override {
        let lock = load_lock(s, project_root)?;
        let updated = load_updated(s, project_root);
        return Ok((
            installed_skills_from_lock(&lock, &updated, s, project_root, false),
            mcp_asset_entries(&lock, s),
            command_asset_entries(&lock, s),
        ));
    }
    let global_lock = load_lock(Scope::Global, project_root)?;
    let project_lock = load_lock(Scope::Project, project_root)?;
    let global_updated = load_updated(Scope::Global, project_root);
    let project_updated = load_updated(Scope::Project, project_root);
    let mut skills = installed_skills_from_lock(
        &global_lock,
        &global_updated,
        Scope::Global,
        project_root,
        true,
    );
    skills.extend(installed_skills_from_lock(
        &project_lock,
        &project_updated,
        Scope::Project,
        project_root,
        true,
    ));
    skills.sort_by_cached_key(|s| (scope_ord(s.scope), s.name.to_lowercase()));
    let mut mcps = mcp_asset_entries(&global_lock, Scope::Global);
    mcps.extend(mcp_asset_entries(&project_lock, Scope::Project));
    mcps.sort_by_cached_key(|m| (m.name.to_lowercase(), scope_ord(m.scope)));
    let mut commands = command_asset_entries(&global_lock, Scope::Global);
    commands.extend(command_asset_entries(&project_lock, Scope::Project));
    commands.sort_by_cached_key(|m| (m.name.to_lowercase(), scope_ord(m.scope)));
    Ok((skills, mcps, commands))
}

fn command_asset_entries(lock: &AgentLockFile, scope: Scope) -> Vec<AssetRow> {
    let mut out: Vec<AssetRow> = lock
        .assets
        .values()
        .filter(|a| a.kind == "command")
        .map(|a| AssetRow {
            name: a.name.clone(),
            scope,
            pack_file: String::new(),
            source: a.source.clone(),
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

fn mcp_asset_entries(lock: &AgentLockFile, scope: Scope) -> Vec<AssetRow> {
    // Distinct installed server names across all mcp assets (the kasetto
    // `LockFile::list_installed_mcps` view, derived here from the destination CSVs).
    let mut names: Vec<String> = Vec::new();
    for a in lock.assets.values().filter(|a| a.kind == "mcp") {
        for s in a.destination.split(',').filter(|s| !s.is_empty()) {
            if !names.contains(&s.to_string()) {
                names.push(s.to_string());
            }
        }
    }
    let mut out = Vec::new();
    for name in names {
        let (pack_file, source) = lock
            .assets
            .values()
            .filter(|a| a.kind == "mcp")
            .find(|a| a.destination.split(',').any(|s| !s.is_empty() && s == name))
            .map(|a| (a.name.clone(), a.source.clone()))
            .unwrap_or_default();
        out.push(AssetRow {
            name,
            scope,
            pack_file,
            source,
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

fn scope_ord(s: Scope) -> u8 {
    match s {
        Scope::Global => 0,
        Scope::Project => 1,
    }
}

fn scope_label(s: Scope) -> &'static str {
    match s {
        Scope::Global => "global",
        Scope::Project => "project",
    }
}

fn skill_display_id(lock_scope: Scope, raw_id: &str, composite: bool) -> String {
    if composite {
        format!("{}::{}", scope_label(lock_scope), raw_id)
    } else {
        raw_id.to_string()
    }
}

fn installed_skills_from_lock(
    lock: &AgentLockFile,
    updated: &BTreeMap<String, String>,
    lock_scope: Scope,
    project_root: &Path,
    composite_ids: bool,
) -> Vec<InstalledSkill> {
    let root = scope_root(lock_scope, project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let mut skills = Vec::new();
    for (id, entry) in &lock.skills {
        let abs_dest = resolve_dest(&entry.destination, &root);
        let abs_dest_str = abs_dest.to_string_lossy().to_string();
        let (name, fallback_description) = read_skill_profile(&abs_dest_str, &entry.skill);
        let description = if entry.description.trim().is_empty() {
            fallback_description
        } else {
            entry.description.clone()
        };
        let updated_at = updated.get(id).cloned().unwrap_or_default();
        let updated_ago = format_updated_ago(&updated_at);
        let effective_scope = entry.scope.unwrap_or(lock_scope);
        skills.push(InstalledSkill {
            id: skill_display_id(lock_scope, id, composite_ids),
            scope: effective_scope,
            name,
            description,
            source: entry.source.clone(),
            skill: entry.skill.clone(),
            destination: abs_dest_str,
            hash: entry.hash.clone(),
            source_revision: entry.source_revision.clone(),
            updated_at,
            updated_ago,
        });
    }
    skills.sort_by_cached_key(|s| s.name.to_lowercase());
    skills
}

// ===================================================================================
// clean (kasetto commands/clean.rs)
// ===================================================================================

/// Counts of what `clean` removed (or would remove).
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct CleanCounts {
    pub skills_removed: usize,
    pub mcps_removed: usize,
    pub commands_removed: usize,
}

/// Tear down every lock-tracked asset on disk: skill dirs, command files, and the
/// MCP servers this lock installed (via `remove_mcp_server` — **only** tracked servers,
/// so pre-existing global MCP servers are never touched). Caller clears + saves the lock.
pub fn apply_removals(lock: &AgentLockFile, scope: Scope, project_root: &Path) -> Result<()> {
    let root = scope_root(scope, project_root)?;
    for entry in lock.skills.values() {
        let _ = fs::remove_dir_all(resolve_dest(&entry.destination, &root));
    }

    for (_id, dest_csv) in lock.list_tracked_asset_ids("command") {
        for p in dest_csv.split(',').filter(|s| !s.is_empty()) {
            let path = resolve_dest(p, &root);
            if path.exists() && path.is_file() {
                let _ = fs::remove_file(path);
            }
        }
    }

    let mcp_targets = match scope {
        Scope::Project => all_mcp_project_targets(project_root),
        Scope::Global => {
            let home = dirs_home()?;
            let cfg_dir = dirs_agent_env_config()?;
            all_mcp_settings_targets(&home, &cfg_dir)
        }
    };
    for (_id, servers_csv) in lock.list_tracked_asset_ids("mcp") {
        for server_name in servers_csv.split(',').filter(|s| !s.is_empty()) {
            for target in &mcp_targets {
                if target.path.exists() {
                    let _ = remove_mcp_server(server_name, target);
                }
            }
        }
    }
    Ok(())
}

/// Count the lock-tracked assets (the `clean` preview numbers).
pub fn clean_counts(lock: &AgentLockFile) -> CleanCounts {
    CleanCounts {
        skills_removed: lock.skills.len(),
        mcps_removed: lock.list_tracked_asset_ids("mcp").len(),
        commands_removed: lock.list_tracked_asset_ids("command").len(),
    }
}

// ===================================================================================
// add / remove edit planning (kasetto commands/add.rs + remove.rs)
// ===================================================================================

/// One resolved section edit: which list, and the entry to insert there.
pub struct SectionEdit {
    pub section: Section,
    pub item: SourceItem,
}

/// Decompose the positional source + flags into the per-section edits `add` will apply.
/// Ported from kasetto `add.rs::{resolve_pin, plan_edits, selector_from}` + browse-URL derivation.
#[allow(clippy::too_many_arguments)]
pub fn plan_add_edits(
    raw_source: &str,
    at_ref: Option<&str>,
    skills: &[String],
    mcps: &[String],
    commands: &[String],
    git_ref: Option<&str>,
    branch: Option<&str>,
    sub_dir: Option<&str>,
) -> (String, Pin, Option<String>, Vec<SectionEdit>) {
    let derived = derive_browse_url(raw_source).unwrap_or_else(|| BrowseDerived {
        source: raw_source.to_string(),
        ..Default::default()
    });
    let source = derived.source.clone();
    let pin = resolve_pin(git_ref, branch, at_ref, &derived);
    let resolved_sub_dir = sub_dir
        .map(str::to_string)
        .or_else(|| derived.sub_dir.clone());

    let skill_names: Vec<String> = if !skills.is_empty() {
        skills.to_vec()
    } else if let Some(name) = &derived.skill_name {
        vec![name.clone()]
    } else {
        Vec::new()
    };
    let nothing_specified = skill_names.is_empty() && mcps.is_empty() && commands.is_empty();

    let mut edits = Vec::new();
    let mut push = |section: Section, selector: Selector| {
        let item_sub = if section == Section::Mcps {
            None
        } else {
            resolved_sub_dir.clone()
        };
        edits.push(SectionEdit {
            section,
            item: SourceItem {
                source: source.clone(),
                pin: pin.clone(),
                sub_dir: item_sub,
                selector,
            },
        });
    };

    if !skill_names.is_empty() {
        push(Section::Skills, selector_from(&skill_names));
    } else if nothing_specified {
        push(Section::Skills, Selector::Wildcard);
    }
    if !mcps.is_empty() {
        push(Section::Mcps, selector_from(mcps));
    }
    if !commands.is_empty() {
        push(Section::Commands, selector_from(commands));
    }
    (source, pin, resolved_sub_dir, edits)
}

fn resolve_pin(
    git_ref: Option<&str>,
    branch: Option<&str>,
    at_ref: Option<&str>,
    derived: &BrowseDerived,
) -> Pin {
    if let Some(r) = git_ref {
        return Pin::Ref(r.to_string());
    }
    if let Some(b) = branch {
        return Pin::Branch(b.to_string());
    }
    if let Some(r) = at_ref {
        return Pin::Ref(r.to_string());
    }
    if let Some(r) = &derived.git_ref {
        return Pin::Ref(r.clone());
    }
    if let Some(b) = &derived.branch {
        return Pin::Branch(b.clone());
    }
    Pin::None
}

fn selector_from(names: &[String]) -> Selector {
    if names.len() == 1 && names[0] == "*" {
        Selector::Wildcard
    } else {
        Selector::Names(names.to_vec())
    }
}

/// Fetch the source once to confirm it resolves before touching the config; for named
/// skill entries also assert each skill exists. Ported from kasetto `add.rs::verify_source`.
pub fn verify_source(
    source: &str,
    pin: &Pin,
    sub_dir: Option<&str>,
    edits: &[SectionEdit],
    config_path: &Path,
) -> Result<()> {
    let spec = SourceSpec {
        source: source.to_string(),
        branch: match pin {
            Pin::Branch(b) => Some(b.clone()),
            _ => None,
        },
        git_ref: match pin {
            Pin::Ref(r) => Some(r.clone()),
            _ => None,
        },
        sub_dir: sub_dir.map(str::to_string),
        skills: SkillsField::Wildcard("*".to_string()),
    };
    let cfg_dir = config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let stage = std::env::temp_dir().join(format!("envctl-agent-add-{}", now_unix()));

    let materialized = materialize_source(&spec, &cfg_dir, &stage)?;

    let mut name_error = None;
    if let Some(names) = named_skills(edits) {
        let sf = SkillsField::List(names.iter().cloned().map(SkillTarget::Name).collect());
        match select_targets(&sf, &materialized.available, &materialized.source_root) {
            Ok((_, broken)) => {
                if let Some(b) = broken.first() {
                    name_error = Some(err(format!("skill `{}` not found in {source}", b.name)));
                }
            }
            Err(e) => name_error = Some(e),
        }
    }

    if let Some(dir) = materialized.cleanup_dir {
        let _ = fs::remove_dir_all(dir);
    }
    match name_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn named_skills(edits: &[SectionEdit]) -> Option<&Vec<String>> {
    edits
        .iter()
        .find_map(|e| match (&e.section, &e.item.selector) {
            (Section::Skills, Selector::Names(names)) => Some(names),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_ctx_from_mode_maps_lock_modes() {
        let cfg = Config {
            destination: None,
            scope: Some(Scope::Project),
            agent: None,
            skills: Vec::new(),
            mcps: Vec::new(),
            commands: Vec::new(),
        };
        let dests: Vec<PathBuf> = Vec::new();
        let root = PathBuf::from("/tmp");

        let plain = DriverCtx::from_mode(
            &cfg,
            &root,
            &dests,
            root.clone(),
            Scope::Project,
            false,
            &LockMode::Plain,
        );
        assert!(plain.dry_run, "apply=false => dry_run");
        assert!(!plain.locked);
        assert!(!plain.update);

        let locked = DriverCtx::from_mode(
            &cfg,
            &root,
            &dests,
            root.clone(),
            Scope::Project,
            true,
            &LockMode::Locked,
        );
        assert!(!locked.dry_run, "apply=true => not dry_run");
        assert!(locked.locked);

        let upd = DriverCtx::from_mode(
            &cfg,
            &root,
            &dests,
            root.clone(),
            Scope::Project,
            true,
            &LockMode::Update(vec!["a".into()]),
        );
        assert!(upd.update);
        assert_eq!(upd.update_only, vec!["a".to_string()]);
    }

    #[test]
    fn plan_add_edits_defaults_to_skills_wildcard() {
        let (source, _pin, _sub, edits) = plan_add_edits(
            "https://example.com/pack",
            None,
            &[],
            &[],
            &[],
            None,
            None,
            None,
        );
        assert_eq!(source, "https://example.com/pack");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].section, Section::Skills);
        assert!(matches!(edits[0].item.selector, Selector::Wildcard));
    }

    #[test]
    fn plan_add_edits_named_mcps_and_commands() {
        let (_s, _p, _d, edits) = plan_add_edits(
            "https://example.com/pack",
            None,
            &[],
            &["github".into()],
            &["review".into()],
            None,
            None,
            None,
        );
        // No skills specified but mcps+commands ARE → no skills wildcard.
        assert_eq!(edits.len(), 2);
        assert!(edits.iter().any(|e| e.section == Section::Mcps));
        assert!(edits.iter().any(|e| e.section == Section::Commands));
        // MCP edits never carry sub-dir.
        let mcp = edits.iter().find(|e| e.section == Section::Mcps).unwrap();
        assert!(mcp.item.sub_dir.is_none());
    }
}
