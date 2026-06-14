//! `Engine::agent_add` (C-07) + `Engine::agent_remove` (C-08) + the C-12 `config_edit` →
//! `sync_after` self-sync. Both edit the agent-env config YAML then (unless `no_sync`)
//! run an in-process `agent_sync(apply=true)` to install/prune. Preview (`apply: false`)
//! plans the edit and returns WITHOUT touching the config or the filesystem.

use std::fs;
use std::path::PathBuf;

use envctl_agent_env::config_edit::{
    insert_item, remove_item, remove_names, split_at_ref, RemoveOutcome, Section,
};
use envctl_agent_env::config_path::default_config_path;
use envctl_agent_env::driver::{plan_add_edits, verify_source, SectionEdit};
use envctl_agent_env::source::{derive_browse_url, BrowseDerived};

use crate::agent::report::{AgentEditItem, AgentEditOutcome, AgentVerb};
use crate::agent::sync::run_sync_in_ctx;
use crate::agent::{AgentAddSpec, AgentCtx, AgentLockMode, AgentRemoveSpec, AgentScope};
use crate::event::{Event, EventSink};
use crate::Engine;

impl Engine {
    /// Add a source (and/or named skills/mcps/commands) to the agent-env config, then sync it.
    ///
    /// Preview (`apply: false`) plans the section edits, runs the `item_exists` guard, and
    /// returns `would_add` with ZERO writes. With `apply: true` the config is (optionally)
    /// verified against the live source, written, and — unless `no_sync` — synced in-process.
    /// `Locked` requires `no_sync` (a brand-new source has no lock entry yet — ported rule).
    pub fn agent_add(
        &self,
        spec: AgentAddSpec,
        sink: &EventSink,
    ) -> anyhow::Result<AgentEditOutcome> {
        if spec.git_ref.is_some() && spec.branch.is_some() {
            anyhow::bail!("--ref and --branch are mutually exclusive");
        }
        if spec.lock_mode == AgentLockMode::Locked && !spec.no_sync {
            anyhow::bail!(
                "`--locked` on `add` requires `--no-sync` — a newly added source cannot be \
                 installed without fetching. Either pass `--no-sync --locked` (edit the manifest \
                 only, then `lock` + `sync --locked` to install offline), or drop `--locked` to \
                 fetch the new source now."
            );
        }

        sink.emit(Event::AgentRunStarted {
            verb: AgentVerb::Add,
            scope: scope_label(spec.scope_override),
            dry_run: !spec.apply,
            lock_mode: spec.lock_mode.label(),
        });

        let path = resolve_local_config_path(spec.config_path.as_deref())?;

        let (raw_source, at_ref) = split_at_ref(&spec.source);
        if at_ref.is_some() && (spec.git_ref.is_some() || spec.branch.is_some()) {
            anyhow::bail!("`@<ref>` shorthand conflicts with --ref/--branch; pass only one");
        }

        let (source, pin, sub_dir, edits) = plan_add_edits(
            &raw_source,
            at_ref.as_deref(),
            &spec.section.skills,
            &spec.section.mcps,
            &spec.section.commands,
            spec.git_ref.as_deref(),
            spec.branch.as_deref(),
            spec.sub_dir.as_deref(),
        );

        let mut text = if path.exists() {
            fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?
        } else {
            "# envctl agent-env config\n".to_string()
        };

        for edit in &edits {
            if envctl_agent_env::config_edit::item_exists(&text, edit.section, &edit.item) {
                anyhow::bail!(
                    "`{source}` is already in `{}:`; edit it directly or remove it first",
                    edit.section.key()
                );
            }
        }

        let items = section_items(&source, &edits);

        if !spec.apply {
            let outcome = AgentEditOutcome {
                action: "would_add".into(),
                source: source.clone(),
                items,
                dry_run: true,
                sync: None,
            };
            emit_edit_finished(sink);
            return Ok(outcome);
        }

        if !spec.no_verify {
            verify_source(&source, &pin, sub_dir.as_deref(), &edits, &path)?;
        }

        for edit in &edits {
            text = insert_item(&text, edit.section, &edit.item)?;
        }
        fs::write(&path, &text)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;

        let sync = if !spec.no_sync {
            Some(sync_after(
                &path,
                spec.scope_override,
                spec.apply,
                &spec.lock_mode,
                sink,
            )?)
        } else {
            None
        };

        emit_edit_finished(sink);
        Ok(AgentEditOutcome {
            action: "added".into(),
            source,
            items,
            dry_run: false,
            sync,
        })
    }

