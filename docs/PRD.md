# envctl — Product Requirements Document

**Status:** Living document · Phases 0–2 complete & dogfooded on the live box · Phase 3 in progress
**Owner:** Single power-user (owner of the dual-RTX-5090 workstation)
**Last updated:** 2026-06-02
**Related docs:** [`README.md`](../README.md) · [`docs/ARCHITECTURE.md`](ARCHITECTURE.md) · [`docs/ROADMAP.md`](ROADMAP.md) · [`docs/DESIGN-NOTES.md`](DESIGN-NOTES.md) · [`assets/scripts/HANDOFF.md`](../assets/scripts/HANDOFF.md)

---

## TL;DR

`envctl` is a **pure-Rust, GPU-aware, source-building environment manager** for one specific machine: an Ubuntu 26.04 LTS developer workstation with **two NVIDIA RTX 5090** (GB202 / Blackwell / sm_120) GPUs. It takes the proven first-login wizard (`yazelix-setup.sh`) and the hardened boot-repair script (`ubuntu-boot-repair.sh`) and makes the whole toolchain **idempotent, resumable, observable, and reversible** — without rewriting any of the proven bash. Every tool is a declarative TOML **component** whose lifecycle hooks *wrap* the original bash verbatim. One Cargo workspace ships a shared **engine** library, an **`envctl`** CLI, and a native egui **`envctl-gui`** desktop app, all driven by the same five verbs: `auto-detect`, `install`, `auto-fix`, `reset`, `add-repo`. Destructive operations inherit the boot-repair "gold standard": **dry-run by default, resolve-then-re-verify, refuse on ambiguity, back up before clobber, never touch user data.**

---

## 1. Overview / Vision

### 1.1 The problem

Provisioning a high-end GPU developer workstation from source is a long, fragile, multi-stage process. The existing first-login wizard (`yazelix-setup.sh`) already installs the entire toolchain — Bun (+ `node`→bun), the AI CLIs (Claude/Codex/Gemini/Kimi/Devin), rustup/rtk, CUDA 13.3 + nvidia-open 610 + LLVM/clang-21, cuda-oxide, the NVIDIA Container Toolkit, PyTorch cu132, gh/Vite/wasmer/uv, Nix + the yazelix Cachix cache, home-manager, yazelix + `yzx desktop`, the guarded `~/.bashrc` blocks, and the GPU smoke-test autostart — but it is a **one-shot, fire-and-forget script**. It has no memory of what it already did, no way to report what state the box is actually in, no clean way to repair a single broken tool, and no way to cleanly undo what it changed.

That leaves four capability gaps that bite a working developer:

1. **No observability.** There is no read-only way to ask "what is actually installed, wired, and healthy on this box right now?" — especially on the **software-rendered first boot** before the GPU driver loads, where naïve tools report a false failure.
2. **No idempotency / resumability.** Re-running the wizard re-runs every `curl | bash` installer. A single failure mid-run leaves the box in an unknown partial state.
3. **No repair.** When one tool drifts or breaks (a missing PATH block, a half-installed driver, a stale cuda-oxide), there is no targeted, safe heal — only a full re-run.
4. **No reversibility.** There is no safe, auditable way to remove a tool and unwind exactly the side effects it added (and *only* those), without risking unrelated `~/.bashrc` content or, worse, the boot configuration.

### 1.2 Product statement

> **envctl is a declarative, GPU-aware environment manager that brings the proven dual-5090 provisioning wizard to a fully observable, idempotent, repairable, and reversible state — by wrapping the existing bash as data-driven TOML components and governing every destructive action with the boot-repair safety discipline.**

### 1.3 Design tenets

