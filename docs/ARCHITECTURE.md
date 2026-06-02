# envctl — Architecture

> **Manifest shape (authoritative):** every component is a `[[component]]` array-of-tables entry with `[component.<phase>]` hook sub-tables and `[[component.guards]]`, exactly as in `manifest/*.toml`. Some inline examples below may show a flattened form for brevity — the loader (`Registry::load`) requires the `[[component]]` shape.

---

# envctl — ARCHITECTURE.md

> A personal, GPU-aware, source-building environment manager for one specific box:
> Ubuntu 26.04 LTS, dual NVIDIA RTX 5090 (GB202 / Blackwell / sm_120) developer workstation.
> Pure Rust. Native egui/eframe GUI (no web, no WebView). One engine library shared by a CLI and a GUI.

---

## 1. Overview

`envctl` orchestrates the exact toolchain the existing first-login wizard
(`yazelix-setup.sh`) installs — Bun + node→bun, the AI CLIs, rustup/rtk, CUDA 13.3 +
nvidia-open 610 + LLVM/clang-21, cuda-oxide, the NVIDIA Container Toolkit, PyTorch cu132,
gh/Vite/wasmer/uv, Nix + the yazelix Cachix cache, home-manager, yazelix + `yzx desktop`,
the guarded `~/.bashrc` blocks, and the GPU smoke-test autostart — but makes it
**idempotent, resumable, observable, and reversible**.

It does this without rewriting any of the proven bash. Each wizard step becomes a
**component** in a declarative TOML manifest, and the component's lifecycle phases hold the
original bash **verbatim** as wrapped hooks. The engine adds three things the bash never had:

1. **Idempotency + best-effort orchestration** — modeled exactly on the wizard's `run()`
   wrapper (`yazelix-setup.sh:27-32`): a step's nonzero exit is *recorded* and the run
   *continues*; the final summary is the wizard's `fail[]` roster.
2. **Reversibility + safety** — modeled exactly on `ubuntu-boot-repair.sh`: destructive ops
   resolve-then-re-verify by UUID, default to dry-run, refuse on ambiguity, back up before
   clobber, and never touch the data `/home`.
3. **Streaming observability** — the engine never prints; it emits structured `Event`s over a
   channel. The CLI drains them on the main thread and pretty-prints; the GUI drains them in
   its `update()` loop and renders live logs + GPU telemetry without ever blocking the UI thread.

The product surface is **five verbs**: `reset · install · auto-detect · add-repo · auto-fix`.

---

## 2. Workspace / crate layout

A single Cargo workspace, three members, **all behavior in the library** so the CLI and GUI
can never diverge:

```
envctl/                         # cargo workspace root
├── Cargo.toml                  # [workspace] members + shared [workspace.dependencies]
├── crates/
│   ├── envctl-engine/          # lib  — THE shared core (no printing, no clap, no egui)
│   │   ├── src/
│   │   │   ├── lib.rs          # pub re-exports; `Engine` entrypoint
│   │   │   ├── component.rs    # Component, Hook, Guard, Phase, HookRunner
│   │   │   ├── model.rs        # Registry, RunPlan, RunSummary, OpResult, Wiring, AddRepoSpec
│   │   │   ├── report.rs       # EnvReport + sub-structs (auto-detect / drift)
│   │   │   ├── event.rs        # Event, EventSink, Stream, Telemetry
│   │   │   ├── error.rs        # EngineError (thiserror) + run_phase() best-effort wrapper
│   │   │   ├── exec.rs         # ProcessHookRunner (spawn + stream), DryRunner, RecordingRunner
│   │   │   ├── wiring.rs       # apply()/revert() for shell-rc/PATH/desktop/systemd
│   │   │   ├── guard.rs        # RunContext + guard evaluation (UUID resolve+re-verify)
│   │   │   ├── detect/         # probes (host, gpu cascade, tool_versions, wiring_state)
│   │   │   ├── addrepo.rs      # the 9-stage build-from-source pipeline
│   │   │   └── telemetry.rs    # sampler thread (nvidia-smi CSV default; NVML optional)
│   │   └── manifests/          # SHIPPED base manifest (one *.toml per component)
│   ├── envctl/                 # bin  — thin CLI (clap) over the engine
│   └── envctl-gui/             # bin  — eframe/egui native app over the engine
└── README.md
```

| Crate | Kind | Depends on | Job |
|---|---|---|---|
| `envctl-engine` | lib | serde, toml, anyhow, thiserror, sysinfo, which, chrono | Manifest loading, the Component/Hook executor, Wiring apply/revert, Guard evaluation, auto-detect + drift, telemetry sampler, the Event API. Pure library. |
| `envctl` | bin | `envctl-engine`, clap | CLI: subcommands map 1:1 to the five verbs (+ `--dry-run`/`--apply`/`--json`/`--only`). Drains events on the main thread, pretty-prints with the wizard's `c_ok/c_warn/c_step` colors, exits nonzero iff the summary has failures/refusals. |
| `envctl-gui` | bin | `envctl-engine`, eframe, egui, egui_extras | Native dashboard. Runs the engine on a worker thread, drains events via `try_recv` in `update()`, renders the component grid + live logs + dual-5090 telemetry. Its only non-engine deps are eframe/egui(+extras). |