    /// Remove a source (or named entries) from the agent-env config, then prune via sync.
    ///
    /// Preview (`apply: false`) plans the removal on an in-memory copy and returns
    /// `would_remove` with ZERO writes. With `apply: true` the config is written and — unless
    /// `no_sync` — synced in-process (the sync prunes the now-orphaned assets via
    /// `remove_stale`).
    pub fn agent_remove(
        &self,
        spec: AgentRemoveSpec,
        sink: &EventSink,
    ) -> anyhow::Result<AgentEditOutcome> {
        sink.emit(Event::AgentRunStarted {
            verb: AgentVerb::Remove,
            scope: scope_label(spec.scope_override),
            dry_run: !spec.apply,
            lock_mode: spec.lock_mode.label(),
        });

        let path = resolve_local_config_path(spec.config_path.as_deref())?;
        if !path.exists() {
            anyhow::bail!("config not found: {}", path.display());
        }
        let mut text = fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;

        let (raw_source, at_ref) = split_at_ref(&spec.source);
        if at_ref.is_some() && (spec.git_ref.is_some() || spec.branch.is_some()) {
            anyhow::bail!("`@<ref>` shorthand conflicts with --ref/--branch; pass only one");
        }

        let derived = derive_browse_url(&raw_source).unwrap_or_else(|| BrowseDerived {
            source: raw_source.clone(),
            ..Default::default()
        });
        let source = derived.source.clone();
        let derived_pin = derived.git_ref.as_deref().or(derived.branch.as_deref());
        let pin = spec
            .git_ref
            .as_deref()
            .or(spec.branch.as_deref())
            .or(at_ref.as_deref())
            .or(derived_pin);
        let sub_dir = spec.sub_dir.as_deref().or(derived.sub_dir.as_deref());

        let kinds = [
            (Section::Skills, spec.section.skills.as_slice()),
            (Section::Mcps, spec.section.mcps.as_slice()),
            (Section::Commands, spec.section.commands.as_slice()),
        ];
        let any_kind = kinds.iter().any(|(_, names)| !names.is_empty());

        let removed = if any_kind {
            remove_by_kind(&mut text, &source, pin, sub_dir, &kinds)?
        } else {
            remove_whole_source(&mut text, &source, pin, sub_dir)?
        };
        let items: Vec<AgentEditItem> = removed
            .iter()
            .map(|r| AgentEditItem {
                target: r.target.clone(),
                section: r.section.to_string(),
            })
            .collect();

        if !spec.apply {
            emit_edit_finished(sink);
            return Ok(AgentEditOutcome {
                action: "would_remove".into(),
                source,
                items,
                dry_run: true,
                sync: None,
            });
        }

        fs::write(&path, &text)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;

        let sync = if !spec.no_sync {
            Some(sync_after(
                &path,
                spec.scope_override,
                spec.apply,
                &spec.lock_mode,
                sink,
            )?)
        } else {
            None
        };

        emit_edit_finished(sink);
        Ok(AgentEditOutcome {
            action: "removed".into(),
            source,
            items,
            dry_run: false,
            sync,
        })
    }
}

/// One applied removal, for the outcome line.
struct Removed {
    target: String,
    section: &'static str,
}

