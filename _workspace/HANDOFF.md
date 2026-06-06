# Feature Forge HANDOFF — 2026-06-06T00:22Z

**Feature:** Agent Web-Access — Phase 1 (n8n via `czlonkowski/n8n-mcp`)
**Start in a FRESH session.** The previous session is past its useful context limit; feature-forge
must begin cold. This handoff + the plan are everything a cold-start successor needs.

> The dashboard forge-loop this worktree was running is **CLOSED** (all PRs merged, live, healthy —
> see commit `4dfc3cb`). Its record lives in `_workspace/{backlog,loop_state}.md`. This HANDOFF has
> been overwritten to launch the NEW feature. Do not resume the dashboard loop.

---

## Resume command (run this first)

```
/feature-forge Phase 1 of _workspace/AGENT-WEB-ACCESS-PLAN.md (agent web-access: n8n via czlonkowski/n8n-mcp). Read the plan + this HANDOFF. Create a fresh worktree (e.g. agent-web-access) per feature-forge Phase 0. The plan/handoff live committed on branch yazelix-dashboard — copy AGENT-WEB-ACCESS-PLAN.md into the new worktree's _workspace. Honor the verdict (API/MCP, not Pake). HUMAN GATE: the n8n API key must be created in the n8n UI + stored via secretctl before the live verify — scaffold up to that gate if absent.
```

---

## Worktree

- **This worktree:** `/home/drdave/Desktop/meta/.worktrees/yazelix-dashboard/envctl`, branch
  `yazelix-dashboard` — this is the **CLOSED dashboard loop's** worktree. **Do NOT build here.**
- **The successor MUST create its OWN worktree** (e.g. `agent-web-access`) per feature-forge Phase 0.
- The plan (`_workspace/AGENT-WEB-ACCESS-PLAN.md`) and this HANDOFF are committed on branch
  `yazelix-dashboard`. **Copy `AGENT-WEB-ACCESS-PLAN.md` into the new worktree's `_workspace/`**
  (it does not live on `main`/`master`).
- `git status` here: clean, last commit `4dfc3cb loop: dashboard forge-loop CLOSED`.

---

## The verdict (do NOT re-litigate)

The user's original **"Pake + CLI-Anything"** framing was **REFUTED by deep research**
(run `wf_157d834d-19f` — 19 confirmed / 6 refuted claims). The verified-strong architecture is a
**3-tier agent web-access ladder**:

1. **Native API via MCP** — preferred. For n8n: `czlonkowski/n8n-mcp` (MIT, most mature).
2. **Playwright MCP** — for API-less apps (already in the kasetto baseline). [Phase 2 doc, not now]
3. **Pake / CLI-Anything** — **optional, non-Rust, external, OUT of the trust boundary.** [Phase 4]

---

## Registry-federation model (user-clarified)

- Tools register into hub **SUB-registries**: `n8n-mcp` → `mcp_hub`; `pake`/`cli-anything` → `tool_hub`.
- **envctl owns the MASTER registry** — today implicit (`.meta.yaml` + `manifest/` + lock).
  Phase 3 makes it an explicit `envctl registry` federation capability. [later run, not Phase 1]
- **Every adopted tool is EXTERNAL** (like ghostty/podman/icm) — never in envctl's Cargo graph, so
  the no-C / rust-native invariants hold.
- **`N8N_API_KEY` via secretd**, never plaintext.

---

## Phase 1 scope (what feature-forge builds first — highest value)