**Dependency discipline (constraint: mainstream + few, stable Rust).** Everything compiles on
stable. `serde`/`toml`/`anyhow`/`thiserror` live behind the engine's public API; the GUI sees
only the engine's types. GPU telemetry's default path shells out to `nvidia-smi` (zero extra
deps, exactly what the kit already does); typed NVML (`nvml-wrapper`) is an **optional cargo
feature** (`gpu-nvml`) that upgrades telemetry to a fork-free FFI when available and degrades to
`nvidia-smi` then the PCI floor when not. `sysinfo` covers CPU/mem/disk/kernel; `which` locates
binaries for version probes; `chrono` stamps reports. No web, no WebView, nothing nightly.

---

## 3. The Component model (data-driven, not a trait-per-tool)

**Decision:** a component is **DATA deserialized from TOML**, not a Rust trait you implement per
tool. Each of the five lifecycle phases is an optional **`Hook`** holding the wizard's bash
verbatim. One executor runs all hooks. *Adding a tool means adding a TOML file, never
recompiling.* The only behavioral trait we keep is `HookRunner`, so tests/GUI can inject a
dry-run or recording runner.

```rust
/// The five verbs map onto these phases on every component.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase { Detect, Install, Verify, Fix, Remove }

pub struct Component {
    pub id: String,                 // stable key: "cuda-toolkit", "bun", "yazelix"
    pub name: String,               // GUI label
    pub description: String,
    pub category: String,           // fonts|js-runtime|ai-clis|rust|gpu-cuda|python|
                                    //   dev-tools|nix-stack|yazelix|system-repair|meta
    pub requires: Vec<String>,      // ordering deps (other component ids)
    pub gpu_required: bool,         // skip-with-reason when no NVIDIA GPU
    pub destructive: bool,          // remove/fix obey guards + dry-run
    pub reboot_gated_verify: bool,  // verify may legitimately fail pre-reboot (driver/torch)
    pub version_lock: Option<String>,// drift severity driver: locked mismatch = Critical

    // The five phase hooks; all optional (a component may only detect+verify).
    pub detect:  Option<Hook>,
    pub install: Option<Hook>,
    pub verify:  Option<Hook>,
    pub fix:     Option<Hook>,
    pub remove:  Option<Hook>,

    pub wiring: Wiring,             // declarative side effects (PATH/rc/desktop/systemd)
    pub guards: Vec<Guard>,         // checked before any destructive phase
    pub machine: BTreeMap<String,String>, // machine-specific config (UUIDs, bl-ids) — NOT literals in code
}
```

### 3.1 Hook — proven bash, wrapped (three shapes)

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Hook {
    /// Clean argv, no shell. e.g. command="nvidia-smi" args=["-L"].
    Command { command: String, args: Vec<String>, env: BTreeMap<String,String>, needs_sudo: bool },
    /// Inline bash run as `bash -lc <script>` so PATH/.bashrc additions resolve.
    /// This wraps wizard fragments VERBATIM (e.g. the bun installer, the apt block).
    Script  { script: String, path: Option<String>, env: BTreeMap<String,String>,
              needs_sudo: bool, login_shell: bool /* default true */ },
    /// Reference a whole shipped script the engine NEVER edits
    /// (yazelix-gpu-verify.sh, yazelix-config.sh, ubuntu-boot-repair.sh subcommands).
    ShippedScript { path: String, args: Vec<String>, needs_sudo: bool },
}
```

Exit-code contract: **0 = success**; for `Detect`/`Verify`, **0 = present / healthy**. This is
the same convention the wizard uses (`command -v bun`, `grep -q 'BEGIN cuda env'`, `nvidia-smi -L`).

### 3.2 Guard — the boot-repair discipline, declarative

A guard is evaluated **before any destructive phase** (`Remove`/`Fix` on a `destructive`
component). ANY failing guard yields `OpStatus::Refused` (a safe abort, *not* a `Failed`/panic) —
this is `ubuntu-boot-repair.sh`'s `resolve_verified()` + `die()` refusal chain expressed as data.

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Guard {
    UuidResolves   { uuid_key: String }, // resolve dev by UUID AND re-verify it carries that UUID
    NotLiveDevice  { uuid_key: String }, // refuse if resolved dev == live/running dev
    NotMounted     { uuid_key: String }, // refuse if the path/mount is in use (the "never umount /home")
    PathExists     { path: String },     // refuse unless a shim/venv exists before touching it
    HookSucceeds   { hook: Hook },       // generic read-only predicate (e.g. lspci nvidia)
}
```

`uuid_key` indexes `Component.machine` (e.g. `machine.dev_root_uuid = "7f8c16c8-…"`), so the
machine-specific UUIDs from `ubuntu-boot-repair.sh` live in the **manifest**, never as literals
in shared engine code.