- **Wrap, don't rewrite.** Each wizard step becomes a `[[component]]` whose phase hooks hold the original bash *verbatim* (`Hook::Script` / `Hook::ShippedScript`). The engine adds idempotency, ordering, observability, and reversibility around bash that is already proven on the live box.
- **One engine, two front-ends.** All behavior lives in `envctl-engine`. The CLI and GUI are thin shells over the identical `Engine` API, so they can never diverge.
- **Best-effort, never abort the batch.** A failing hook is recorded and the run continues (the wizard's `fail[]` roster), exactly as `yazelix-setup.sh:run()` does.
- **Boot-repair discipline for anything destructive.** Resolve-then-re-verify, dry-run by default, refuse on ambiguity, back up before clobber, never touch user data.
- **Few mainstream deps, stable Rust, no web/WebView.** Single binary per front-end; the GPU telemetry default path shells out to `nvidia-smi` (zero extra deps).

---

## 2. Goals & Non-Goals

### 2.1 Goals

| # | Goal |
|---|------|
| G1 | Provide a **read-only, always-safe `auto-detect`** that produces a complete `EnvReport` (host, GPU, NVIDIA stack, per-component state, drift) — including on the driverless first boot. |
| G2 | Make `install` **idempotent and resumable**: skip-if-present, dependency-ordered, one failure never aborts, dependents of a failure are blocked rather than run on rubble. |
| G3 | Make the environment **repairable** via `auto-fix` and **reversible** via `reset`, both under the boot-repair safety model (dry-run default, guards, backup-before-clobber). |
| G4 | Make the manifest **extensible** via `add-repo`: turn an arbitrary upstream git repo into a first-class, removable, source-built component drop-in. |
| G5 | Stream all activity as **structured events** to both a live console (CLI/GUI) and an on-disk run log, so a crash or closed window never loses the record. |
| G6 | Ship a **native egui GUI** (no web) with live dual-5090 telemetry, a component grid, add-repo form, live logs, and settings — where the UI thread never blocks. |
| G7 | Keep the build **pure-Rust, stable-toolchain, few mainstream deps**, compiling green with and without the optional `gpu-nvml` feature. |

### 2.2 Non-Goals

| # | Non-Goal |
|---|----------|
| N1 | **Not a general-purpose, cross-machine config manager** (not Ansible/Nix-the-OS/Chef). It targets *one* documented machine profile; machine-specific facts (UUIDs, pins) live in the manifest, not in code. |
| N2 | **Not a rewrite of the proven bash.** envctl orchestrates the wizard's bash; it does not reimplement installers in Rust. |
| N3 | **Not an OS installer.** The Subiquity autoinstall (`autoinstall.yaml`) provisions the base OS; envctl manages the post-login toolchain. |
| N4 | **Not a boot-repair tool of its own.** Host boot/driver repair is *delegated* to `ubuntu-boot-repair.sh` (opt-in behind `--allow-boot`); envctl never improvises GRUB/ESP/NVRAM edits. |
| N5 | **Not a daemon / always-on service.** It runs on demand (CLI invocation or GUI session). |
| N6 | **Not a secrets/credentials manager.** Interactive auth (`claude /login`, `gh auth login`) is explicitly out of scope and left to the user. |
| N7 | **No web UI, no WebView, nothing nightly** (the optional `gpu-nvml` feature aside, which is still stable). |

---

## 3. Target User & Personas

### 3.1 Primary persona — "the owner-operator"

A single, expert developer who owns and operates the dual-RTX-5090 box. They:

- Are comfortable with the terminal, Rust, CUDA, Nix, and the curl-pipe-bash supply-chain tradeoff (accepted per HANDOFF).
- Want the box provisioned **unattended from a fresh OS boot** and then kept healthy with minimal ceremony.
- Need to *trust* destructive operations — they will run `reset`/`auto-fix` on a machine they cannot casually re-image, so safety and dry-run-first are non-negotiable.
- Value a fast read-only "what's the state of my box?" answer, and a live dashboard while a 10-minute CUDA build runs.

There is no multi-tenant, no team, no fleet. The product optimizes for *this one operator's trust and throughput*.

### 3.2 The core journey: "fresh OS boot → fully provisioned"

1. The Subiquity autoinstall lays down Ubuntu 26.04 + the apt base (GNOME, Ghostty, Podman, QEMU/KVM, dev base). **First boot is software-rendered** (no NVIDIA driver yet).
2. The operator runs `envctl auto-detect` — and gets a truthful inventory even with no driver loaded (PCI floor sees both GPUs; "REBOOT to load the driver" is reported as a precondition, not a failure).
3. The operator runs `envctl install` — the whole toolchain comes up in dependency order, idempotently, with live logs.
4. After the driver install, **reboot**; the GPU smoke test goes green.
5. Thereafter, `auto-detect` answers "is my box healthy?", `auto-fix` heals drift, `reset` cleanly removes tools, and `add-repo` extends the managed set.

---

## 4. User Journeys

### 4.1 Fresh-boot install (the headline journey)

> *As the operator on a freshly installed, driverless box, I want to bring the entire dev/AI/GPU toolchain up in the right order, idempotently, watching live logs, so that one failed installer doesn't strand me.*

- `envctl auto-detect` → confirms the base, sees 2× GPU via PCI floor, flags `software_rendered` + "reboot to load nvidia-open 610".
- `envctl install` → forward topo order: fonts → bun → node-via-bun → AI CLIs → rustup → rtk → GPU subgraph → gh/vite/wasmer/uv → nix → home-manager → yazelix → desktop/config.
- Already-present components are **Skipped** (idempotent), their wiring still reconciled. A failed component records into `failed[]`; its dependents become `SkippedBlocked`. The GPU subgraph auto-skips on a non-NVIDIA box.
- nvidia-open's verify is **reboot-gated** → reported `RebootRequired`, not `Failed`.
- Reboot → `envctl auto-detect` shows the driver live, smoke test green.

### 4.2 Drift detection ("is my box still healthy?")

> *As the operator, I want a read-only diff between what's installed and what the manifest says should be installed/wired/healthy, with a suggested verb for each divergence.*

- `envctl auto-detect` (or the GUI Dashboard) runs every component's detect (+verify), assembles the `EnvReport`, and computes `drift` as a pure diff.
- Each `DriftItem` carries a `kind` (`Missing` / `Unhealthy` / `WiringMissing` / `DriverInactive`), a `severity`, and a `suggested_verb`. Drift only *proposes* — it never executes. Destructive fixes are surfaced dry-run-first.

### 4.3 Add a repo (extend the managed set)

> *As the operator, I want to point envctl at an upstream git repo and have it built from source, installed user-scope, wired in, and registered as a first-class, removable component.*

- `envctl add-repo <url|path> --id <slug> --build-cmd <cmd> [--ref …]` → validates the slug, refuses collision with a built-in, synthesizes a component TOML, and **atomically** writes a drop-in to `components.d/<slug>.toml` (temp+rename, timestamped backup on collision).
- From then on the five verbs manage it like any built-in. (The full 9-stage build-from-source pipeline is Phase 4; the hardened drop-in writer ships now.)

### 4.4 Break / fix (targeted heal)

> *As the operator, after something drifts or breaks, I want a safe, minimal, dry-run-first repair of just that component.*

- `envctl auto-fix <id>` → **dry-run by default**; review the plan; `--apply` to act. Acts only on BROKEN/PARTIAL components, runs guards before any destructive fix, and (Phase 3) re-verifies after, reverting on verify-failure.
- Host boot/driver repair is opt-in: `--allow-boot` delegates to `ubuntu-boot-repair.sh`'s subcommands, inheriting that script's UUID resolve+re-verify guards unchanged.

### 4.5 Full reset (return to baseline)

> *As the operator, I want to uninstall a tool (or the whole environment) and unwind exactly the side effects it added — and only those — without losing unrelated config or touching my data.*

- `envctl reset <id>` → **dry-run by default**; `--apply` to act. Reverse topo order (tear down dependents first); refuse to strand a depended-upon component unless `--cascade`. `reset --all` requires an extra `--confirm`.
- `Wiring::revert()` excises **only** the owned marker block after a timestamped backup — unrelated `~/.bashrc` content is never lost. Data dirs are never purged unless `--purge` is explicit.

---

## 5. Functional Requirements

The product surface is five verbs that map onto five lifecycle **phases** on every component (`Detect`, `Install`, `Verify`, `Fix`, `Remove`).

### 5.0 The component & manifest model

**REQ-MODEL-1** — A component is **data deserialized from a `[[component]]` TOML table**, not a Rust trait. Each of the five phases is an optional `Hook`; one executor runs all hooks. Adding a tool is adding a TOML file — no recompile.

**REQ-MODEL-2** — A `Hook` is one of three shapes: `command` (clean argv, no shell), `script` (inline bash via `bash -lc`, wrapping wizard fragments verbatim, or a `path` to a script file), or `shipped_script` (reference a whole script the engine never edits — boot-repair, gpu-verify). Exit-code contract: **0 = success**; for Detect/Verify, **0 = present / healthy**.

**REQ-MODEL-3** — A component declares `requires = [...]` (other component ids). The registry builds a DAG and computes a **deterministic Kahn topological order** with ties broken by manifest declaration order, so the built-ins reproduce the wizard's exact proven sequence.

**REQ-MODEL-4** — The registry loads every `<dir>/*.toml` **and** `<dir>/components.d/*.toml` (add-repo drop-ins), dedups by id, and merges base + drop-ins at load. The manifest dir defaults to `./manifest` (override `ENVCTL_MANIFEST_DIR`).

**REQ-MODEL-5** — Setup-time problems are **typed `EngineError`s returned as `Err`**: manifest parse failure, dependency cycle, unknown component, unknown dependency, missing manifest dir. (A cycle is a hard load-time refusal — refuse-on-ambiguity applied to config.) A **failing hook is NOT an `Err`** (see REQ-INSTALL-2).

**REQ-MODEL-6** — Machine-specific values (boot UUIDs, version pins) live in the manifest (guard config / component fields), **never as literals in shared engine code**.

*Acceptance:* the full base manifest (**44 components** across `apt-base`, `base`, `ai-clis`, `gpu`, `dev-tools`, `nix-yazelix`, `boot-repair`) loads, topo-sorts deterministically, and round-trips through serde; a manifest with a cycle or unknown dep fails to load with the corresponding typed error.

---

### 5.1 `auto-detect` — read-only inventory + drift

| | |
|---|---|
| **Phase(s)** | `Detect` (+ optional `Verify`) |
| **Default** | read-only — never writes |
| **CLI** | `envctl auto-detect [--json] [-v] [--gpu]` |

**REQ-DETECT-1** — `Engine::detect` runs every component's detect (and optional verify) plus system probes, **writes nothing**, and returns a serde-serializable `EnvReport` (host, GPU, NVIDIA stack, per-component `ComponentState`, drift). It always exits 0.

**REQ-DETECT-2 (GPU graceful-degradation cascade)** — GPU detection always yields a report, even on a driverless first boot:
- **Tier 0 — PCI floor** (always first, never fails, no driver needed): walk `/sys/bus/pci/devices` + `lspci` for vendor `0x10de`; sets the **authoritative GPU count** (==2 on this box).
- **Tier 1 — NVML** (preferred when the driver is live; optional `gpu-nvml` feature): `Nvml::init()` is fallible by design → driverless boot falls through, never panics.
- **Tier 2 — `nvidia-smi` CSV** (default build, zero extra deps): hard timeout; failure/timeout ⇒ `driver_loaded = false`.

`driver_loaded` / `open_kernel_module` come independently from `/proc/driver/nvidia/version`. `software_rendered = (PCI sees NVIDIA) AND NOT driver_loaded` drives the "reboot to load the driver" precondition.

**REQ-DETECT-3** — Every probe failure is a **non-fatal warning**, never an error. Absence is a normal `Option`/`bool`/enum state. Each probe has a hard timeout.

**REQ-DETECT-4 (drift model)** — `drift::compute(&EnvReport, &Registry)` is a **pure, deterministic, unit-testable** diff producing `DriftItem`s, each with a `kind`, `severity`, `suggested_verb`, and `detail`:

| DriftKind | Trigger | Suggested verb | Severity |
|---|---|---|---|
| `Missing` | declared + installable (has install hook or owned wiring) but not detected | `install` | Medium (Low for meta/group) |
| `Unhealthy` | detected but verify failed | `auto-fix` (**dry-run-first** if destructive) | High |
| `WiringMissing` | detected but its owned shell-rc block is absent | `install` (re-wire) | Low |
| `DriverInactive` | GPUs present but kernel driver not loaded | `install nvidia-open  (then REBOOT)` | High |

**REQ-DETECT-5** — Drift **only proposes** verbs; it never executes. It **never auto-suggests `--apply`** for a destructive component (defense-in-depth: such fixes are surfaced dry-run-first). GPU-skipped components on a non-NVIDIA box are N/A, not drift. Items sort highest-severity-first.

*Acceptance (met & dogfooded):* on the live dual-5090 box, `auto-detect` reports both GPUs pre-driver via the PCI floor; `--json` emits a parseable `EnvReport`; drift is computed from a constructed report in unit tests.

---

### 5.2 `install` — additive, idempotent, streaming

| | |
|---|---|
| **Phase** | `Install` |
| **Default** | **acts** (additive, matching the wizard) |
| **CLI** | `envctl install [COMPONENT...] [--dry-run] [--only] [--skip] [--no-gpu] [--force] [--fail-fast] [--json]` |

**REQ-INSTALL-1 (idempotent)** — Before running a component's install, run its detect; if it passes, mark `Skipped` ("already present"), **reconcile its wiring anyway** (so an installed tool with a missing PATH/rc block is fixed), and continue. Never re-run a `curl | bash` installer that already succeeded.

**REQ-INSTALL-2 (best-effort)** — A non-zero hook exit becomes `OpStatus::Failed` in an `OpResult`, recorded in `RunSummary.failed`; the loop continues (the wizard's `fail[]`). `Engine::run` returns `Err` only for setup-time problems.

**REQ-INSTALL-3 (dependency gate)** — Forward topo order; a component whose `requires` includes a component that failed *this run* is `SkippedBlocked` — never run on a broken foundation.

**REQ-INSTALL-4 (GPU gate)** — `gpu_required` components are pruned with `Skipped("no NVIDIA GPU")` on a GPU-less box. GPU presence is resolved **once per run** into `RunContext` (PCI floor, driver-independent).

**REQ-INSTALL-5 (reboot-gated verify)** — Components whose verify legitimately fails pre-reboot (nvidia-open, driver-dependent torch/cuda-oxide) report `RebootRequired`, **not** `Failed`, so they never poison the failure roster.

**REQ-INSTALL-6 (wiring apply)** — On successful install, `Wiring::apply()` runs idempotently: guarded `# >>> BEGIN <marker> >>> … <<< END <<<` shell-rc blocks written only if `grep -q` of the marker fails; PATH entries appended only if absent; `.desktop` autostarts (incl. one-shot self-disabling); `update-alternatives`.

**REQ-INSTALL-7 (streaming + on-disk log)** — The real `ProcessRunner` spawns each hook with piped stdout/stderr, reads line-by-line, emits each as `Event::Log`, classifies the exit into an `OpResult`, and **tees every line to `~/.local/state/envctl/envctl.log`** (the analogue of `~/yazelix-setup.log`). Per-hook timeout + `catch_unwind` isolate one bad component from the run. `needs_sudo` is pre-warmed once (with keepalive) so streamed, TTY-less hooks don't prompt mid-run; no TTY ⇒ fail fast with a warning rather than hang.

*Acceptance (met & dogfooded):* real streaming install with wiring, timeouts, and sudo keepalive validated on the live box; idempotent re-run skips present components; a forced failure blocks its dependents.

---

### 5.3 `auto-fix` — repair (destructive-capable)

| | |
|---|---|
| **Phase** | `Fix` |
| **Default** | **dry-run** (`--apply` to act) |
| **CLI** | `envctl auto-fix [COMPONENT...] [--apply] [--only-wiring] [--allow-boot] [--json]` |

**REQ-FIX-1** — `auto-fix` is **dry-run by default**; mutation requires `--apply`.

**REQ-FIX-2** — It acts only on **BROKEN/PARTIAL** components (detected-but-unhealthy, or missing wiring), applying minimal corrective fix-hook / wiring actions.

**REQ-FIX-3 (atomic heal — Phase 3)** — Each fix is **backup → apply → re-verify**, reverting on verify-failure, so a failed heal does not leave a worse state than it found.

**REQ-FIX-4 (guards)** — Before any *destructive* fix, all guards must pass (REQ-SAFE-*). Any failing guard ⇒ `Refused`, never `Failed`/panic.

**REQ-FIX-5 (boot delegation)** — `auto-fix` never improvises GRUB/ESP/driver fixes. It surfaces the exact `ubuntu-boot-repair.sh {diagnose|repair-dev|rename-pro|finalize}` subcommand and only invokes it under `--allow-boot`, inheriting that script's resolve+re-verify / same-disk / never-touch-`/home` / NVRAM-verify guards unchanged. Machine UUIDs/bl-ids come from the manifest `guards` config.

*Status:* dry-run defaults + guard refusal shipped; system-scope revert + post-verify in progress (Phase 3).

---

### 5.4 `reset` — uninstall + unwind (destructive)

| | |
|---|---|
| **Phase** | `Remove` |
| **Default** | **dry-run** (`--apply` to act) |
| **CLI** | `envctl reset [COMPONENT...] [--apply] [--cascade] [--all --confirm] [--keep-config] [--purge] [--json]` |

**REQ-RESET-1** — `reset` is **dry-run by default**; mutation requires `--apply`. `reset --all` (whole-environment baseline) requires an **additional `--confirm`**.

**REQ-RESET-2 (reverse order + dependents)** — Traverse the dependency graph **reversed** (tear down dependents first). Refuse to remove a depended-upon component unless its reverse-dependents are also in the set or `--cascade` is given.

**REQ-RESET-3 (anti-clobber revert)** — `Wiring::revert()` performs a **timestamped backup** then excises **only** the owned marker block (`# >>> BEGIN <marker> >>> … <<< END <<<`); unrelated `~/.bashrc`/config content is never lost. Symlink/unit deletion tolerates missing targets.

**REQ-RESET-4 (post-verify — Phase 3)** — After `--apply`, re-detect to confirm the component is **ABSENT**; report if removal did not fully take.

**REQ-RESET-5 (never touch data)** — `reset`/`auto-fix` never purge data dirs unless `--purge` is explicit (and even then re-verify the path). Disks flagged as data/home by UUID are skipped by destructive verbs.

*Status:* dry-run defaults, reverse-order, guard refusal, owned-block revert shipped; system-scope revert (`/etc/nix`, `/etc/apt`, `/etc/cdi`, alternatives) + post-verify in progress (Phase 3).

---

### 5.5 `add-repo` — build-from-source + wire-in

| | |
|---|---|
| **Pipeline** | synthesize component → register drop-in → (Phase 4) build & wire |
| **CLI** | `envctl add-repo <url\|path> --id <slug> --build-cmd <cmd> [--ref …] [--bin …] [--depends …] [--dry-run] [--no-wire] [--force] [--json]` |

**REQ-ADDREPO-1 (validation, fail-closed)** — Strict slug validation (safe as a bare TOML key *and* a filename component); refuse id-collision with any existing/built-in component; reject inputs that could break out of the emitted TOML literal (e.g. `'''`).

**REQ-ADDREPO-2 (atomic drop-in)** — Synthesize a component TOML (provenance: url, ref, build_cmd, fetched-at) and write it **atomically** (temp + rename) to `components.d/<slug>.toml`, with a **timestamped backup** of any existing drop-in. Clean removal = delete the drop-in (it never edits a shared file in place).

**REQ-ADDREPO-3 (dry-run)** — `--dry-run` prints the exact TOML that would be written and changes nothing.

**REQ-ADDREPO-4 (full pipeline — Phase 4)** — The complete 9-stage pipeline: acquire (clone/fetch, record resolved SHA, snapshot local paths), detect build system (nix flake > cargo > meson > cmake > make > python/uv > bun/npm), best-effort dep resolution, build (kit CUDA/LLVM/PATH env sourced, SHA+flags-keyed cache), locate+classify artifacts, install (symlink-default into `~/.local`, backup-before-clobber), wire-in (guarded PATH/completions/desktop/systemd `--user`), register, verify. All user-scope, best-effort, refuse-on-ambiguity.

*Status:* hardened drop-in writer (validation, collision-refusal, atomic write, dry-run, clone into `~/.local/share/envctl/repos/<slug>` at 0700, escaped interpolation) shipped; full 9-stage build pipeline is Phase 4.

---

## 6. Safety Requirements (the boot-repair gold standard)

These are **hard invariants** inherited verbatim from `ubuntu-boot-repair.sh`'s `resolve_verified()` + `die()` discipline. They are unconditional: **there is no `--force` that bypasses them.**

**REQ-SAFE-1 (dry-run by default for destructive verbs)** — `reset` and `auto-fix` default to `dry_run = true`; mutation requires explicit `--apply`. `reset --all` requires an additional `--confirm`. `install` keeps the opposite (additive ⇒ acts), matching the wizard.

**REQ-SAFE-2 (resolve-once, no TOCTOU)** — A `RunContext` resolves live-device identities + GPU presence **once per run**; guards read from it. (`gpu_present` via PCI floor, `live_root_uuid` via findmnt+blkid.)

**REQ-SAFE-3 (resolve + re-verify before acting)** — Guards resolve targets by UUID and **re-verify** the device actually carries that UUID before any touch — the `resolve_verified()` chain expressed as the declarative `Guard` enum: `UuidResolves`, `NotLiveDevice`, `NotMounted`, `PathExists`, `HookSucceeds`.

**REQ-SAFE-4 (fail-closed guards)** — Guards are implemented **fail-closed**: when a guard cannot prove an op is safe (UUID won't resolve, re-verify mismatches, `blkid`/`findmnt` missing or erroring, target mounted, target is the live device), it returns a refusal → `OpStatus::Refused` + a red `GuardRefused` event. It **never silently passes**. A unit test asserts a bogus-UUID guard refuses.

**REQ-SAFE-5 (refuse on ambiguity, not panic)** — A failing guard yields a **safe abort** (`Refused`), never a `Failed`/panic. `reset` refuses to strand a dependent.

**REQ-SAFE-6 (back up before clobber)** — Every user-file edit is **resolve → re-verify → timestamped backup (`.bak.<epoch>`) → apply**. `revert()` excises **only** the owned marker block (the anti-clobber guarantee).

**REQ-SAFE-7 (never touch user DATA)** — Like the boot script never mounting `/home`, destructive verbs never purge data dirs unless `--purge` is explicit (and re-verify even then). Data/home disks are flagged by UUID and skipped.

**REQ-SAFE-8 (host boot repair is opt-in + delegated)** — envctl never improvises boot/driver repair; it delegates to `ubuntu-boot-repair.sh` only under `--allow-boot`, inheriting that script's guards unchanged. The four subcommands' machine UUIDs/bl-ids live in the manifest, never as engine literals.

**REQ-SAFE-9 (sudo only where the wizard used it)** — apt, nix-daemon restart, update-alternatives, the boot subcommands. The common path is user-scope. Sudo is pre-warmed once per run with a keepalive; no-TTY fails fast rather than hanging.

### 6.1 Forbidden states (must never occur)

| ID | Forbidden state |
|---|---|
| FS-1 | A destructive op runs without `--apply` (or `reset --all` without `--confirm`). |
| FS-2 | A guard that cannot prove safety silently allows the op to proceed. |
| FS-3 | A `revert()` removes any content other than the exact owned marker block. |
| FS-4 | Any user file is overwritten without a prior timestamped backup. |
| FS-5 | A data/`/home` filesystem is unmounted, purged, or written without explicit `--purge` + re-verify. |
| FS-6 | A boot/GRUB/ESP/NVRAM edit happens outside the delegated `ubuntu-boot-repair.sh` path (i.e. without `--allow-boot`). |
| FS-7 | A single component's hook failure or panic aborts the whole run. |
| FS-8 | A `--force` flag bypasses any REQ-SAFE-* guard. |

---

## 7. GUI Requirements (eframe / egui — native, immediate-mode, no web)

**REQ-GUI-1 (no-block UI)** — The App spawns **one** long-lived serial worker thread (so two destructive sudo/apt/nix ops can never race) and two mpsc channels (commands App→worker, events worker→App). The UI thread drains events via `try_recv` at the top of every `update()`; the worker calls `ctx.request_repaint()` after each event (~0% CPU at rest). No Mutex on the hot path; per-component **busy** state is a `HashSet` in the App.

**REQ-GUI-2 (Dashboard / telemetry)** — Global health pill (Healthy / N degraded / M failed) from the last detect+verify; the `fail[]` roster from the last action; a row of **dual-RTX-5090** cards (name/driver/temp/util%/VRAM) + CPU + memory strips with painter-drawn sparklines (~120-sample ring). Pre-reboot shows a **yellow "REBOOT to load nvidia-open 610" card** (`DriverNotActive`), never an error.

**REQ-GUI-3 (Components grid)** — A `TableBuilder` grid: Name · Category · State (colored dot) · Version · Wiring badges · Actions. Per-row Install/Fix/Remove/Verify dispatch `EngineCommand`s; busy rows spin + disable. Search + category filter. A destructive Remove opens a **confirmation modal naming exactly what will be unwired/backed up**.

**REQ-GUI-4 (Add-Repo screen)** — Form (URL + ref, build-kind combo, command overrides, wiring checkboxes, **Dry-run toggle default ON**). "Validate" runs detect-only; "Build & wire" dispatches to the worker and auto-switches focus to Live Logs; inline validation errors.

**REQ-GUI-5 (Live Logs)** — `ScrollArea::stick_to_bottom` over a bounded `VecDeque` ring (cap ~8000, oldest dropped). Per-line color by level, level/component filters, pause-autoscroll, clear, copy/save, sticky active-step header. The same stream is tee'd to disk.

**REQ-GUI-6 (Settings / Manifest)** — Read-mostly viewer: each component's five hooks (read-only monospace) + wiring badges; global options — telemetry-interval slider, log-cap, **"destructive ops dry-run by default" checkbox (ON)**, "require confirmation for Remove/Reset/AutoFix", "Reload manifest from disk". Deep edits happen in `$EDITOR`; the manifest is the source of truth.

**REQ-GUI-7 (telemetry cadence)** — A dedicated ~1s sampler thread emits `Event::Telemetry` while the Dashboard is active, backing off to ~3–5s off-Dashboard and pausing when the window is unfocused — so a 10-minute CUDA build never starves the GPU gauges and never needlessly spawns `nvidia-smi`.

*Status:* Dashboard + Components grid render the live `EnvReport` read-only (Phase 1); per-row install + Live Logs streaming (Phase 2); confirmation modals, full telemetry, and polish are Phase 3/5.

---

## 8. Non-Functional Requirements

**REQ-NFR-1 (pure Rust, single binary)** — One Cargo workspace, three members: `envctl-engine` (lib), `envctl` (CLI bin), `envctl-gui` (eframe bin). **All behavior lives in the library** so CLI and GUI cannot diverge. Each front-end is a self-contained binary.

**REQ-NFR-2 (stable toolchain, few mainstream deps)** — Compiles on **stable Rust**, nothing nightly. Engine deps: serde, toml, anyhow, thiserror, sysinfo, which, chrono (+ optional `nvml-wrapper` behind `gpu-nvml`, serde_json for `lsblk -J`). CLI adds clap; GUI adds only eframe/egui/egui_extras. No web, no WebView. `cargo build` must be green **with and without** the `gpu-nvml` feature.

**REQ-NFR-3 (best-effort)** — One component's failure (or panic, isolated via `catch_unwind`) never aborts the run. The run always ends with a `RunSummary` roster (`failed` / `refused` / `skipped_blocked`). `RunSummary::ok()` ⟺ no failures and no refusals.

**REQ-NFR-4 (observability)** — The engine **never prints**; it emits structured `Event`s over an mpsc channel (`RunStarted` / `StepStarted` / `Log` / `StepFinished` / `Telemetry` / `GuardRefused` / `RunFinished`). The CLI drains on the main thread and pretty-prints (`--json` ⇒ NDJSON for scripting); the GUI drains via `try_recv`. The line stream is tee'd to `~/.local/state/envctl/envctl.log` so a crash never loses the record; the log is replayable.

**REQ-NFR-5 (deterministic & reproducible)** — Dependency order is a deterministic Kahn sort (declaration-order tie-break), so built-ins reproduce the wizard's proven sequence and runs are reproducible.

**REQ-NFR-6 (XDG layout)** — Drop-ins: `~/.config/envctl/components.d/*.toml`. Repos: `~/.local/share/envctl/repos/<slug>` (0700). State/log: `~/.local/state/envctl/envctl.log`. add-repo builds install user-scope under `~/.local`.

**REQ-NFR-7 (testability)** — The one behavioral seam (`HookRunner`) lets tests inject `DryRunRunner` / recording runners. Targets: golden runs, drift fixtures, wiring apply/revert round-trip, guard refusal cases, manifest serde round-trip.

---

## 9. Success Metrics

| # | Metric | Target |
|---|--------|--------|
| M1 | **Fresh-box provisioning** | A freshly installed, driverless box reaches a **green GPU smoke test** (nvidia-smi sees 2× 5090, `torch.cuda` runs an sm_120 kernel, cargo-oxide works, Podman CDI works) after `envctl install` + one reboot, **unattended** on the common path. |
| M2 | **Truthful pre-driver detect** | On the software-rendered first boot, `auto-detect` reports both GPUs (PCI floor) and a `DriverInactive`/reboot precondition — **zero false failures**. |
| M3 | **Idempotency** | Re-running `install` on a provisioned box performs **zero re-installs** (all `Skipped`), only reconciling missing wiring. |
| M4 | **Clean reset** | `reset --apply` of any component returns it to ABSENT with **zero orphaned system config** and **zero loss of unrelated `~/.bashrc`/config** content. |
| M5 | **Safety: no forbidden states** | Across all runs, **none of FS-1…FS-8 ever occur**; every ambiguous destructive op is `Refused`, never silently executed. |
| M6 | **Best-effort resilience** | A single failing installer never aborts the batch; the final `RunSummary` accurately rosters `failed` / `refused` / `skipped_blocked`. |
| M7 | **Responsive GUI** | The UI thread never blocks during a multi-minute CUDA build; telemetry keeps updating; CPU at rest ≈ 0%. |
| M8 | **Build hygiene** | `cargo build` green on stable, **with and without** `gpu-nvml`; clippy clean. |
| M9 | **Extensibility** | `add-repo` of a real cargo `--git` tool makes it appear in `auto-detect` and be managed by install/reset/auto-fix end-to-end. |

---

## 10. Milestones / Status

Six roadmap phases. **Phases 0–2 are complete and dogfooded on the live dual-5090 box; Phase 3 is in progress; Phases 4–5 are pending.**

| Phase | Scope | Status |
|---|---|---|
| **0 — Scaffold that compiles** | Green Cargo workspace, full type skeleton, event/runner seams, shared-engine boundary, fail-closed guard engine up front, one example manifest deserializes in a test. | ✅ **Done** |
| **1 — auto-detect (read-only)** | `Registry::load` + topo sort; host/GPU/wiring/tool probes; three-tier GPU cascade (PCI floor validated pre-driver on the live box); `EnvReport` + preconditions + warnings; pure drift diff; CLI `auto-detect [--json]`; GUI Dashboard + Components grid read-only. | ✅ **Done & dogfooded** |
| **2 — install (additive, idempotent, streaming)** | `ProcessRunner` line-streaming + tee to disk; best-effort `run_phase` (gpu skip, NoHook, catch_unwind, sudo keepalive, timeouts); idempotent skip-if-detected; `Wiring::apply` (grep-guarded rc blocks, PATH, .desktop, alternatives); **full 44-component base manifest**; forward traversal w/ SkippedBlocked + RebootRequired; CLI install flags; GUI per-row install + Live Logs + health pill. | ✅ **Done & dogfooded** |
| **3 — reset + auto-fix (destructive, full guard discipline)** | `RunContext` resolve-once; `Wiring::revert` backup-then-excise-owned-block; reset reverse-order/`--cascade`/`--all --confirm`/re-detect-ABSENT; auto-fix BROKEN/PARTIAL-only, atomic backup→apply→verify w/ revert-on-failure; **system-scope revert** (`/etc/nix`, `/etc/apt`, `/etc/cdi`, alternatives); boot-repair delegation behind `--allow-boot`; GUI confirmation modals. | 🚧 **In progress** (dry-run defaults, guards, owned-block revert shipped; system-scope revert + post-verify being completed) |
| **4 — add-repo (build-from-source + wire-in)** | Full 9-stage pipeline (acquire→detect-build-system→deps→build→locate→install→wire→register→verify); component synthesis + atomic drop-in; refuse-on-ambiguity edges; CLI/GUI add-repo; end-to-end on a real cargo `--git` tool. | ⏳ **Pending** (hardened drop-in writer already shipped) |
| **5 — GUI polish + telemetry + hardening** | Live ~1s sampler w/ cadence backoff + sparklines; yellow DriverNotActive card; Settings/Manifest screen; Live Logs ring + filters; NDJSON CLI stream; RecordingRunner golden runs, drift/wiring/guard tests; green build with/without `gpu-nvml`. | ⏳ **Pending** |

---

## 11. Open Questions / Risks

| # | Item | Type | Notes / mitigation |
|---|------|------|--------|
| R1 | **Curl-pipe-bash supply chain.** Many installers (Claude/Kimi/Devin/Bun/rustup/wasmer/uv/Nix) are `curl \| bash`. | Risk | Accepted per HANDOFF; could be hardened with checksums / pinned script versions if desired. envctl preserves them verbatim, so hardening is a manifest-level change. |
| R2 | **Point-in-time version pins** (cuda-toolkit-13-3, nightly-2026-04-03, torch 2.12/cu132, Codex/Gemini npm, yazelix Cachix key). | Risk | Pins live in the manifest; re-verify at build time. `version_lock` drives drift severity so a locked mismatch is loud. |
| R3 | **Driver intentionally unlocked** (live 595-vs-desired-610). | Open | Per HANDOFF Decision #5, the driver is not frozen; a driver mismatch should be **Warn**, not Critical. An optional apt pin to freeze it was explicitly deferred. |
| R4 | **Secure Boot OFF is load-bearing** (nvidia-open 610 is not Canonical-signed). | Risk | Only safe unattended because SB is off; a re-enabled Secure Boot would break the unattended driver install. Should be surfaced as a precondition by `auto-detect`. |
| R5 | **Single-machine assumptions baked into the manifest** (boot UUIDs, GPU count==2, device id 0x2b85). | Open | These are manifest/config, not code — but portability to a second box requires a new manifest profile, not a code change. Is a second profile in scope? |
| R6 | **System-scope revert completeness** (`/etc/nix`, `/etc/apt`, `/etc/cdi`, update-alternatives). | Open (Phase 3) | Reverting system-scope wiring must honor the same backup-before-clobber + fail-closed discipline as user-scope; this is the active Phase 3 work and the highest-risk safety surface remaining. |
| R7 | **add-repo build-system breadth.** The 9-stage pipeline must handle nix/cargo/meson/cmake/make/python/bun without hard-failing on the long tail. | Open (Phase 4) | Best-effort + refuse-on-ambiguity (unknown build system without `--build-cmd` ⇒ refuse) bounds the risk; the drop-in writer is already hardened. |
| R8 | **Interactive auth is out of scope.** `claude /login`, `gh auth login`, etc. are not automatable. | Known limitation | envctl provisions the binaries; the operator authenticates. Should be documented in the install summary. |
| R9 | **Reboot-gated verifies depend on operator action.** Several verifies cannot pass until the operator reboots. | Known behavior | Modeled as `RebootRequired` (not `Failed`) + a yellow GUI card; `auto-detect` reports the reboot precondition. |

---

*This PRD is grounded in the repository's `README.md`, `ARCHITECTURE.md`, `ROADMAP.md`, `DESIGN-NOTES.md`, the seven manifest files (44 components), the engine source (`lib.rs`, `component.rs`, `model.rs`, `executor.rs`, `drift.rs`, `guard.rs`), and the machine/mission context in `assets/scripts/HANDOFF.md`. Where requirements describe Phase 3–5 behavior, status is marked accordingly.*