**Step 1 — HUMAN GATE (FIRST BLOCKER; cannot be automated):**
Create an n8n API key in the n8n UI (http://localhost:5678 → Settings → n8n API), then store it via
`secretctl`. n8n is **LIVE in Docker on :5678** (Public API enabled; `/api/v1/workflows` → 401,
confirmed 2026-06-06T00:22Z). **The successor can scaffold EVERYTHING below EXCEPT the live verify
until this key exists.** If the key is absent, scaffold up to the gate and surface it as the blocker.

**Step 2 —** `mcp_hub/registry.json` entry + `mcp_hub/entries/n8n-mcp.md` for `n8n-mcp`; then run
`mcp_hub/scripts/validate.py`.

**Step 3 —** envctl manifest component `n8n-mcp` (Node/TS server; full
detect/install/verify/fix/remove lifecycle; env `N8N_API_URL=http://localhost:5678` +
`N8N_API_KEY` from secretd). Regen `envctl.lock`; run `ci/gates/{no-c,shape,enable}.sh`.

**Step 4 —** Wire the MCP server into the agent baseline (kasetto project MCP, or global), keyed via
secretd. **Smoke test:** agent creates + activates a test n8n workflow.

**Phases 2–4 (NOT Phase 1, in the plan for later runs):** Playwright-MCP doc; `envctl registry`
master federation; optional Pake/CLI-Anything.

---

## Cycle ledger

- Not a loop handoff — this is a **cold launch of a NEW feature** via feature-forge (single-shot
  orchestrator, not forge-loop). No cycle budget tripped this; the prior dashboard loop completed
  cleanly and is closed.

---

## In-flight cycle

- **None — clean boundary.** No partial architect/implementer/guardian artifacts for the new
  feature exist yet. (The `_workspace/0{1,2,3}_*.md` files are the CLOSED dashboard loop's; ignore.)

---

## Landed this session (dashboard loop — already merged, do NOT redo)

- `4dfc3cb` loop: dashboard forge-loop CLOSED — all PRs merged, live + healthy
- `0054113` loop: dashboard backlog COMPLETE — FU1 (pane-cmd doc) + FU2 (untagged regroup → meta PR #8)
- `b7c92e5` loop: dashboard wire-live complete — gate cleared, installed live, verified

---

## Open findings / blockers

- **BLOCKER (human):** n8n API key not yet created/stored. This is Step 1's HUMAN GATE and gates the
  live verify in Steps 1–4. Scaffold up to it; do not fake it.

---

## Decisions & dead ends

- **Pake + CLI-Anything as the primary path = REFUTED.** Do not architect around it. It is the
  optional tier-3 fallback only (Phase 4, external, out of trust boundary).
- Native-API-via-MCP is the verified-strong primary; `czlonkowski/n8n-mcp` (MIT) is the chosen,
  most-mature n8n server.
- Registry is federated (sub-registries under hub; master owned by envctl) — confirmed by user.

---

## Invariant watch (re-verify — these are non-negotiable)

- **External tools stay OUT of the trust boundary.** `n8n-mcp` is a Node/TS server managed as an
  EXTERNAL component (like ghostty/podman) — it must **never** enter envctl's Cargo graph. No-C /
  rust-native invariants must still hold after the manifest component lands.
- **`N8N_API_KEY` via secretd** — never plaintext in manifest, lock, or config.
- After touching envctl: `bash ci/gates/{no-c,shape,enable}.sh` must pass; `envctl lock --check`
  clean (baseline **49 components**).

---

## Verify-on-resume (run BEFORE mutating, in the NEW worktree)

```bash
rtk proxy git fetch && git status                                   # expect clean
curl -s -o /dev/null -w "%{http_code}" http://localhost:5678/api/v1/workflows   # expect 401 (Public API enabled)
# Only if touching envctl:
bash ci/gates/no-c.sh && bash ci/gates/shape.sh && bash ci/gates/enable.sh
cargo run -p envctl -- lock --check                                 # baseline 49 components, clean
```

---

## Pointers

- **The spec / source of truth:** `_workspace/AGENT-WEB-ACCESS-PLAN.md` (in THIS worktree — copy it).
- **Deep-research run id:** `wf_157d834d-19f` (19 confirmed / 6 refuted).
- Closed-loop record (FYI only): `_workspace/{backlog,loop_state}.md`,
  `_workspace/0{1,2,3}_*.md` (dashboard architect/implementer/guardian artifacts).

---

## Gotchas (these will burn you if ignored)

- **rtk wraps `cargo` and `git`** — its filtering corrupts cargo/git output and diagnostics. For
  raw output (fmt/clippy diffs, exact git state) use **`rtk proxy <cmd>`**.
- **n8n credential read-back is NOT supported** by the n8n API/MCP — you can **create/list only**,
  not read secrets back. Design the smoke test (create + activate a workflow) accordingly.
- **The meta repo uses `main`, not `develop`** for the default/integration branch.
- **This worktree is the CLOSED dashboard loop's** — make a NEW worktree; do not build here.
- The plan file is on branch `yazelix-dashboard`, not on `main`/`master` — **copy it** into the new
  worktree's `_workspace/` before starting.