### 3.3 Wiring — the guarded side effects, declarative + reversible

`install` calls `Wiring::apply()` (idempotent, marker-guarded); `remove` calls `revert()`
(removes exactly what `apply` added, backing up first); `verify`/`fix` reconcile. Each kind maps
to a proven wizard idiom.

```rust
pub struct Wiring {
    pub path_entries:   Vec<String>,      // dirs PATH-appended only if absent (grep-guarded)
    pub shell_rc:       Vec<ShellRcBlock>,// guarded `# >>> BEGIN <marker> >>> ... <<< END <<<` blocks
    pub desktop_entries:Vec<DesktopEntry>,// ~/.config/autostart/*.desktop (one_shot self-disabling supported)
    pub systemd_user:   Vec<SystemdUnit>, // systemctl --user units (unused by built-ins; for add-repo)
    pub alternatives:   Vec<Alternative>, // update-alternatives (ghostty x-terminal-emulator)
}
```

* `ShellRcBlock { file, marker, content }` → rendered as the wizard's
  `# >>> BEGIN <marker> (added by envctl) >>> … # <<< END <marker> <<<`, written only if
  `grep -q` of the marker fails. `revert()` excises **only** the marked block (`sed
  '/BEGIN/,/END/d'`) after a timestamped backup — the **anti-clobber guarantee**: unrelated
  `~/.bashrc` content is never lost.
* `DesktopEntry { filename, content, one_shot }` → models the gpu-verify autostart that
  self-deletes once the driver is live.
* `path_entries` → the `$HOME/.bun/bin`, `$HOME/.cargo/bin`, `$HOME/.nix-profile/bin`, CUDA-bin
  appends, each guarded.

### 3.4 The one behavioral seam

```rust
pub trait HookRunner: Send + Sync {
    fn run(&self, comp: &str, phase: Phase, hook: &Hook, ctx: &RunContext) -> OpResult;
}
```

* `ProcessHookRunner` — real: spawns the process with piped stdout/stderr, reads it
  line-by-line, emits each line as `Event::Log`, classifies the exit into an `OpResult`. Also
  tees every line to the on-disk log.
* `DryRunner` — prints/emits the plan, executes nothing, returns `OpStatus::DryRun`.
* `RecordingRunner` — for tests/fixtures (replay a recorded run).

---

## 4. Manifest (TOML)

The shipped base manifest is one `*.toml` file per component under
`crates/envctl-engine/manifests/`. User/`add-repo` drop-ins live under
`~/.config/envctl/components.d/*.toml`. The `Registry` merges base + every drop-in at load, so
`add-repo` never edits a shared file in place (clean removal = delete the drop-in).

```toml
# manifests/cuda-toolkit.toml — one component, wrapping the wizard fragment verbatim.
id           = "cuda-toolkit"
name         = "CUDA 13.3 + nvidia-open 610"
category     = "gpu-cuda"
requires     = ["nvidia-cuda-repo"]
gpu_required = true
destructive  = false           # install additive; remove is the destructive phase
version_lock = "13.3"          # locked -> a 13.x mismatch is Critical drift

[detect]
kind = "command"
command = "bash"
args = ["-lc", "command -v nvcc && nvcc --version | grep -q 'release 13.3'"]

[install]                      # the proven apt block, wrapped verbatim
kind = "script"
needs_sudo = true
script = "sudo apt-get install -y cuda-toolkit-13-3"

[verify]                       # shipped smoke test, referenced (never edited); reboot-gated
kind = "shipped_script"
path = "~/.local/bin/yazelix-gpu-verify.sh"

[fix]
kind = "command"
command = "sudo"
args = ["apt-get", "install", "-y", "--reinstall", "cuda-toolkit-13-3"]

[remove]                       # destructive -> guards + dry-run default
kind = "command"
needs_sudo = true
command = "sudo"
args = ["apt-get", "remove", "-y", "cuda-toolkit-13-3"]

[wiring]
path_entries = ["/usr/local/cuda/bin"]   # the cuda-env-block component owns the rc block

[[guards]]                     # refuse remove on a non-GPU box
kind = "hook_succeeds"
[guards.hook]
kind = "command"
command = "bash"
args = ["-lc", "lspci | grep -qiE 'nvidia'"]
```

**Schema reference.** Hook `kind ∈ {command, script, shipped_script}`. Guard
`kind ∈ {uuid_resolves, not_live_device, not_mounted, path_exists, hook_succeeds}`. Wiring kinds:
`path_entries[]`, `shell_rc[]`, `desktop_entries[]`, `systemd_user[]`, `alternatives[]`. Pins
(`cuda-toolkit-13-3`, `nightly-2026-04-03`, `torch cu132`, `llvm/clang-21`, the yazelix Cachix
public key, the boot-repair UUIDs/bl-ids) are manifest values, not hard-coded constants.

---

## 5. Engine entrypoints, results, and the verb→phase mapping