fn remove_whole_source(
    text: &mut String,
    source: &str,
    pin: Option<&str>,
    sub_dir: Option<&str>,
) -> anyhow::Result<Vec<Removed>> {
    let mut removed = Vec::new();
    for section in [Section::Skills, Section::Mcps, Section::Commands] {
        let section_sub = if section == Section::Mcps {
            None
        } else {
            sub_dir
        };
        let (updated, did) = remove_item(text, section, source, pin, section_sub)?;
        if did {
            *text = updated;
            removed.push(Removed {
                target: source.to_string(),
                section: section.key(),
            });
        }
    }
    if removed.is_empty() {
        anyhow::bail!(
            "`{source}` not found in any list (entries inherited via `extends` must be removed in the parent)"
        );
    }
    Ok(removed)
}

fn remove_by_kind(
    text: &mut String,
    source: &str,
    pin: Option<&str>,
    sub_dir: Option<&str>,
    kinds: &[(Section, &[String])],
) -> anyhow::Result<Vec<Removed>> {
    let mut removed = Vec::new();
    for (section, names) in kinds {
        if names.is_empty() {
            continue;
        }
        let section_sub = if *section == Section::Mcps {
            None
        } else {
            sub_dir
        };
        if names.len() == 1 && names[0] == "*" {
            let (updated, did) = remove_item(text, *section, source, pin, section_sub)?;
            if !did {
                anyhow::bail!("`{source}` not found in `{}:`", section.key());
            }
            *text = updated;
            removed.push(Removed {
                target: source.to_string(),
                section: section.key(),
            });
            continue;
        }
        let (updated, outcome) = remove_names(text, *section, source, pin, section_sub, names)?;
        match outcome {
            RemoveOutcome::NotFound => {
                anyhow::bail!("`{source}` not found in `{}:`", section.key());
            }
            RemoveOutcome::WholeItem => {
                *text = updated;
                removed.push(Removed {
                    target: source.to_string(),
                    section: section.key(),
                });
            }
            RemoveOutcome::Names(ns) => {
                *text = updated;
                removed.push(Removed {
                    target: ns.join(", "),
                    section: section.key(),
                });
            }
        }
    }
    Ok(removed)
}

fn section_items(source: &str, edits: &[SectionEdit]) -> Vec<AgentEditItem> {
    let mut sections: Vec<&str> = edits.iter().map(|e| e.section.key()).collect();
    sections.dedup();
    sections
        .into_iter()
        .map(|s| AgentEditItem {
            target: source.to_string(),
            section: s.to_string(),
        })
        .collect()
}

/// The C-12 follow-up sync: resolve the just-edited config and run `agent_sync(apply=true)`
/// in-process (NOT a subprocess), so add/remove install/prune through the identical engine path.
fn sync_after(
    config_path: &std::path::Path,
    scope_override: Option<AgentScope>,
    apply: bool,
    lock_mode: &AgentLockMode,
    sink: &EventSink,
) -> anyhow::Result<crate::agent::report::AgentReport> {
    let cfg_str = config_path.to_string_lossy().to_string();
    let ctx = AgentCtx::resolve(Some(&cfg_str), scope_override)?;
    run_sync_in_ctx(&ctx, apply, lock_mode, sink)
}

/// Resolve the LOCAL (writable) config path for an edit. The agent-env config edits only ever
/// touch a local file (`ensure_local_config` semantics): an explicit path is honored after
/// rejecting remote (`scheme://…`) configs — which cannot be rewritten in place — otherwise the
/// default config filename in the cwd. Fail-closed: a remote `--config` is refused, never
/// silently treated as a local path (parity with kasetto `resolve_local_config_path`; C-12).
fn resolve_local_config_path(config: Option<&str>) -> anyhow::Result<PathBuf> {
    match config {
        Some(c) => {
            envctl_agent_env::config_edit::ensure_local_config(c)?;
            Ok(PathBuf::from(c))
        }
        None => Ok(PathBuf::from(default_config_path())),
    }
}

fn scope_label(scope: Option<AgentScope>) -> AgentScope {
    scope.unwrap_or(AgentScope::Project)
}

fn emit_edit_finished(_sink: &EventSink) {
    // The per-edit confirmation is carried by the returned AgentEditOutcome (and the embedded
    // sync's own AgentRunFinished). No extra event needed; kept as a seam for symmetry.
}
