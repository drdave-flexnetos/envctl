---
name: env-toolchain-install
description: "How to install and configure the developer toolchain the way envctl does it — declaratively, idempotently, with detect→install→verify→fix→remove lifecycle hooks per component. Use whenever installing, repairing, or configuring environment tooling (Rust, bun/node, CUDA/GPU stack, ai-clis, nix-yazelix, boot-repair, the secretd daemon) or authoring a new component. Triggers: 'install the toolchain', 'set up the environment', 'add a component', 'why is X not on PATH', 'repair the environment', 'the install isn't idempotent'."
---

# Environment Toolchain Install (envctl-grounded)

The reference for "a properly built environment" is **envctl**'s declarative component model (`/home/drdave/Desktop/envctl/manifest/*.toml`). Each tool is a TOML **component** whose lifecycle hooks wrap proven shell rather than rewriting it. Mirror this discipline whenever you touch the toolchain — never hand-run an ad-hoc `curl | bash` outside the model, because that leaves the environment undeclared and unrepairable.

## The Component Contract

Every component declares an idempotent lifecycle. From `manifest/base.toml`:

```toml
[[component]]
id = "bun"
name = "Bun JS runtime"
description = "JS runtime + package manager; provides node via a bun symlink."
requires = ["rustup"]              # ordering / dependency edges
[component.detect]   kind = "command"  # is it already installed? (cheap, no side effects)
[component.install]  kind = "script"   # idempotent install
[component.verify]   kind = "command"  # post-install proof it works
[component.fix]      kind = "script"   # repair a broken-but-present install
[component.remove]   kind = "command"  # clean uninstall
[component.wiring]   path_entries = ["~/.bun/bin"]   # PATH + shell_rc markers
```

**Why each hook exists:**
- **detect** runs first and must be side-effect-free and PATH-robust. Login shells (`bash -lc`) don't source `~/.bashrc`, so detect by checking the binary path directly (`[ -x "$HOME/.bun/bin/bun" ]`), not just `command -v`.
- **install** must be safe to run twice. Re-running install on an installed component is a no-op or a harmless refresh.
- **verify** is separate from detect: detect answers "is it here?", verify answers "does it actually work?" (e.g. `bun --version`). A component that detects-present but fails verify is **broken**, not installed.
- **fix** repairs the present-but-broken state without a full remove/reinstall.
- **remove** is the reset path; it must leave no PATH/shell_rc residue.
- **wiring** records PATH entries and `shell_rc` markers so the environment's PATH is *declared*, not accreted.

## The Reference Component Set

The proper environment is the union of envctl's manifest files (`manifest/*.toml`, pinned by `envctl.lock`):

| Manifest | Provides |
|----------|----------|
| `base.toml` | nerd-fonts, bun, node-via-bun, rustup, rtk |
| `apt-base.toml` | system build packages |
| `dev-tools.toml` | cargo tooling, clippy, fmt |
| `gpu.toml` | CUDA / cuDNN / TensorRT (GPU-aware; dual-RTX-5090 target) |
| `ai-clis.toml` | claude-cli, codex, etc. |
| `nix-yazelix.toml` | nix for isolated shells |
| `boot-repair.toml` | UEFI / Secure-Boot repair |
| `env-ctl.toml` | secretd / secretctl daemon + systemd `--user` unit |

## Working Rules

- **Author, don't improvise.** Need a tool installed? Add or invoke a component, don't run a bare install command. The environment must stay fully described by the manifest.
- **Order by `requires`.** `node-via-bun` requires `bun`; `rtk` requires `rustup`. Respect the dependency edges — envctl resolves install order from them.
- **Idempotency is the test.** Before considering a component done: run install twice, then verify. A second install that errors or a verify that fails means the component is wrong.
- **PATH is declared via wiring.** When a tool installs to a new bin dir, add it to `[component.wiring] path_entries` + a `shell_rc` marker — don't tell the user to "just add it to your PATH."
- **Secrets are out of scope here.** Credential/secret handling belongs to the secretd/secretctl stack (env-ctl), not the install components. Don't bake tokens into install scripts; reference the broker.

## Verify Your Work

After installing/repairing: run the component's `verify` hook (or its equivalent command), confirm PATH wiring is present in the shell rc, and confirm a fresh login shell sees the tool. The whole environment is reproducible iff every component detects-present and verifies-green from a clean shell.