```rust
pub struct RunPlan { pub phase: Phase, pub targets: Vec<String>, pub dry_run: bool } // []==all, topo order

pub struct OpResult { pub component: String, pub phase: Phase, pub status: OpStatus,
                      pub exit_code: Option<i32>, pub duration_ms: u128, pub message: String, pub dry_run: bool }

#[serde(rename_all="snake_case")]
pub enum OpStatus { Ok, Failed, Skipped, Refused, DryRun, NoHook, SkippedBlocked, RebootRequired }

pub struct RunSummary { pub results: Vec<OpResult>, pub failed: Vec<String>, pub refused: Vec<String>,
                        pub skipped_blocked: Vec<String> }
impl RunSummary { pub fn ok(&self) -> bool { self.failed.is_empty() && self.refused.is_empty() } }
```

The five verbs are thin wrappers over `(Phase, RunPlan)`:

| Verb | Phase(s) | dry-run default | Notes |
|---|---|---|---|
| `auto-detect` | `Detect` (+`Verify`) | n/a (read-only) | Returns `EnvReport`. Never writes. The sensing substrate the other four consume. |
| `install` | `Install` | **false** (additive) | Forward topo order; skip-if-present; apply wiring idempotently; record-and-continue. |
| `add-repo` | synthesize → `Install` | false | Build-from-source pipeline (§8), then registers + installs a new component. |
| `auto-fix` | `Fix` | **true** (destructive-capable) | Repairs BROKEN/PARTIAL; `--allow-boot` delegates to `ubuntu-boot-repair.sh`. |
| `reset` | `Remove` | **true** (destructive) | Reverse topo order; `--cascade`/refuse-on-dependents; `--all` needs extra `--confirm`. |

**Best-effort error model (the key decision).** A **failing hook is NOT a Rust `Err`.** Per
`yazelix-setup.sh`'s `run()`, a nonzero exit becomes `OpStatus::Failed` in an `OpResult`,
recorded in `RunSummary.failed`, and the loop continues. `Engine::run(plan) -> anyhow::Result<RunSummary>`
returns `Err` **only** for setup-time problems — manifest parse failure, dependency cycle, unknown
component — surfaced via typed `EngineError` (`thiserror`). A hook spawn is wrapped in
`catch_unwind` so one bad component can never abort the whole run.

```rust
#[derive(thiserror::Error, Debug)]
pub enum EngineError {
    #[error("manifest parse error in {file}: {source}")] Manifest { file: String, #[source] source: toml::de::Error },
    #[error("dependency cycle involving '{0}'")]         DependencyCycle(String),
    #[error("unknown component id '{0}'")]               UnknownComponent(String),
    #[error("unknown dependency '{dep}' required by '{by}'")] UnknownDependency { by: String, dep: String },
}
```

---

## 6. Dependency resolution (deterministic topo order)

Each component declares `requires = ["<id>", …]`. The engine builds a DAG (edge dependent →
dependency) and computes a **deterministic** topological order via **Kahn's algorithm with ties
broken by manifest declaration order** — so the built-ins reproduce the wizard's exact proven
sequence and runs are reproducible. A cycle is a hard `EngineError::DependencyCycle` at load
(refuse-on-ambiguity, the boot-repair ethos applied to config).

