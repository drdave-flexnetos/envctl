# Symbol map — the per-symbol parity contract (under the unit ledger)

`.handoff/loop/symbol-map.md` sits **beneath** `parity-ledger.md`: the ledger has one row per *unit*
(module/file), the symbol map has **one row per source *symbol*** inside those units. It exists
because a unit can be ported and even parity-verified while a single method, field, enum variant, or
route *inside* it is silently dropped — and a unit-level ledger can't see that. The symbol map closes
that hole: a dropped symbol becomes a visible `- [ ]` row, and an *unmapped* symbol is caught by the
symbol sweep. Owned by `rust-port-cartographer`; symbol statuses gated by `rust-port-parity-verifier`
(same authority split as the unit ledger). This is the finest grain of the no-feature-left-behind
guarantee.

## What is a symbol (one row each — exhaustive)

Every *exported / public / externally-observable* source symbol gets a row:

| Kind | Examples |
|------|----------|
| `fn` | exported/public function, free function |
| `method` | method on a class/struct/trait/object (each overload/signature) |
| `type` | struct / class / interface / type alias / enum (the type itself) |
| `field` | struct field, class property, interface member (each one) |
| `variant` | enum variant / union member / discriminated-union case |
| `const` | exported const / static / module-level constant |
| `trait` | interface / abstract class / protocol / trait |
| `cli` | CLI subcommand or flag (each flag is its own row) |
| `route` | HTTP route (method+path) / RPC method / event handler |

Private/internal symbols with no observable effect are **not** rows — but err toward inclusion: if a
symbol is reachable from any `route`/`cli`/public `fn`, it is observable and gets a row. When in
doubt, add the row (a spurious row is cheap; a missed one is the exact failure mode this prevents).
This set of row-eligible kinds **is** the *visibility filter* — and the sweep below must diff against
a harvest filtered by the *same* rule, so an intentionally-excluded private symbol is excluded from
both the map and the denominator (never flagged as "unmapped"). Record the exact filter (visibility
levels + kinds) in `reports/inventory.md` so it is reproducible.

## Row format

```
- [ ] <sym-id> · <kind> · <source-path>:<symbol-path> · <contract/signature> · -> <rust-target-symbol-path> · unit: <ledger-unit-id>
```

- `<sym-id>` — stable id, namespaced under its unit, e.g. `U07.s12`.
- `<symbol-path>` — fully-qualified in source terms, e.g. `AuthService.refresh` / `Config.timeoutMs`
  / `Status::Pending` / `GET /v1/runs/:id` / `--no-color`.
- `<contract/signature>` — the observable contract: source signature **and** what it *does* (return,
  error kinds, side effects). A row whose contract is just a name is incomplete — a stub can satisfy a
  name, not a contract.
- `<rust-target-symbol-path>` — the concrete Rust item the porter will produce, e.g.
  `auth::AuthService::refresh` / `config::Config::timeout_ms` / `Status::Pending` /
  `routes::runs::get` . `?` until the porter assigns it.
- `unit:` — the parent `parity-ledger.md` row id this symbol belongs to (the rollup key).

## Status legend (identical to the unit ledger — see `parity-ledger.md`)

| Mark | Meaning | Who sets it |
|------|---------|-------------|
| `- [ ]` | not ported | cartographer (harvest) |
| `- [~]` | ported, parity **unproven** | porter |
| `- [x]` | ported **and** differentially parity-verified | orchestrator, only on verifier PASS |
| `- [!] blocked: <reason>` | can't proceed | any |
| `- [≠] intentional-divergence: <reason+approval>` | deliberate behavior change (e.g. symbol intentionally not ported) | only with owner approval |

**Only `- [x]` and `- [≠]` count toward DONE** — same rule as the unit ledger, applied per symbol.

## Deterministic harvest (provable coverage — NOT grep)

The symbol set must be *enumerable and reproducible*, so coverage is a diff, not a judgment call.
Harvest from the AST/index, never from `grep` (grep finds text, misses overloads/re-exports, and
can't prove completeness):

1. **Index the source** (once per run / on refresh): `git kb code index <source_root>` (the `index`
   subcommand lives under `git kb code` — `git kb index` is not a command). Without this step the
   harvest returns `{"symbols":[],"count":0}` and the fail-closed rule below correctly walls — so
   indexing is the precondition, not an optional warm-up.
2. **Harvest per unit**, full output (no cap):
   ```bash
   git kb code symbols --file <source-file> --json --limit -1
   # or for a whole subtree: git kb code symbols --path '<glob>' --json --limit -1
   ```
   `--json --limit -1` is mandatory — JSON is the stable machine shape; `-1` means *no truncation*
   (a default-50 cap would silently drop symbols, defeating the guarantee). Filter by `--kind` /
   `--language` / `--path '<glob>'` as needed. Each JSON row carries `symbol_id`, `name`, `kind`,
   `signature`, `file_path`, `line_range_*`, `parent`, and `language` — exactly the row fields above.
   The `visibility` field may be **null** (e.g. for Rust in current git-kb), so derive the **visibility
   filter from the `signature`'s own marker** (`pub`/`export`/`module.exports`/no-marker=private) +
   the kind, not from that field. Apply the *same* derived filter to the map and the sweep denominator.
3. **Routes / CLI flags** aren't always AST symbols — harvest them from the route table / CLI
   definition (the same sources the inventory skill names) and add a row each. A framework route
   macro or a config-driven flag still gets a row.
4. **Fallback when unindexed:** if `git kb code symbols` returns empty for a language it can't index,
   use a language server / the language's own AST tool (`tsc --emitDeclarationOnly` d.ts, `ast` /
   `inspect` for Python). Record the harvest method in `reports/inventory.md`. **Never fall back to
   grep for the authoritative set** — grep may *cross-check*, never *define* coverage.

