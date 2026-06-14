---
name: harness-loop-init
description: >-
  Initializes a harness loop's durable state directory (.handoff/loop/) in the current repo — the
  first step of any loop harness, before DISCOVER. Creates findings/ + reports/, seeds loop_state.md
  from the harness's template, and writes the state-contract README. Idempotent (never clobbers
  existing state). ALWAYS use on "init the loop", "create the handoff directory", "set up .handoff/
  loop", "harness loop init", "initialize durable state", or when a loop starts in a repo that has no
  .handoff/loop yet. Lays down the directory that session-relay-wrap-up writes and session-relay-resume
  reads.
---

# harness-loop-init — initialize a loop's durable state

Every loop harness keeps its truth on disk under **`.handoff/loop/`** so a fresh process resumes cold
with zero loss (ADR-0004 / P7.36). This skill lays that directory down — the loop's *first* action,
before DISCOVER — and is safe to re-run (idempotent: it creates what's missing, never overwrites
live state). It's the bootstrap partner to `session-relay-wrap-up` (writes the HANDOFF) and
`session-relay-resume` (reads it).

## What it creates (in the current repo, or a target dir)

```
.handoff/loop/
├── README.md            # the durable-state contract (what each file is, what's committed vs ignored)
├── loop_state.md        # the ledger (seeded from the harness's scripts/loop_state.template.md if present)
├── findings/            # per-agent findings (kept; .gitkeep so the dir is tracked empty)
└── reports/             # synthesized reports / inventories
```

At runtime the loop also writes `backlog.md` / `parity-ledger.md` / `research-ledger.md` (per harness),
`baseline.md`, `HANDOFF.md`, and the terminal sentinels (`DONE` / `NEEDS-HUMAN` / `STOP`). Per the
`.gitignore` convention: commit everything under `.handoff/loop/` **except** `*.log` / `ralph-run-*.log`.

## How to run

```bash
bash <harness>/skills/harness-loop-init/scripts/init-handoff-loop.sh [TARGET_DIR] [LOOP_TEMPLATE]
```
- `TARGET_DIR` defaults to the current directory ("here").
- `LOOP_TEMPLATE` (optional) = path to the harness's `scripts/loop_state.template.md`; if given and
  no `loop_state.md` exists yet, it's copied in as the seed. Otherwise a generic skeleton is written.

The script is **idempotent**: existing `loop_state.md` / `HANDOFF.md` / findings are left untouched;
only missing pieces are created. Re-running it to *confirm* the contract is safe.

## Discipline

- **Kernel-first when available.** If the meta handoff kernel (`hf`) owns `.handoff/` in this repo,
  defer to it (`hf` lays down its own structure); use this file-based init only when `hf` is not the
  manager here. Either way the committed `.handoff/loop/` (or the `hf` packet) is the resume signal.
- **Never clobber live state.** Re-init must not erase an in-flight backlog/ledger/HANDOFF. The script
  guards every write with an existence check.
- **Track the contract, ignore the logs.** Ensure the repo's `.gitignore` carries
  `.handoff/loop/*.log` + `.handoff/loop/ralph-run-*.log` (the eject snippet sets this).

## Where it fits

`harness-loop-init` (lay down state) → DISCOVER (seed backlog/ledger) → ITERATE → `session-relay-wrap-up`
(hand off) → `session-relay-resume` (cold start) → … Phase E (`harness-evolution`) closes each run.