Canonical built-in order (the wizard's order, encoded as `requires`):

```
meta-base-sanity (assert apt base)
 → nerd-fonts → bun → node-via-bun → [claude, codex, gemini, kimi, devin]
 → rustup → rtk
 → {GPU subgraph, gated lspci nvidia}:
      nvidia-cuda-repo → cuda-toolkit → nvidia-open → llvm-clang → cuda-env-block
      → rust-nightly-cuda-oxide → cuda-oxide → nvidia-container-toolkit
      → pytorch-venv → gpu-verify-scripts
 → gh → vite → wasmer → uv
 → nix → nix-yazelix-cache → home-manager → yazelix → yazelix-desktop
      → yazelix-config → yazelix-shell → ghostty-default-terminal
```

* **install / add-repo** traverse FORWARD; a component is attempted only after all its
  `requires` are present+verified. If a dep failed/missing, the dependent is
  `OpStatus::SkippedBlocked` — never run on a broken foundation (exactly why the wizard
  front-loads Nix before home-manager/yazelix).
* **auto-detect** uses forward order so a dependency's state is known before its dependents are
  classified.
* **auto-fix** repairs forward (fix a dependency before its dependent).
* **reset** traverses the SAME graph REVERSED, refusing to remove a depended-upon component
  unless its reverse-dependents are also in the set or `--cascade` is given.

**GPU gate is orthogonal to the graph.** `gpu_required` components carry a runtime predicate
(`lspci | grep -qiE nvidia`, resolved once per run into `RunContext`) that prunes them with
`OpStatus::Skipped("no NVIDIA GPU")` on a GPU-less box — matching the wizard's section-3b guard,
independent of dependency ordering.

---

## 7. auto-detect — the read-only EnvReport (and drift)

A single pure function `Engine::detect(&Registry) -> EnvReport` runs every component's `detect`
(and optional `verify`) plus a set of system probes, writing nothing, with hard per-probe
timeouts. Every probe failure becomes a non-fatal `ProbeWarning` (best-effort, the `fail[]`
pattern) — **absence is a normal `Option`/enum state, never an error.** The `EnvReport` is
serde-serializable so the CLI `--json`-streams it and the GUI renders it without re-probing.

```rust
pub struct EnvReport {
    pub generated_at: String, pub schema_version: u32,
    pub host: HostInfo,                  // os-release, kernel, uefi, secure_boot, cpu, mem, disks
    pub gpu: GpuReport,                  // NEVER None: always at least the PCI floor (expected_count=2)
    pub nvidia_stack: NvidiaStackReport, // driver / cuda toolkit / container toolkit + CDI / torch venv
    pub components: Vec<ComponentStatus>,// one per manifest component: detected, verify_passed, wiring[]
    pub drift: Vec<DriftItem>,           // diff(detected, manifest desired-state)
    pub preconditions: Vec<Precondition>,// SecureBoot=off, reboot-needed-for-driver
    pub warnings: Vec<ProbeWarning>,     // non-fatal probe failures
}
```

### 7.1 GPU detection — three-tier graceful-degradation cascade

Always yields a report, even on the software-rendered first boot with no driver:

* **Tier 0 — PCI floor (always first, never fails, no driver needed).** Walk
  `/sys/bus/pci/devices/*/{vendor,device,class}`, match vendor `0x10de` + class `0x0300/0x0302`;
  enrich via `lspci -mm -nn -d 10de:`. Sets the **authoritative GPU count** (==2 on this box) and
  device id `0x2b85` (GB202 → "Blackwell"/"sm_120" inferable pre-driver). Catches a GPU that
  fell off the bus (Xid) when a later tier sees fewer.
* **Tier 1 — NVML (preferred when driver live, optional `gpu-nvml` feature).** `Nvml::init()`
  gives `compute_capability()` directly as `(12,0)=sm_120`, plus pci/uuid/memory/temp/util/power
  and the CUDA driver version, with no fork/parse/locale fragility. `init()` is fallible by
  design → on a driverless boot it returns `Err`, which we treat as "not active" and fall
  through. No `unwrap`, polled off the UI thread.
* **Tier 2 — `nvidia-smi` CSV fallback (default build, zero extra deps).**
  `nvidia-smi --query-gpu=index,name,uuid,pci.bus_id,compute_cap,driver_version,memory.total,memory.used,temperature.gpu,utilization.gpu,power.draw --format=csv,noheader,nounits`
  with a hard timeout; non-zero/timeout ⇒ `driver_loaded=false`.

`driver_loaded` and `open_kernel_module` come independently from
`/proc/driver/nvidia/version` (the `NRVM version …` line + the `Open Kernel Module` marker), so
the driver-present signal doesn't depend on NVML or nvidia-smi succeeding.
`software_rendered = (PCI sees NVIDIA) AND NOT driver_loaded` drives the "reboot to load the
driver" precondition — mirroring the wizard's pre-reboot reality rather than reporting a false
failure.

### 7.2 Drift

Drift = `diff(detected EnvReport, manifest desired-state)`. A pure, unit-testable function emits
`DriftItem`s with a `suggested_verb` and a `severity` driven by the manifest's `version_lock`
flag + dependency criticality (so engine/CLI/GUI all rank identically):

* `Missing` → `install`; `VersionMismatch` (locked ⇒ Critical, unlocked ⇒ Warn — e.g. the live
  driver 595.71.05 vs desired 610 is **Warn** because the driver is intentionally unlocked per
  HANDOFF Decision #5); `WiringMissing` → `auto-fix` (re-wire only, non-destructive);
  `Unexpected` → Info (never auto-removed); `Unverified` → Warn (e.g. installed but reboot
  pending, the `reboot_gated_verify` case). Drift only *proposes* verbs — it never executes.

---

## 8. add-repo — build-from-source + wire-in (9-stage pipeline)

The extensibility verb: turn an arbitrary upstream repo into a first-class, idempotent,
removable component. All stages stream structured events; the build runs on the worker so the
GUI never stalls; everything is user-scope (no system dirs / no root on the common path);
best-effort with refuse-on-ambiguity.

1. **acquire** — `git clone --filter=blob:none` into `~/.local/share/envctl/repos/<slug>`
   (slug = sanitized `host_owner_repo`); existing repo → `git fetch && git reset --hard @{u}`.
   `--ref` pins; **record the resolved commit SHA** (resolve-then-record, the boot-repair
   pattern). Local paths are snapshot-copied, never mutated in place. Re-verify before
   proceeding; refuse on ambiguity (empty resolve, dirty tree on `--update`).
2. **detect build system** — ordered, most-hermetic-first: `nix flake > cargo > meson > cmake >
   autotools/make > python(uv) > bun/npm`. First match wins unless `--build-system` forces one;
   emit a structured `BuildPlan{system, root, build_cmd, artifact_glob}`.
3. **resolve build deps (best-effort)** — probe with `command -v`; provision missing ones from
   the known kit (rust via `. ~/.cargo/env`, nix via the daemon profile, bun/uv/cmake/meson).
   Never hard-fail; record + mark degraded. System `-dev` headers only with `--with-deps`.
4. **build** — run `build_cmd` in a clean logged subprocess with the kit's
   CUDA/LLVM/PATH env sourced first (so GPU-aware source builds needing `nvcc`/`clang-21`/
   `CUDA_OXIDE_LLC` succeed). SHA+flags-keyed cache skips unchanged rebuilds. Streams stdout/stderr
   as events; honors `--jobs`.
5. **locate artifacts** — resolve the glob, classify (executables, `.so`, completions,
   `.desktop`/icons, man, units), pick the install set. Refuse if zero executables and none
   specified.
6. **install** — into XDG user roots: bins → `~/.local/bin`, libs → `~/.local/lib`, completions
   → the shell's XDG dir, `.desktop` → `~/.local/share/applications`, man → `~/.local/share/man`.
   **Symlink-by-default** (clean update/reset), `--copy` to materialize; always
   timestamped-backup before clobber (the `yazelix-config.sh .bak.$(date)` pattern); refuse to
   overwrite an unmanaged file without `--force`.
7. **wire-in** — apply the component's `Wiring`: ensure `~/.local/bin` on PATH via a guarded
   `# >>> BEGIN envctl path >>>` block; install completions; XDG desktop entry; for
   `daemon`-flagged components write + `systemctl --user enable --now` a unit. All guarded,
   idempotent; dry-run prints without applying.
8. **register as component** — synthesize a TOML fragment (provenance: url, resolved SHA, ref,
   build_system, flags, fetched_at; the five hooks; the wiring) and atomically write it to
   `~/.config/envctl/components.d/<slug>.toml` (temp + rename, timestamped-backup on collision).
9. **verify** — run the component's verify hook; a failure marks it degraded (eligible for
   `auto-fix`), it does not delete the registration.

From that point the five verbs manage the repo like any built-in. The `Detect` hook records the
SHA so it distinguishes *present-and-current* from *present-but-stale* (→ `auto-fix` rebuilds).

---

## 9. Streaming event model

The engine **never prints**. It emits `Event`s into a channel. We use `std::sync::mpsc`
(unbounded — lossless under bursty apt/nix output, single-consumer, **dependency-free**); the
doc-comment notes `crossbeam-channel` as a one-type drop-in if multi-consumer/`select` is ever
needed.

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    RunStarted   { phase: Phase, total: usize, dry_run: bool },
    StepStarted  { component: String, phase: Phase, index: usize, total: usize },
    Log          { component: String, stream: Stream, line: String },   // one streamed stdout/stderr line
    StepFinished { result: OpResult },                                  // the c_ok/c_warn line
    Telemetry(Telemetry),                                               // periodic GPU/CPU/mem snapshot
    GuardRefused { component: String, reason: String },                 // surfaced prominently (red)
    RunFinished  { summary: RunSummary },                              // the CLI summary / GUI health pill
}
pub enum Stream { Stdout, Stderr }

