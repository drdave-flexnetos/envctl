# envctl — Hardening notes applied in Phase 0

This scaffold was produced by a design swarm and then adversarially reviewed. The following review findings were folded into the code as written:

## Rust compile-pitfall fixes (applied)
- `EventSink::new()` → `EventSink::channel()` (clippy `new_ret_no_self`).
- `#[allow(clippy::too_many_arguments)]` on `run_phase`/`run_plan`.
- `runner.rs` `Hook::Script{path}` branch fixed to run the file directly (`bash -lc <path>`), not the broken `"$0"` indirection.
- `OpStatus` gained `SkippedBlocked` + `RebootRequired`; `RunSummary` gained `skipped_blocked`.
- `auto-detect` now serializes/prints a real `EnvReport` (was a silent no-op); `EnvReport.generated_at` set via `chrono` (so `chrono`/`which`/`serde_json` are all used).

## Safety fixes (applied — boot-repair discipline)
- **Guards fail-closed.** `UuidResolves`/`NotLiveDevice`/`NotMounted` are implemented for real via `blkid`/`findmnt` (ported from `ubuntu-boot-repair.sh`): any unresolved/ambiguous/mounted/live condition → `Refused`, never silent-pass. A unit test asserts a bogus-UUID guard refuses.
- `RunContext` resolves the live-root UUID **once** per run (resolve-once, no TOCTOU); guards read it.
- Destructive verbs (`reset`/`auto-fix`) default to **dry-run** unless `--apply`.
- `install` is **idempotent**: skip-if-detected; dependants of a failed component are `SkippedBlocked`.
- `add-repo` hardened: strict slug validation, refuse id-collision with a built-in, atomic temp+rename drop-in write with timestamped backup, clone into `~/.local/share/envctl/repos/<slug>` (0700) not `/tmp`, all interpolated values escaped.
- `base.toml` `yazelix-shell`: removed the hand-rolled `sed -i ~/.bashrc` remove hook (literal-`~` bug + double-ownership); `Wiring::revert` is the single owner of marker-block excision (backup-then-excise-only-owned-block).

## Deferred to later phases (tracked in ROADMAP.md)
- Live line-streaming of hook stdout/stderr + on-disk run log (Phase 2).
- System-scope `Wiring` revert (/etc/nix, /etc/apt, /etc/cdi, update-alternatives) (Phase 3).
- Post-action re-verify (reset→absent / auto-fix→healthy) + auto-fix revert-on-failure (Phase 3).
- Per-hook timeout + `catch_unwind` panic isolation + sudo keepalive (Phase 2/3).
- Full 9-stage add-repo build-system pipeline + artifact wiring (Phase 4).

## Reviewer verdict
> Approve with required fixes. The scaffold COMPILES on stable as written — the Send/Sync/'static engine<->egui-worker boundary is correct (the HookRunner: Send + Sync supertrait is the load-bearing piece and is present), eframe 0.30 App::update / run_native signatures match, the clap derive is valid, the thiserror/anyhow/toml error-type plumbing lines up, and the sysinfo 0.33 calls (associated kernel_version/load_average, byte-based memory) are correct. No hard blockers. The two things that will actually bite: (1) the `cargo clippy -- -D warnings` post-scaffold check will FAIL (new_ret_no_self on EventSink::new, too_many_arguments, unused vars) — relax that gate for the skeleton or apply the renames/allows; and (2) auto-detect is a silent no-op that emits zero events and discards the EnvReport, so the two post-scaffold checks relying on it demonstrate nothing — wire detect to print/serialize its report. Before writing, pin eframe+egui+egui_extras to the SAME 0.30.x and confirm via `cargo tree -p egui` that only one egui node resolves (the classic break for this stack); if 0.30.0 is not published, drop all three to the latest common 0.29.x. Harden the one brittle seam — internally-tagged Hook/Guard enums under toml — with a round-trip unit test, and never add #[serde(flatten)]/untagged there. Reconcile the ARCHITECTURE.md manifest examples with the [[component]] shape the loader actually requires.