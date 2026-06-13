---
name: cross-repo-reference
description: >-
  How to build a cross-repo reference map — who calls/uses what ACROSS multiple repos (source X,
  destination Y, shared substrates) — using git-kb code intelligence (callers/callees/impact), the
  meta CLI, and the protocol-drift method. ALWAYS use when merging code between repos, assessing the
  blast radius of a change across repo boundaries, finding where a symbol attaches/collides in another
  repo, or checking contract compatibility across repos. Triggers on "cross-repo references", "who
  calls this in <repo>", "blast radius across repos", "does this break <repo>'s consumers". Graph, not grep.
---

# Cross-Repo Reference

A merge (or any cross-repo change) lands blind without a **reference map** spanning the repos involved.
This skill builds that map — the call/usage graph across source X, destination Y, and the substrates —
so you can see where an X symbol attaches in Y, who breaks if it changes, and whether a contract Y's
consumers depend on stays compatible. Used by `rust-port-cross-repo-referencer`; feeds
`rust-port-merge-integrator`. **Graph (AST/call-graph), never grep.**

## Method

1. **Index every repo in scope** — `git kb code index <repo>` for X, Y, and the substrate repos
   (`hf`/`weave`/`grit`/`icm` as relevant). An empty index is **fail-closed** (re-index); never read an
   un-indexed repo as "no references". The code index is **branch-scoped** — re-index after a
   branch/worktree switch.
2. **Map the crossing edges.** For each candidate landing, query both sides:
   ```bash
   git kb code callers <symbol> --json     # who calls it (in each repo)
   git kb code callees <symbol> --json     # what it calls
   git kb code impact <file> --json        # transitive blast radius
   ```
   plus the meta CLI for repo-spanning queries (`meta query`, `meta exec -- git kb code ...`). The value
   is the **crossing** edge: X-symbol ⟷ Y-symbol, with the caller set on each side — a reference that
   only looks at one repo is half a fact.
3. **Blast radius per merge.** For each unit to merge, state exactly who in Y breaks if the landing
   changes a signature/type — the input to the merge-integrator's grit symbol locks and conflict
   resolution.
4. **Contract/protocol compatibility.** Where the change crosses a contract Y's consumers depend on (a
   shared protocol/API/type — e.g. a `meta_plugin_protocol`-style surface), apply the **protocol-drift
   method**: compare the contract across the boundary. A wire/type change is a **breaking change to
   flag**, not silently ship.

## Output

`.handoff/loop/reports/cross-repo-refs.md` — per-symbol table:
`X-symbol · candidate Y landing · Y callers impacted (blast radius) · contract-compat note · suggested
grit lock scope`. Plus a refreshed delta per merge cycle (only the touched symbols).

## Discipline

- **Graph, not grep** — `git kb code` / meta, never text matching (misses overloads, re-exports;
  can't prove completeness). Grep is for strings/config only.
- **Both sides of every edge** — X *and* Y (and substrate); a one-repo view hides the breaking merge.
- **Fail-closed on empty** — an empty graph for a non-empty repo is `INCONCLUSIVE` (re-index), never
  "no references" — a fake-empty graph would green-light a breaking merge.
- **Collect mechanically, escalate judgment** — gather and structure the graph (cheap/mechanical); the
  reconciliation *decision* (which symbol absorbs which) belongs to the merge-integrator.
- **Feed the dual no-downgrade gate.** The Y-side blast radius you compute is also the set of Y behaviors
  the merge must not regress — surface it so the orchestrator captures Y's golden baseline for those
  symbols at DISCOVER and diffs after each merge. A breaking contract is **flagged here, resolved by the
  merge-integrator** (additive / shim / versioned bump via protocol-drift) — never silently shipped.
- **Y-drift re-check.** Y's base advances during a multi-session merge. On resume / per cycle, after Y is
  fetched+rebased, **re-run over the *merged* set** — any merged unit whose Y blast-radius changed is
  surfaced so it re-verifies (a merge proven against an old Y isn't proven against the new Y).
