# 01 — Architect Plan: meta Mission-Control Dashboard (yazelix/zellij)

VERDICT: GO

> Persisted by the Feature Forge orchestrator from the read-only `feature-architect` (`Plan` type) return value.

## User decisions driving this plan
1. **Placement:** BOTH `envctl` AND `meta` surfaces must be true, with NO drift (`.meta.yaml` stays the single source of truth).
2. **Layout:** tabs grouped by `.meta.yaml` tag, pane-per-repo, + fixed overview tab. Scale to ~40 repos.
3. **Tile behavior:** auto-launch IDLE `claude` agent sessions per repo (NOT autonomous loops on open; escalatable later).
4. **Mail bus:** use BOTH weave AND repowire; north-star = unify weave+repowire+broker (broker = follow-on, not v1).

---

## Resolution (RECOMMENDED): engine owns logic; two thin surfaces, one delegates
- **(a) Logic in `envctl-engine`** — new read-only `dashboard` method emitting `Event`s; `envctl dashboard` CLI verb + GUI parity. Engine stays pure-Rust, sync, non-printing.
- **(b) `meta` surface = NEW subprocess plugin** `meta-dashboard` (`meta_dashboard_cli`, meta_plugin_protocol) that **shells to `envctl dashboard --json`**. No crate dep between meta_cli and envctl → no cycle.
- **No-drift guarantee:** generator READS `.meta.yaml` at runtime (never hardcodes repos); both surfaces hit the same engine code path, so they cannot diverge.
- Rejected: shared crate (cross-repo crate edge + version lockstep); direct crate dep (cycle/coupling).

## Config-tool layering map (the "right option" verification)
**Tabs/panes/windows = zellij KDL layout — NOT settings.jsonc, NOT ghostty/starship.**

| Concern | Owner | Where |
|---|---|---|
| Tabs / panes / windows / per-pane command + cwd | **zellij (KDL layout)** | `~/.config/yazelix/configs/zellij/layouts/<name>.kdl` ← **what we generate** |
| Sidebar, default_shell, terminals, theme/tips, keymap remaps | yazelix | `~/.config/yazelix/settings.jsonc` (high-level only) |
| Native zellij overrides | zellij | `~/.config/yazelix/zellij.kdl` |
| Terminal window/font | ghostty | ghostty config (out of scope) |
| Prompt | starship | starship config (out of scope) |
| Default shell in panes | nushell via yazelix `shell.default_shell` | settings.jsonc |

**Right-layer call-out:** keep `yazelix-config` component untouched; do NOT embed the layout in settings.jsonc — ship it as a separate KDL asset.

## Dashboard model
- Tabs from `.meta.yaml` tags: meta-core (untagged core crates, dep order), tools/env, ops, ai, docs, mcp, hubs, untriaged.
- Pane-per-repo; cap `panes_per_tab` (default ~6) with overflow → numbered sub-tabs (`ai (1)`, `ai (2)`).
- Fixed "mission-control" overview tab (focus): mesh status (repowire/weave), aggregate logs/`meta git status`, free shell.

## Per-pane idle-agent launch + mesh
- KDL: `pane name=<repo> cwd=<abs> command=<launcher> { args <repo-id> }`.
- Launcher (shipped asset `envctl-dashboard-pane`): sets `META_REPO`, mesh identity env, `DASHBOARD_PANE=1`, then `exec claude` (idle). Overridable via `ENVCTL_DASHBOARD_PANE_CMD` to escalate to forge-loop/env-install-loop later.
- Mesh registration is IMPLICIT via each session's MCP baseline (weave + repowire). Engine never talks to the mesh.

## Work breakdown ([V] = independently verifiable)
1. [V] Engine types (`dashboard.rs`): MetaRepo, MetaWorkspace, DashboardSpec, DashboardPlan/Tab. KDL as String (no kdl crate).
2. [V] `.meta.yaml` reader (pure-Rust YAML already in graph; walk-up locate). Fixture unit tests.
3. [V] Grouping + KDL renderer: pure `render(workspace, spec) -> DashboardPlan`. Golden-file KDL test.
4. Engine method + Event: `Engine::dashboard(spec, sink) -> DashboardPlan` (read-only) + `deploy_dashboard(spec, dry_run, sink)`; `Event::Dashboard`; `EngineCommand::Dashboard` for GUI parity.
5. Fail-closed deploy as Wiring: write to layouts dir, dry-run default, backup-then-write, revert on reset, `--force` to clobber foreign file.
6. CLI verb `envctl dashboard {meta_file, panes_per_tab, deploy, apply, force, json}`. Default = render to stdout (read-only).
7. GUI parity action (same Engine API).
8. meta surface: NEW `meta_dashboard_cli` plugin (info+exec → shells `envctl dashboard --json`). Dep: meta_plugin_protocol only.
9. Config assets (`assets/scripts/`): pane launcher `envctl-dashboard-pane` (idle claude, overridable, mesh identity).
10. Manifest component (additive): `dashboard` drop-in; install/fix → `envctl dashboard --deploy --apply`; remove reverts. Keep `yazelix-config` untouched.
11. [V] Tests: unit (golden KDL, grouping, spill, fixture .meta.yaml), CLI integration, deploy dry-run (no write), plugin contract.
12. Lock/manifest + docs: regen `envctl.lock` if component added; `.meta.yaml`+`.gitignore` for new plugin repo; broker-unification follow-on note.

## Invariants (all PASS)
- No-C: YAML parser must be pure-Rust/already-in-graph; KDL = String. Run `no-c.sh`.
- One rustls/ring: unaffected (zero networking).
- Single non-printing engine: logic in engine, returns via Event; CLI+GUI parity; meta plugin consumes CLI not engine.
- Fail-closed/dry-run: render read-only; layout WRITE is dry-run by default, `--apply`/`--force`, backup, revert on reset; panes idle (no auto-loops).
- No language drift: generator is Rust; KDL + launcher are config assets (sanctioned like manifest/*.toml).
- Lock/manifest sync: only if component added; + .meta.yaml/.gitignore for plugin repo.
- CI gates: no-c.sh, shape.sh, enable.sh (unaffected).

## Open risks
1. YAML parser dep — confirm pure-Rust/in-graph (verification step, not a fork).
2. Untagged core crates → synthetic "meta-core" tab (declaration order). Explicit tags = .meta.yaml edit (no-drift holds).
3. Pane density ~40 → panes_per_tab cap + sub-tab spill.
4. Mesh identity uniqueness — launcher owns stable unique ids; weave self-inbox invisibility → cross-pane mail via to:all/repowire peers.
5. envctl-on-PATH for the meta plugin → clear fail-closed error if absent.