The harvested set (after the **visibility filter** above) is the denominator. Total symbols `Y` =
|filtered harvest|; verified `X` = symbols at `- [x]`/`- [≠]`. This `X/Y` is the `loop_state.md`
symbol counter — and the map and the denominator must be filtered by the *same* rule so the diff is
sound.

### Empty harvest is fail-closed (the anti-vacuous-pass rule)

A harvest that returns **zero symbols for a non-empty source** (e.g. a language the index can't
parse, an un-run `git kb code index`, a wrong path) is **`INCONCLUSIVE` → write `.handoff/loop/NEEDS-HUMAN`**,
never read as "no symbols → sweep clean." `Y = 0` over real source code is a *tooling failure*, not
100% coverage: a `0/0` symbol sweep would let an entire unported source pass the symbol gate. It
**blocks DONE** until the symbol set is harvested by a working method (language server / AST tool) and
recorded. The unit ledger does not depend on AST-indexability, so the symbol gate must never be
*weaker* than it by silently degrading to an empty denominator.

## Scale — shard the map for large sources (repos ≥ the flagship target)

A flat `symbol-map.md` is fine for hundreds of symbols, but a source the size of Archon (600+ units,
likely thousands of symbols) makes one file too heavy to read and commit each cycle. For large
sources, **shard the map by package/top-level directory**: `.handoff/loop/symbol-map/<package>.md`
(one shard per ledger package), with `symbol-map.md` kept as a thin index (package → shard path,
symbols X/Y per shard). Then:
- the **porter/verifier read only the shard for the unit they touch** (bounded per-cycle context),
- the **cartographer's sweep concatenates all shards** to form the full denominator (coverage is the
  union of shards — a missing shard is itself a left-behind blocker),
- commits stay small (one shard changes per cycle).
Sharding is an organizational change only — it never relaxes a gate; the rollup rule, the two-grain
sweep, and the fail-closed empty-harvest rule apply per shard and across the union.

## Relationship to the unit ledger (the rollup rule — load-bearing)

- Every symbol row's `unit:` points at exactly one `parity-ledger.md` unit row.
- **A unit cannot be marked `- [x]` until *all* of its symbols are `- [x]` or `- [≠]`.** The
  orchestrator/verifier checks the rollup before flipping a unit — a unit with any `- [ ]`/`- [~]`/`- [!]`
  symbol stays `- [~]`. This is what makes "unit verified" actually mean "every symbol verified."
- A symbol with no owning unit is an inventory bug: add the missing unit row first, then the symbol.
- The verifier's per-unit differential test (see `rust-port-parity`) must exercise **each symbol's**
  contract; a passing unit test that skips a method/variant/route leaves that symbol `- [~]`, which by
  the rollup rule blocks the unit `- [x]`.

## Symbol-level left-behind sweep (pre-DONE — mandatory, additive to the unit sweep)

Before `DONE`, in addition to the unit sweep, the cartographer:
1. **Re-harvests** the full source symbol set (steps above, *same visibility filter*) into a fresh
   denominator. A zero/empty re-harvest of a non-empty source is **fail-closed** — `NEEDS-HUMAN`,
   never a vacuous `0/0` clean (see the anti-vacuous-pass rule above).
2. **Diffs** it against `symbol-map.md`. **Any source symbol with no row → DONE is blocked** (it was
   left behind / never mapped). Any row at `- [ ]`/`- [~]`/`- [!]` → DONE is blocked.
3. **Cross-checks the rollup:** every `- [x]` unit must have 100% `- [x]`/`- [≠]` symbols; a mismatch
   is a defect, not a rounding error.
The sweep result (symbols `X/Y`, unmapped count, rollup violations, harvest method) is recorded in
`DONE` alongside the unit-sweep evidence. Assume a symbol was missed and prove it wasn't — the same
discipline as the unit sweep, one level finer.

## Completeness discipline

- **No silent caps.** Always harvest with `--limit -1`; a truncated harvest is a fake denominator.
- **Empty harvest ≠ done.** A zero-symbol denominator over a non-empty source is fail-closed
  (`NEEDS-HUMAN`), never a vacuous `0/0` pass. Coverage is *harvested and proven*, never assumed.
- **One inclusion rule, both sides.** The map and the sweep denominator use the *same* visibility
  filter (recorded in `reports/inventory.md`), so the diff is sound and DONE is achievable — a
  private symbol excluded from the map is excluded from the denominator too.
- **Divergence is a row, not an omission.** A symbol deliberately not ported is `- [≠]` with owner
  approval — never just absent. Anything else (dropped method, narrowed field, missing variant) is a
  defect the sweep must surface.
- **The map only ever tightens DONE.** It adds blocking conditions on top of the unit ledger; it can
  never let a unit pass that the unit ledger would have blocked.
