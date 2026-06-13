# ADR — Cognitum Seed as envctl's USB possession factor (`seed-factor`)

**Status:** accepted (2026-06-13) · **Owner:** envctl · **Derived from:** owner direction
("full envctl vault-factor, strictly aligned to the Cognitum Seed; a refactor is out of the
question"), ADR-0007 (flexnetos_secrets retirement), the Seed's own ADR-048 (pairing) / ADR-057
(USB implicit trust) / ADR-058 (MCP). Implemented on branch `seed-usb-factor` (PR #50).
Full design log + spike transcript: `PLAN-cognitum-seed-envctl-vault-factor.md` (meta root).

## Context

envctl's secrets vault uses a dual-KEK unlock: a passphrase KEK (Argon2id) and a **USB
possession KEK** (`keyslot::kek_from_usb`, HKDF-SHA256 over a keyfile). The USB side was a
**deliberately unimplemented seam** — `UsbProbe::keyfile_for` had only `RealUsbProbe` returning
`todo!()`, and `secretd::read_usb_keyfile` hard-refused USB enrollment as "hardware-gated." The
whole dual-KEK machinery (slot format, possession gate, daemon init/unlock plumbing) existed and
was exercised; only the hardware backend was missing.

The owner has a **Cognitum Seed** — a Raspberry-Pi hardware root of trust on a USB composite
gadget. It is configured strictly by its own documented procedure; **reconfiguring/refactoring
the Seed is forbidden** ("results in automatic break"). It exposes, over a link-local USB network
(`169.254.42.1`) and SSH (`genesis@…`):

- `POST /api/v1/custody/sign` — signs arbitrary `data` with the device's **Ed25519** key; the
  private key never leaves the device (per the device guide, §Custody).
- `GET /api/v1/identity` — the device's Ed25519 public key.
- USB-only pairing (`pair/window` → `pair`, ADR-048) minting a bearer token; the USB interface is
  implicitly trusted (ADR-057).

Two facts make the Seed a natural USB factor **without touching it**:

1. **Ed25519 signatures are deterministic** (RFC 8032 §5.1.6: the per-message nonce is derived
   from the key and message, not random). So signing a *fixed* message yields a *reproducible*
   64-byte value that only a holder of the device can produce. Empirically confirmed: byte-
   identical signatures across repeated calls **and across a `cognitum-agent` restart**.
2. envctl's USB factor only needs **deterministic high-entropy keyfile material**, which the
   signature is — it slots straight into `kek_from_usb` as HKDF IKM. No new crypto.

## Decision

**Implement the waiting `UsbProbe` seam with a Cognitum Seed backend, behind an opt-in
`seed-factor` cargo feature. Additive only — no seam/trait/crypto change, no Seed change.**

1. **KEK material** (`secrets-engine/src/seam.rs`, `RealUsbProbe::keyfile_for`): under
   `seed-factor`, sign the domain-separated, PARTUUID-bound message
   `envctl/usb-kek/v1/<partuuid>` via the Seed's `custody/sign`, and return the 64-byte signature
   as the keyfile. Default build returns `None` (correct fail-closed "USB absent", replacing the
   `todo!()` panic). Transport is the **documented SSH path** via `std::process` (system `ssh`) —
   **no linked dependency**, so the no-C trust-boundary gate stays green. The bearer token is
   minted and used entirely device-side (localhost-only pairing window over SSH); only the public
   signature returns. Every device call is `--max-time` bounded and SSH uses `ServerAlive*`, so a
   wedged device cannot hang the synchronous unlock path. A fresh random pairing-client name per
   call avoids concurrent-unlock collisions.

2. **Presence gate — Profile S** (`secrets-engine/src/broker/gate.rs`, `SeedPresenceGate`): the
   first concrete `PresenceGate`. `resolve()` signs a **fresh random challenge** and verifies the
   signature with **`ring` Ed25519** against the operator-pinned device public key
   (`ENVCTL_SEED_PUBKEY`) → `Present`; any uncertainty → `Unproven` (fail-closed, no grace). Fresh
   nonce = replay-proof; pinned-key verify = responder authenticity. `ring` is the sanctioned
   verifier (already in the resolved graph via rustls; no-C unaffected).

3. **Daemon wiring** (`secretd`): `read_usb_keyfile` forwards to the seam; a `seed-factor` feature
   forwards to the engine feature. `Vault.Init --enroll-usb` then enrolls a Seed-backed USB
   keyslot alongside the passphrase keyslot; unlock is automatic (`Engine::open` already injects
   `RealUsbProbe`). **Init-time and unlock-time material match by construction** (same partuuid →
   same deterministic signature).

4. **The passphrase keyslot stays enrolled as the recovery factor** — a lost/dead/absent Seed is
   never a permanent lockout.

**Access plane is SSH + one REST call, NOT MCP.** The Seed's MCP advertises 114 tools/21 groups
(ADR-058) — a standing context tax for a device we only need to *sign* with — and Claude Code's
streamable-HTTP client can't pass its health probe against the Seed's transport anyway. SSH is
the Seed's documented admin path and the right possession bootstrap.

## Amendment (2026-06-13) — transport: SSH tunnel → direct pinned-CA HTTPS

**Trigger:** `secretctl unlock` (USB-first) succeeded interactively but **failed under the
`env-ctl.service` systemd sandbox**. Root cause: the seam shelled `ssh genesis@169.254.42.1`, and
`ProtectHome=read-only` (no writable `known_hosts`) + `BatchMode=yes` without
`StrictHostKeyChecking=accept-new` + no ssh-agent ⇒ non-interactive host-key verification fails.
Shelling `ssh` from a sandboxed daemon is fundamentally fragile.

**Change (supersedes Decision §1's transport clause):** `seed_factor::sign_hex` now reaches the
Seed by a **direct, blocking, pure-Rust HTTPS call** (ring-only `rustls`, already in the resolved
graph) to `POST /api/v1/custody/sign`, validating the Seed's TLS against the **pinned Cognitum CA
only** (`ENVCTL_SEED_CA`, default `/usr/local/share/ca-certificates/cognitum-ca.crt`) — FS-S7
frozen-roots discipline, never the OS store. No `ssh`, no `known_hosts`, no agent, no `$HOME`
access, no subprocess → works unchanged under the sandbox; the no-C gate stays green (rustls is
already linked via `mitm-ca`; `seed-factor` just enables the same optional deps). Bounded by a
15s connect/read/write timeout (replaces the SSH `ServerAlive*`/`--max-time` fences).

**Token-at-rest decision (the open ADR item from the plan, now locked):** the device-bound,
revocable bearer token is resolved from `ENVCTL_SEED_TOKEN`, else a **0600 token file**
(`ENVCTL_SEED_TOKEN_FILE`, default `$XDG_DATA_HOME/env-ctl/seed-token`) — deliberately inside the
unit's `ReadWritePaths` (`%h/.local/share/env-ctl`) so the daemon can read it *and* refresh it
under `ProtectSystem=strict`. **Rotation = re-mint on demand:** on a missing/rejected token,
`sign_hex` re-opens the **USB-only** `pair/window` (possession of the USB is the trust floor,
ADR-057), re-pairs under the stable client `envctl-daemon` (replacing any prior token, so no
per-unlock client leak — an improvement over the transient-client churn), persists, and retries
once. A lost/expired token is therefore self-healing whenever the Seed is present. Env knobs:
`ENVCTL_SEED_API` (base URL, default `https://169.254.42.1:8443`), `ENVCTL_SEED_CA`,
`ENVCTL_SEED_TOKEN[_FILE]`, `ENVCTL_SEED_KEK_CONTEXT`.

**Residual (owner-gated live verify):** if the Seed serves `pair/window` *only* on its own
localhost (not over the `169.254.42.1` USB link-local), a cold daemon with no token file cannot
self-bootstrap — the owner pre-seeds the token file once (e.g. by running the
`seed_factor_probe` example or pairing over ssh-localhost). The `custody/sign` hot path is
unaffected either way. This is the only step that needs the physical Seed + the running service.

## Consequences

- **Positive:** the dual-KEK USB half is now real and hardware-backed; SSH-key possession +
  device-held Ed25519 key is a stronger possession proof than a wrapped keyfile on a partition;
  zero Seed change honors the no-refactor constraint; default builds and every invariant are
  unaffected (no-c/shape/enable PASS, ring single 0.17.14).
- **Negative / accepted:** unlock latency includes an SSH round-trip (~1–2s); the SSH key must be
  enrolled on the Seed once (one-time `ssh-copy-id`, owner-gated); the seam fails closed and
  silent (engine is non-printing by design — debugging a `None` relies on the operator probe
  example).
- **Profile S → relay gate: DONE** (commit `068491e`). The egress choke point + both mint sites
  route through one `presence_proven()` resolver — Profile A (uncached local probe) by default,
  Profile S (Seed challenge) under `seed-factor` + a pinned pubkey, behind a **5s presence cache**
  so the per-request egress path never does a live SSH probe (owner decision; ≤5s network-factor
  staleness is the sole deviation from REQ-SEC-13's no-grace rule).
- **Follow-ups (out of scope here):** HARDENING — verify the KEK signature against the pinned
  pubkey in `keyfile_for` too (Profile S already does); `status.usb_possessed` is still a
  hardcoded `false` stub (cosmetic — a live probe per status call would be too costly); fix
  **ADR-0007's phantom `secretctl import`** (correct verb `secretctl secret add`) in the *handoff*
  repo; live `/verify` of a Profile-S-gated relay mint/swap with the Seed present vs absent.

## Alternatives considered

- **Wrapped keyfile on the Seed's storage** (the literal pre-existing model): rejected — the
  COGNITUM partition is read-only and there is no documented secret-storage endpoint; would bend
  the Seed.
- **MCP client integration:** rejected — token-suck (114 tools) and a transport-dialect
  incompatibility with the Seed's MCP server; the REST custody API is what the factor needs.
- **Presence-only (no signature-derived KEK):** the fallback if determinism had failed the spike;
  not needed — determinism holds.

## Verification (runtime, against the live Seed — 2026-06-13)

- **Library surface** (`examples/seed_factor_probe.rs`): `keyfile_for` → 64 bytes, identical
  across calls; `SeedPresenceGate::resolve()` → `Present` (pinned key); `Unproven`/`None` for
  no-key / wrong-key / unreachable. The real spike signature verifies under `ring` ED25519
  (confirms standard, non-prehashed Ed25519 over raw `data` bytes).
- **Daemon surface** (seed-factor `secretd` + `secretctl`, inmem, temp XDG): `init --enroll-usb`
  → `unlock` (no passphrase) → **`vault unlocked (factor: usb)`**; `unlock --passphrase-stdin` →
  **`vault unlocked (factor: passphrase)`**; unreachable Seed → `init --enroll-usb` refuses
  cleanly (no panic, daemon alive).
- secrets-engine tests 108/115; secretd 21; clippy `-D warnings` clean; no-c/shape/enable PASS.

## Research / Cross-References

**Cryptography.** Ed25519 determinism — RFC 8032 §5.1.6 (the nonce `r = H(prefix ‖ M)` is a
deterministic function of the secret key and message; no RNG), which is *why* a fixed message
yields reproducible KEK material. Verification via `ring::signature::ED25519` (the sanctioned,
ring-only verifier; FIPS-style Ed25519 over the raw message, no context/prehash). HKDF-SHA256
(RFC 5869) is the existing `kek_from_usb` KDF; the signature is high-entropy IKM.

**Seed device (authority = COGNITUM drive + live `/guide`, strictly followed, never modified):**
`custody/sign|verify|attestation`, `identity`, `pair/window`→`pair` (ADR-048 pairing,
ADR-057 USB implicit trust, ADR-058 MCP); default SSH login documented in the device-served
`/guide`. Determinism + wire-format confirmed empirically (spike 2026-06-13), not assumed.

**envctl codebase:** `seam.rs` (`UsbProbe`, `RealUsbProbe`), `keyslot.rs:229` (`kek_from_usb`,
`Factor`), `lib.rs:116/214/272/290/1492` (`Unlock`, dual-KEK init, `usb_possession_proven`),
`broker/gate.rs` (`PresenceGate`/`GateState`, Profile A/B/**S**), `broker/decide.rs:250`,
`secretd/src/grpc.rs` (`read_usb_keyfile`, `Vault.Init`), `secretctl/src/cli.rs`
(`init`/`unlock` — note: **no `import` verb**). Invariants: `CLAUDE.md` (no-C trust boundary,
ring-only, engine non-printing); gates `ci/gates/{no-c,shape,enable}.sh`.

**Related ADRs:** ADR-0007 (flexnetos_secrets → envctl as single secrets source); the envctl
secrets corpus `docs/secrets/{SERVER-MODE,THREAT-MODEL,DESIGN-NOTES}.md` (F14 presence gate,
REQ-SEC-13 fail-closed, CF-4 possession, OI-6 rollback fence).
