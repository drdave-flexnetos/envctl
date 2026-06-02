# envctl

A personal, **GPU-aware, source-building environment manager** for this dual-RTX-5090
Ubuntu 26.04 workstation. One Rust workspace: a shared engine, a CLI (`envctl`), and a
native egui desktop app (`envctl-gui`). It manages the box declaratively — every tool is
a TOML **component** whose lifecycle hooks *wrap the proven bash* from the Desktop kit
(`yazelix-setup.sh`, `ubuntu-boot-repair.sh`, …) rather than rewriting it.

## Verbs

| verb | what it does | default |
|---|---|---|
| `auto-detect` | read-only inventory: host, GPU (works pre-driver), tools, component drift | — |
| `install` | bring components to present+verified, in dependency order; **idempotent** | acts |
| `auto-fix` | repair broken/partial components | **dry-run** (`--apply`) |
| `reset` | uninstall + unwire back toward baseline; gates `--all/--confirm/--cascade/--purge` | **dry-run** (`--apply`) |
| `add-repo` | build any repo from source (as-is / cherry-pick / rename / **AI port-to-Rust**) + wire-in; `--connect` for a supervised agent session | **preview** (`--build`) |
| `graph` | dependency-DAG intelligence: summary, `--impact` blast-radius, `--why` paths, `--dot`/`--json` | — |
| `lock` | content-hashed `envctl.lock` (reproducible) + `--check` CI gate (exit 1 on drift) | writes |
| `doctor` | read-only health: writability, toolchains, sudo, UEFI/Secure-Boot, GPU, last-op | — |

## Quick start

```bash
# Rust is required (this repo pins stable via rust-toolchain.toml):
#   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && . "$HOME/.cargo/env"

cargo build -p envctl-engine -p envctl     # engine + CLI (zero system deps)
cargo run  -p envctl -- auto-detect        # read-only; safe to run anytime
cargo run  -p envctl -- auto-detect --json # machine-readable EnvReport
cargo run  -p envctl -- install bun --dry-run
cargo run  -p envctl -- reset boot-repair-dev      # dry-run by default
```

The manifest dir defaults to `./manifest` (override with `ENVCTL_MANIFEST_DIR`).

### Native GUI

The `envctl-gui` crate needs system dev libs (winit/glow + a native file dialog):

```bash
sudo apt-get install -y cmake libxkbcommon-dev libwayland-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libgl1-mesa-dev libgtk-3-dev
cargo run -p envctl-gui
```

Dashboard (live GPU/CPU/mem telemetry) · Components grid (install/fix per row) ·
Add-Repo form · Live Logs · Settings. The engine runs on a worker thread; the UI never blocks.

## Status

**Phase 0 + a working `auto-detect`.** The workspace compiles green on stable Rust; the
read-only verb is fully implemented and validated on the live dual-5090 box (PCI-floor GPU
detection that works even before the driver loads). `install`/`reset`/`auto-fix`/`add-repo`
are wired end-to-end with the real safety machinery (fail-closed guards, dry-run defaults,
idempotent install, hardened add-repo), with their deeper behavior staged in
[`docs/ROADMAP.md`](docs/ROADMAP.md).

## Safety model (boot-repair discipline)

Destructive operations follow `ubuntu-boot-repair.sh`'s gold standard:
**resolve + re-verify, refuse on ambiguity, dry-run by default, back up before clobber,
never touch user data.** Guards (`UuidResolves` / `NotLiveDevice` / `NotMounted`) are
implemented **fail-closed** — when they can't prove an op is safe, they *refuse* (a unit
test enforces this). See [`docs/DESIGN-NOTES.md`](docs/DESIGN-NOTES.md).

## Layout

```
crates/engine/   # envctl_engine: Component model, Registry, the 5 verbs, detect, guards, GUI worker API
crates/cli/      # envctl
crates/gui/      # envctl-gui (eframe/egui)
manifest/        # declarative components (base.toml, cuda.toml, boot-repair.toml) + components.d/ drop-ins
assets/scripts/  # the proven Desktop kit, referenced verbatim by ShippedScript hooks
docs/            # ARCHITECTURE.md · ROADMAP.md · DESIGN-NOTES.md
```

Design produced by a multi-agent design swarm and adversarially reviewed; the applied
review fixes are listed in `docs/DESIGN-NOTES.md`.
