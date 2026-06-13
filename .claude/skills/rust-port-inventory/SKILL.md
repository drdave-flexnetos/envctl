---
name: rust-port-inventory
description: >-
  How to exhaustively inventory a source project for a Rust port and build the parity ledger —
  every module, export, behavior, error path, config, CLI, route, side effect, and edge case.
  ALWAYS use when starting a Rust port, seeding/refreshing the parity ledger, or running the
  pre-DONE left-behind sweep. Triggers on "inventory the source", "what needs porting", "parity
  ledger", "did we miss anything", "left-behind sweep". The anti-"feature left behind" method.
---

# Rust-Port Inventory

The port can only be complete *against a list*. This skill builds that list — the parity ledger —
exhaustively, so nothing is silently dropped. Used by `rust-port-cartographer`.

## Method (breadth-first, then deepen)

1. **Map the surface.** Enumerate packages/modules/files carrying logic. Use the source's own
   structure (workspace members, `package.json`/`pyproject` entry points, route tables, CLI defs).
   Prefer AST/symbol tools (`git-kb code symbols --json`, language servers) over grep for accuracy.
2. **Extract contracts, not names.** For each unit record what it *does*: inputs, outputs, side
   effects (fs/net/DB/process), **every error/exception path**, edge/empty/null handling, and any
   ordering/concurrency guarantee. A row named but not contracted is a stub waiting to happen.
3. **Capture the implicit surface** — the things naive ports drop:
   - config keys + env vars (read `.env.example`, config loaders) and their defaults/validation;
   - CLI flags & subcommands; HTTP routes, middleware, auth, status codes;
   - background jobs, schedulers, signal handlers, graceful-shutdown;
   - serialization formats & wire compatibility; logging/metrics; feature flags;
   - documented behaviors in README/CHANGELOG/tests that aren't obvious from code.
4. **Write the ledger** — `.handoff/loop/parity-ledger.md`, one row per unit, status `- [ ]`,
   dependency-tagged. Schema + legend: `rust-port/references/parity-ledger.md`.
5. **Harvest symbols (per unit, deterministically).** For each unit, enumerate **every** exported/
   public/observable symbol (fn, type, method, field, const, enum variant, trait, CLI flag, HTTP
   route) into `.handoff/loop/symbol-map.md` — one row each, `unit:` tagged to its ledger row, status
   `- [ ]`. Harvest from the AST/index, **never grep**:
   ```bash
   git kb code index <source_root>
   git kb code symbols --file <source-file> --json --limit -1   # --limit -1 = no truncation
   ```
   Routes/CLI flags that aren't AST symbols come from the route table / CLI definition. The harvested
   set (after the row-eligible visibility filter) is the provable denominator; a zero-symbol harvest
   of a non-empty source is fail-closed (`NEEDS-HUMAN`), never "no symbols". Schema + harvest detail:
   `rust-port/references/symbol-map.md`.

## Completeness discipline

- **Never sample a large source** — record deferred areas as explicit `- [ ]` "inventory X" sweep
  rows. Coverage is stated, never assumed.
- **Tests are inventory.** The source's test suite enumerates behaviors the authors cared about;
  every distinct behavior tested is a ledger row (and a future parity fixture).
- **Left-behind sweep (pre-DONE), at two grains:** re-walk the source and diff against the *unit*
  ledger (any absent unit or `- [ ]`/`- [~]` row blocks DONE); then **re-harvest the full source
  *symbol* set** (`git kb code symbols --json --limit -1`) and diff against `symbol-map.md` — any
  source symbol with no row, any `- [ ]`/`- [~]`/`- [!]` symbol row, or any `- [x]` unit whose
  symbols aren't all `- [x]`/`- [≠]` also blocks DONE. A zero/empty symbol re-harvest of a non-empty
  source is fail-closed (`NEEDS-HUMAN`), never a vacuous `0/0`. Treat "I think that's everything" as a
  hypothesis to disprove, at both grains.

## Output
`.handoff/loop/parity-ledger.md` (authoritative units) + `.handoff/loop/symbol-map.md` (authoritative
symbols, one row per source symbol, `unit:`-tagged) + `.handoff/loop/reports/inventory.md` (counts by
status at both grains — units X/Y, **symbols X/Y** — deferred areas, harvest method + visibility
filter, coverage notes).