#[derive(Clone)]
pub struct EventSink(std::sync::mpsc::Sender<Event>);
impl EventSink {
    pub fn new() -> (EventSink, std::sync::mpsc::Receiver<Event>) { let (tx,rx)=std::sync::mpsc::channel(); (EventSink(tx),rx) }
    pub fn emit(&self, ev: Event) { let _ = self.0.send(ev); } // infallible from the engine's view: drop if receiver hung up
}
```

* **CLI** drains on the main thread and pretty-prints with the wizard's `c_ok/c_warn/c_step`
  color vocabulary; `--json` streams events as NDJSON for scripting.
* **GUI** (next section) drains via `try_recv` in `update()`; the worker holds an
  `egui::Context` clone and calls `request_repaint()` after each event so the UI wakes
  immediately (~0% CPU at rest).

The same line stream is **tee'd to disk** by the engine at
`~/.local/state/envctl/envctl.log` — the direct analogue of `~/yazelix-setup.log` — so a
crash or closed window never loses the record.

---

## 10. GUI (eframe / egui — immediate mode, native, no web)

**Why egui over iced.** This is a personal single-box dashboard whose whole job is to render
rapidly-changing live state (GPU/CPU/mem every ~1s + a streaming log console). Immediate mode
means "the UI *is* a function of state" — each frame reads the latest snapshot and draws it; no
widget-tree diffing, no Message/update/view choreography, no risk of the view drifting from
engine state. Driving repaints from a worker via `ctx.request_repaint()` is idiomatic and needs
no Subscription machinery to bridge an mpsc channel. egui's built-in widgets cover 100% of the
need with zero extra deps (`Grid`/`TableBuilder` for the components table, `ScrollArea`
`.stick_to_bottom` for logs, `ProgressBar` + painter sparklines for telemetry,
`CollapsingHeader` for the manifest viewer). It matches the kit's spirit: small, mainstream, few
deps, stable Rust, nothing web/WebView. The only non-engine GUI deps are `eframe`/`egui`
(+`egui_extras`).

### 10.1 Threading model — the UI thread NEVER blocks

At startup the `App` spawns **one** long-lived `std::thread` (a single worker processing
commands **serially**, so two destructive ops — sudo/apt/nix mutating system state — can never
race) and creates two channels:

* **Command channel** `Sender<EngineCommand>` (App → worker): `Detect`, `VerifyAll`,
  `InstallComponent(id)`, `FixComponent(id)`, `RemoveComponent(id){confirmed}`,
  `AddRepo(spec){dry_run}`, `Reset{confirmed}`, `AutoFix{dry_run}`, `SampleTelemetry`,
  `Shutdown`.
* **Event channel** `Receiver<Event>` (worker → App), drained by `try_recv` at the top of every
  `update()`.

The worker gets a cheap (`Arc`-backed) `egui::Context` clone captured at spawn; after sending
ANY event it calls `ctx.request_repaint()`. Per-component **busy** state is a
`HashSet<ComponentId>` in the App (set on dispatch, cleared on the terminal `StepFinished`/result)
so a row shows a `Spinner` and disables its buttons with **no Mutex on the hot path** — the App
owns its state, the worker owns the engine, they communicate only by message-passing +
`request_repaint`. Telemetry runs on its own ~1s cadence (a second lightweight sampler), so a
10-minute CUDA build never starves the GPU gauges.

### 10.2 Screens

| Screen | Purpose |
|---|---|
| **Dashboard** | Landing page. Global health pill (Healthy / N degraded / M failed) from the last detect+verify, the `fail[]` roster from the last action, and a row of dual-RTX-5090 cards (name/driver/temp/util%/VRAM) + CPU + memory strips with painter-drawn sparklines (~120-sample ring). Pre-reboot shows `DriverNotActive` as a **yellow "REBOOT to load nvidia-open 610" card**, not an error. |
| **Components** | The operational grid (`TableBuilder`): Name \| Category \| State (colored dot) \| Version \| Wiring badges \| Actions. Per-row Install/Fix/Remove/Verify dispatch `EngineCommand`s; busy rows spin + disable. Search + category filter. Destructive Remove opens a confirmation modal naming exactly what will be unwired/backed up. |
| **Add Repo** | Form for the add-repo verb (URL + ref, build-kind combo, command overrides, wiring checkboxes, **Dry-run toggle default ON**). "Validate" runs detect-only; "Build & wire" dispatches to the worker and switches focus to Live Logs. |
| **Live Logs** | `ScrollArea::stick_to_bottom` over a bounded `VecDeque<LogEvent>` ring (cap ~8000, oldest dropped). Per-line `Color32` by level (the `c_ok/c_warn/c_step` vocabulary), level/component filters, pause-autoscroll, clear, copy/save. Monospace; sticky active-step header. Same stream is tee'd to disk. |
| **Settings / Manifest** | Read-mostly viewer: each component's five hooks (read-only monospace) + wiring badges; global options — telemetry interval slider, log-cap `DragValue`, **"destructive ops dry-run by default" checkbox (ON)**, "require confirmation for Remove/Reset/AutoFix". "Reload manifest from disk". The manifest is the source of truth; deep edits happen in `$EDITOR`. |

---

## 11. Telemetry

Source mirrors the kit: shell out to
`nvidia-smi --query-gpu=index,name,driver_version,temperature.gpu,utilization.gpu,memory.used,memory.total --format=csv,noheader,nounits`,
one `GpuSample` per GPU (handles the dual 5090 by index). Pre-reboot, nvidia-smi is
absent/failing → reported as `GpuState::DriverNotActive` (the yellow card), never a panic. When
the optional `gpu-nvml` feature is built and the driver is live, telemetry upgrades to typed
NVML. CPU+mem come from `sysinfo` (or `/proc/stat` + `/proc/meminfo` directly to stay lean). The
sampler emits `Event::Telemetry` every ~1s while the Dashboard is active, backing off to ~3-5s
on other screens and pausing when the window is unfocused — so it never needlessly spawns
nvidia-smi.

---

## 12. Safety model (destructive ops follow ubuntu-boot-repair.sh)

The three dangerous surfaces — **reset**, **auto-fix**, and the wrapped **boot-repair**
subcommands — inherit the boot script's discipline verbatim:

1. **Dry-run by default.** `reset`/`auto-fix` default `dry_run=true`; mutation requires explicit
   `--apply`. `reset --all` (whole-environment baseline) requires an additional `--confirm`.
   `install` keeps the opposite default (additive ⇒ acts), matching the wizard.
2. **Resolve + re-verify before acting.** A `RunContext` resolves the live-device identities and
   GPU presence **once per run** (avoiding TOCTOU drift). Guards then resolve targets by UUID and
   **re-verify** the device actually carries that UUID before any mount/touch — the
   `resolve_verified()` chain. Files are resolved by exact path + marker/ownership check.
3. **Refuse on ambiguity.** A failing guard yields `OpStatus::Refused` (a safe abort + a red
   `GuardRefused` event), never a `Failed`/panic. Reset refuses to strand a dependent. There is
   **no `--force` that bypasses the resolve/re-verify/refuse guards** — those are unconditional.
4. **Back up before clobber.** Every user-file edit is resolve → re-verify → timestamped backup
   (`.bak.$(date +%s)`) → apply. `revert()` excises **only** the owned marker block, so unrelated
   `~/.bashrc`/config content is never lost.
5. **Never touch user DATA.** Like the boot script never mounting `/home`, `reset`/`auto-fix`
   never purge data dirs unless `--purge` is explicit (and even then re-verify the path). Disks
   are flagged `is_data_home` by UUID so destructive verbs skip them.
6. **Host boot/driver repair is opt-in and delegated.** `auto-fix` does not improvise GRUB/ESP/
   driver fixes; it surfaces the exact `ubuntu-boot-repair.sh diagnose|repair-dev|rename-pro|
   finalize` subcommand and only invokes it under `--allow-boot`, inheriting that hardened
   script's UUID resolve+re-verify, same-disk, never-touch-`/home`, NVRAM-verify guards
   unchanged. Its machine-specific UUIDs/bl-ids live in the manifest `machine` map, never as
   engine literals.
7. **sudo only where the wizard used it** (apt, nix-daemon restart, update-alternatives, the
   boot subcommands), each step guarded; the common path is user-scope.

---

## 13. How every decision is grounded in the existing kit

* `yazelix-setup.sh:27-32` `run()` `fail+=()` → the best-effort error model: failing hook =
  `OpStatus::Failed`, recorded, run continues; `Engine::run` returns `Err` only for
  manifest/cycle problems; final `RunSummary` = the `fail[]` roster.
* `yazelix-setup.sh` guarded `# >>> BEGIN cuda env >>>` (139-148) and
  `# >>> BEGIN yazelix auto-enter >>>` (392-410) blocks, gated by `grep -q`, + the one-shot
  self-disabling gpu-verify `.desktop` (309-318) → the declarative `Wiring` type
  (`shell_rc` markers, `desktop_entries.one_shot`, guarded `path_entries`).
