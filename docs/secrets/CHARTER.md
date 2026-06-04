# env-ctl — Charter

**Status:** Scaffolding · parallel repo to [`envctl`](../../envctl), destined to merge in
**Owner:** Single power-user (owner of the dual-RTX-5090 workstation)
**Created:** 2026-06-02

---

## 1. Mission

Be the **secrets/security layer** `envctl` deliberately left out (Non-Goal N6): a local,
single-operator **secrets vault + credential injector** for one Ubuntu 26.04 dual-RTX-5090
dev box. Store keys, certs, and API tokens encrypted at rest; hand them to tools through a
disciplined injection path instead of `.env` files, shell history, or pasted secrets; and
expose them to authorized local clients through a small API — all under `envctl`'s
boot-repair safety discipline.

## 2. Why parallel, and how it merges back

`envctl` is mid-flight and dogfooded. We build the security layer in its own repo so it can
move fast without destabilizing the shipped engine, then merge by **matching conventions**:

| Inherited from envctl | env-ctl matches it |
|---|---|
| Workspace metadata (`edition = 2021`, `rust-version = 1.80`, `MIT OR Apache-2.0`, shared `[workspace.dependencies]`) | identical |
| `rust-toolchain.toml` (stable + rustfmt + clippy) | identical |
| Pure **engine library**, thin front-ends; **the engine never prints** — it emits a structured `Event` stream | same spine |
| Best-effort orchestration; typed errors for setup-time failures only | same |
| Fail-closed guards; dry-run by default for destructive ops; back up before clobber; never touch user data | same |
| XDG layout (`~/.config`, `~/.local/share`, `~/.local/state`) | same roots, `env-ctl`-namespaced |
| Few mainstream deps, stable toolchain, **no web / no WebView** | same constraint |

**Merge mechanics (target):** the crates here become `envctl/crates/*` (e.g. a `secrets`
engine library + a `secretd` daemon), the secrets CLI verbs fold into the `envctl` binary,
and the secrets components ship as new manifest entries so `envctl install` can stand the
vault up like any other tool. Crate/package names are chosen up front to avoid collision
with the existing `envctl-engine` / `envctl` / `envctl-gui`.

## 3. Scope — the six pillars

1. **security** — threat model (local attacker, process snooping, accidental git commit,
   backup leakage), encryption-at-rest, fail-closed authorization, tamper-evident audit log.
2. **keys** — SSH, API tokens, GPG: import / generate / rotate / expire / list.
3. **certs** — local CA + leaf/mTLS certs for the API and local services: issue / renew / revoke.
4. **auto-inject** — deliver secrets to a process as env vars or files without polluting
   global shell state or touching git.
5. **database** — encrypted-at-rest store: secret bodies, metadata, version history, audit log.
6. **api** — a local daemon serving secrets to authorized clients (CLI, `envctl`, tools).

## 4. Non-goals (this subsystem)

- Not a networked / multi-tenant / team secrets service (no fleet, one operator).
- Not a password manager UI replacement (KeePassXC stays available; we may *interoperate*,
  not replace).
- Not a CA for the public internet (the local CA is for localhost/mTLS between local pieces).
- Not a rewrite of `envctl`'s component engine — it composes with it.

## 5. Foundational decisions (being confirmed with the operator)

These gate the architecture; they are settled with the operator before code lands and then
recorded here as resolved, with rationale:

- **Storage / crypto backend** — how secrets are encrypted at rest and where they live.
- **Master-key / unlock mechanism** — how the vault is unlocked per session.
- **API transport** — how authorized clients reach the daemon (and how that channel is authed).
- **Auto-inject mechanism** — how secrets become a tool's environment without global leakage.

(Resolved values land in `docs/DESIGN-NOTES.md` once chosen.)

## 6. Planned docs (mirroring envctl's set)

- `docs/CHARTER.md` (this file) → grows into `docs/PRD.md`
- `docs/ARCHITECTURE.md` — crate layout, the secrets engine, threat model, data model
- `docs/ROADMAP.md` — phased plan (scaffold → vault core → inject → certs/CA → API daemon → merge)
- `docs/DESIGN-NOTES.md` — resolved decisions + adversarial-review fixes
