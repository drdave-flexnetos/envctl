# envctl — HANDOFF / verification guide

Paste this whole file as the first message of a new session to pick up + verify.

## What envctl is
A pure-Rust, GPU-aware, **source-building environment manager** for this live
dual-RTX-5090 Ubuntu 26.04 box. One cargo workspace at `/home/drdave/Desktop/envctl`:
`crates/engine` (lib `envctl_engine`) + `crates/cli` (`envctl`) + `crates/gui`
(`envctl-gui`, native egui — no web). It manages the box declaratively: every tool
is a TOML **component** whose hooks wrap the proven Desktop bash kit. **All six
roadmap phases are complete + dogfooded**, plus graph intelligence and an
interactive add-repo "connect" mode.

Verbs: `auto-detect · install · reset · auto-fix · add-repo · graph`.

## Status (commits on `master`)
```
Phase 4+5 add-repo+telemetry · Phase 3 reset/auto-fix · GUI theme · PRD ·
Phase 2 streaming install · review fixes · Phase 0+1 scaffold+manifest+drift
```
Plus (this session): **graph intelligence** (`graph.rs` + `envctl graph`),
**interactive connect** (`Engine::connect_repo` + `add-repo --connect`), and
`docs/KASETTO-FEATURES.md` (a 588-line feature catalog extracted from the kasetto
project, with ranked ADOPT recommendations — top: a committed lock file with
content hashing).

## Live-box facts
- `rustc`/`cargo` 1.96 installed (rustup). `claude` is the only AI CLI present
  (codex/gemini/kimi absent) → the add-repo AI/connect strategies use it.
- `sudo` is **interactive** (needs a password); the in-session `!`/tool shell has
  no TTY, so `sudo apt` must run in a real terminal (or `echo PW | sudo -S …`).
- Live Wayland session (`DISPLAY=:0`, `WAYLAND_DISPLAY=wayland-0`); the GUI runs.
- GUI build deps already installed (cmake/libxkbcommon/wayland/xcb/gl/gtk-3 -dev).

## Verify everything (run from `/home/drdave/Desktop/envctl`)

```bash
. "$HOME/.cargo/env"
export ENVCTL_MANIFEST_DIR="$PWD/manifest"

# 1. builds + tests (expect: clean; ~20 engine tests pass)
cargo build --workspace
cargo test -p envctl-engine

# 2. auto-detect — read-only; should see 2x RTX 5090 (PCI floor), driver,
#    Threadripper, 44 components with accurate drift
cargo run -p envctl -- auto-detect
cargo run -p envctl -- auto-detect --json | head

# 3. graph intelligence — summary / impact / why / DOT / JSON
cargo run -p envctl -- graph
cargo run -p envctl -- graph --impact bun       # blast radius (cascade removes 5)
cargo run -p envctl -- graph --why cuda-oxide   # root->id dependency paths
cargo run -p envctl -- graph --dot | dot -Tsvg -o /tmp/envctl-graph.svg  # if graphviz
cargo run -p envctl -- graph --json --live | head

# 4. install — idempotent, streaming, wiring. SAFE reversible dogfood:
cargo run -p envctl -- install bun --dry-run    # preview
cargo run -p envctl -- install bun              # real (curl|bash to ~/.bun, PATH block)
cargo run -p envctl -- install bun              # idempotent: skips

# 5. reset gates — DRY-RUN by default; reverse-dep guard fires
cargo run -p envctl -- reset                    # exit 2: refuses untargeted w/o --all --confirm
cargo run -p envctl -- reset bun                # REFUSED: live reverse-dependent node-via-bun
cargo run -p envctl -- reset bun --cascade      # dry-run preview: folds node-via-bun first
cargo run -p envctl -- reset bun --cascade --confirm --apply   # real (reversible: reinstall)

# 6. add-repo — PREVIEW by default; --build to act
#    AI port preview (no clone/agent/build, shows the confined claude argv):
cargo run -p envctl -- add-repo https://github.com/sharkdp/pastel --id pastel-rs \
  --strategy refactor --ai-goal port-to-rust --dry-run
#    real build-from-source of a tiny cargo tool (use a temp manifest to avoid
#    polluting ./manifest/components.d):
M=/tmp/m; rm -rf $M; cp -r manifest $M
ENVCTL_MANIFEST_DIR=$M cargo run -p envctl -- add-repo \
  https://github.com/sharkdp/pastel --id pastel --build
ENVCTL_MANIFEST_DIR=$M cargo run -p envctl -- auto-detect | grep pastel   # now managed
ENVCTL_MANIFEST_DIR=$M cargo run -p envctl -- reset pastel --apply        # clean remove

# 7. interactive connect (needs a real terminal — drops you into `claude`):
cargo run -p envctl -- add-repo https://github.com/owner/repo --id thing \
  --strategy refactor --ai-goal port-to-rust --connect

# 8. GUI (native; opens on the Wayland session)
ENVCTL_MANIFEST_DIR="$PWD/manifest" DISPLAY=:0 WAYLAND_DISPLAY=wayland-0 \
  XDG_RUNTIME_DIR=/run/user/1000 cargo run -p envctl-gui
```

## Safety invariants to confirm still hold
- Destructive verbs (`reset`/`auto-fix`) are dry-run unless `--apply`.
- Guards fail closed (`UuidResolves`/`NotLiveDevice`/`NotMounted` via blkid/findmnt;
  a bogus UUID → Refused). `add-repo` refuses as root, gates real work behind
  `--build`, sandboxes the clone 0700, confines the AI agent (`--add-dir`/
  `--permission-mode`, never `--yolo`), and never auto-commits/pushes.
- `reset` excises **only envctl-owned** wiring blocks; foreign edits are reported.
- A half-failed run is `Incomplete` → CLI exits nonzero (never green on failure).

## Docs
`docs/ARCHITECTURE.md` · `docs/ROADMAP.md` · `docs/PRD.md` · `docs/DESIGN-NOTES.md`
· `docs/ADD-REPO.md` · `docs/KASETTO-FEATURES.md` · this `HANDOFF.md`.

## Open / next (optional polish, not blocking)
Adopt from kasetto (see `docs/KASETTO-FEATURES.md`): a committed **lock file** with
content hashing (top pick); `--locked`/`--update` sync modes; a multi-host source
resolver to harden `add-repo` beyond GitHub; universal `--json` on every verb; a
`doctor`-style diagnostic. Also: a GUI graph tab; shell-completions install;
systemd `--user` daemon units; live add-repo build streaming into the GUI.