* `ubuntu-boot-repair.sh:72-126` `resolve_verified()`/`mount_dev_root()` (resolve by UUID,
  re-verify, refuse if live==dev or different disks, never umount `/home`, `cp -a … .bak`, verify
  NVRAM after) → the `Guard` enum + the §12 safety model; its hardcoded UUIDs/bl-ids → manifest
  `machine` config.
* `yazelix-config.sh:243-249` timestamped-backup-then-restore (`--force`) → the
  back-up-before-clobber idiom for add-repo install + component config reset.
* `lspci | grep -qiE nvidia` (setup `116`) → `gpu_required` + the `RunContext` GPU gate
  (`Skipped`, not `Failed`, on a non-GPU box).
* Proven bash WRAPPED not rewritten → `Hook::Script` holds wizard fragments verbatim
  (`bash -lc`); `Hook::ShippedScript` references whole scripts (yazelix-gpu-verify.sh,
  yazelix-config.sh, the four boot-repair subcommands) the engine never edits.
* The wizard's "REBOOT for the driver to load" reality (184-188, 246-248) → `reboot_gated_verify`
  + `OpStatus::RebootRequired` (not `Failed`) + the yellow Dashboard card.

---

## 14. Crate dependency summary (stable Rust, mainstream, few)

```
envctl-engine = { serde, toml, anyhow, thiserror, sysinfo, which, chrono,
                  optional: nvml-wrapper (feature "gpu-nvml"), serde_json (lsblk -J parse) }
envctl        = { envctl-engine, clap, anyhow }
envctl-gui    = { envctl-engine, eframe, egui, egui_extras }
```

