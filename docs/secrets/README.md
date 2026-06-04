# Secrets-stack design corpus (carried over from the `env-ctl` project)

These are the design, threat-model, and research documents for the **secrets vault +
credential broker** subsystem (crates `secrets-engine`, `secrets-proto`, `secretd`,
`secretctl`, `secrets-store-libsql`). They were authored in the standalone `env-ctl`
repository (the feature-enhancement project for envctl) and carried into envctl during the
**2026-06-04 consolidation**, so the canonical repo owns the design basis for the secrets
stack it now ships. The `env-ctl` repo is being archived after this carry-over.

> Note: `docs/ARCHITECTURE.md`, `docs/DESIGN-NOTES.md`, and `docs/ROADMAP.md` at the docs/
> root are the **env-manager** versions. The copies here under `secrets/` are the distinct
> **secrets-stack** versions — kept separate to avoid clobbering either.

## Key entry points
- **`SERVER-MODE.md`** — the Phase-8 remote-relay-edge spec (F2/F5/F6/F14, forbidden-states
  FS-S16..S25, open items OI-SM-1..6). **Read this before the F2/F5/F6 design spike.**
- `THREAT-MODEL.md` — attacker profiles (A1..A16) + threat assertions.
- `DESIGN-NOTES.md` — locked operator decisions; CF/HF/OI item ledger.
- `CHARTER.md`, `SCAFFOLD-SPEC.md` — mission + the Phase-0 type skeleton/acceptance criteria.
- `research/` — 15 deep-dives (e.g. `12-remote-token-binding.md` for DPoP, `02-argon2id-keyslots.md`, `13-tamper-evident-audit.md`).
- `audits/` — the two independent multi-agent security audits (phase-1 crypto+vault, server-mode design).
- `ops/` — operational guides (systemd hardening, USB ceremony, backup, audit signing, CI supply-chain).
- `api/control-plane.proto`, `db/schema.sql` — the gRPC control proto + store schema.

The `env-ctl` build-orchestration scripts (`workflows/*.js`) were intentionally NOT carried
over — they are build-process history, preserved only in the `env-ctl` archive.
