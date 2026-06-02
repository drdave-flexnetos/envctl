# envctl `add-repo` — build any repo from source and wire it in

`add-repo` turns an upstream git repo (or local working tree) into a **first-class
managed component**: it clones, optionally transforms, detects the build system,
builds from source, installs the artifacts, wires them onto `PATH`, and registers a
drop-in so `auto-detect` / `install` / `reset` / `auto-fix` manage it from then on.

```
acquire → [strategy transform] → detect → build (streamed) → locate
        → install → wire-in → register → verify
```

## Safety model (read this first)

add-repo runs **untrusted upstream code**, so it is gated:

- **Acquire + detect + PREVIEW by default.** A bare `add-repo` does *not* build,
  run a refactor agent, or install. You must pass **`--build`** to actually run the
  upstream build / AI agent / install. (`--dry-run` previews without even cloning.)
- **Never as root.** The pipeline refuses (`Refused`) if `euid == 0`.
- **0700 sandbox.** Clone + build live under `~/.local/share/envctl/repos/<id>/`,
  created `0700`. The build/agent run in their own **process group**, killed
  wholesale on timeout (acquire 10m, refactor/build 1h).
- **Install never hijacks a name.** It refuses to symlink an artifact whose name
  already resolves on `PATH` outside `~/.local/bin`, and hard-refuses well-known
  names (`sudo`, `git`, `bash`, `cargo`, …) — use `--rename` instead.
- **Refuse-overwrite-unmanaged.** A target is "ours" only if it's a symlink whose
  canonical path is inside the repo store. A real foreign file is **refused** unless
  `--force` (which backs it up `*.bak.<epoch>` first).
- **The AI agent is structurally confined.** Invoked non-interactively, TTY-less,
  in the clone dir (`claude --add-dir <clone> --permission-mode acceptEdits`, never
  `--dangerously-skip-permissions`/`--yolo`), bounded by a timeout, and it **never
  auto-commits or pushes**. Rebuild replays the recorded SHA, never re-drives the agent.

## Strategies

| strategy | what it does | flags |
|---|---|---|
| `as-is` (default) | build the repo unchanged (nvidia/mise/pytorch-style) | — |
| `cherry-pick` | install only a subset of built binaries (by file-stem) | `--bin foo` (repeatable) |
| `rename` | install artifacts under new names for synergy | `--rename old=new` (repeatable) |
| `refactor` | transform the clone before building | `--patch-cmd …` **or** `--ai-goal …` |

`refactor` has two flavors:
- **patch** — `--patch-cmd '<shell transform>'` runs in the clone before detect+build.
- **ai** — drives an available AI CLI (claude → codex → gemini → kimi, or
  `--ai-agent`) with a structured prompt: `--ai-goal port-to-rust |
  cherry-pick-to-crate | rename-for-synergy | custom` (+ optional `--ai-instruction`).
  After the agent runs, the pipeline **re-detects** the (often now-cargo) tree and
  builds it. This is how you take a C/Go repo and ship a Rust port.

## Build-system detection

Top-down by signal file (cargo first so a Rust port wins; flake last):

| system | signal | default build | artifacts |
|---|---|---|---|
| cargo | `Cargo.toml` | `cargo build --release` | `target/release/*` |
| cmake | `CMakeLists.txt` | `cmake -S . -B build … && cmake --build build` | `build/*`, `build/bin/*` |
| meson | `meson.build` | `meson setup build … && meson compile -C build` | `build/*` |
| autotools | `configure.ac`/`Makefile.am` | `./configure && make -j` | `src/*`, `*` |
| make | `Makefile` | `make -j` | `*`, `bin/*`, `build/*` |
| go | `go.mod` | `go build ./...` | `*` |
| zig | `build.zig` | `zig build -Doptimize=ReleaseSafe` | `zig-out/bin/*` |
| node | `package.json` | bun/npm install + build | `dist/*`, `bin/*`, … |
| python | `pyproject.toml` | `uv build` / pip | `dist/*`, `.venv/bin/*` |
| nix flake | `flake.nix` | `nix build …` | `result/bin/*` |

Override with `--build-system <name>`, `--build-cmd '<cmd>'`, `--artifact '<glob>'` (repeatable).

## The registered drop-in

On a successful `--build`, envctl writes `components.d/<id>.toml` with:
- a **provenance** header (strategy / source / ref / resolved SHA / build system /
  build cmd / transform / artifacts);
- `detect` = `command -v <primary-bin>`;
- `install` = **rebuild-from-source pinned to the SHA** (re-clone → checkout → replay
  patch → build → relink) so a fresh box reproduces it;
- `verify` = `<bin> --version`;
- `remove` = excise **only our** symlinks (readlink-into-store guard) then drop the clone;
- `[component.wiring] path_entries = ["~/.local/bin"]` so `reset` unwinds `PATH`.

It's marked build-from-source: an *untargeted* `install`/`auto-fix` won't re-run the
upstream build — you must name it.

## Worked examples

```bash
# preview an AI port-to-Rust (no clone, no agent, no build):
envctl add-repo https://github.com/sharkdp/pastel --id pastel-rs \
  --strategy refactor --ai-goal port-to-rust --dry-run

# build a small cargo tool from source and register it:
envctl add-repo https://github.com/sharkdp/pastel --id pastel --build

# cherry-pick one binary, renamed, from a multi-bin repo:
envctl add-repo https://github.com/BurntSushi/ripgrep --id rg --strategy rename \
  --rename rg=rgx --build

# build a local working tree:
envctl add-repo ./my-tool --id my-tool --local ./my-tool --build

# then it's a managed component:
envctl auto-detect          # shows it
envctl reset pastel --apply # removes symlinks + drops the clone
```

## GUI

The **Add Repo** screen exposes URL / id / ref / build-cmd, a **strategy** picker
with strategy-specific fields (cherry-pick bins, rename map, refactor patch/AI-goal),
and a **"Build now"** toggle (off = preview). The AI flavor shows a banner that the
agent runs non-interactively and never auto-commits. Output streams live into the
**Logs** tab.