No web, no WebView, nothing nightly. The CLI and GUI are two thin front-ends over one engine, so
behavior cannot diverge.

---

## §8b — add-repo pipeline (Phase 4, implemented)

Module map: `detect_build.rs` (signal→build-cmd→artifact-glob table, re-runnable
after a transform so an AI port re-detects as cargo) · `addrepo.rs` (the staged
pipeline: euid-0 refusal, `allow_build` opt-in gate, acquire into a 0700 store with
origin-verify-on-reuse + hardened git, the patch/AI transform with a
structurally-confined agent, streamed build in its own process group with a timeout,
glob locate, strategy shaping) · `install.rs` (symlink into `~/.local/bin`,
PATH-shadow refusal, canonical managed-symlink ownership, refuse-unmanaged-unless-force,
PATH wire-in via the existing `Wiring`) · `register.rs` (synthesize the provenance
drop-in with a SHA-pinned rebuild + owned-symlink remove, written through the
executor's atomic temp+rename+backup writer). Streaming reuses `Event::Log` /
`StepFinished{phase: Install}` so CLI + GUI render identically.

**§11b — Telemetry thread (Phase 5):** a dedicated `TelemetryControl` sampler thread
(peer to the event forwarder, `Send + 'static`) emits `Event::Telemetry` on a
GUI-controlled cadence; a long `engine.run` no longer starves telemetry.
