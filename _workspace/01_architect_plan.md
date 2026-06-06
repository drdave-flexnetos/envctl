# 01 — Architect Plan: Fix secretd (VERDICT: NEEDS-DECISION)

## Root causes (4, compounding) — deeper than first diagnosed
1. **Crash loop:** unit is `Type=notify` but secretd never sends `sd_notify(READY=1)` → 90s timeout → 789 restarts. Fix: emit READY=1 after bind (pure-Rust `sd-notify` crate — verified `lib_links: null`, zero C). GO.
2. **No durable store:** no `secretd.toml` → in-memory ephemeral → vault wiped each restart. Fix: ship `secretd.toml` `[store] backend="libsql" url="http://127.0.0.1:8080"`. Requires a loopback **sqld** (libSQL remote only; NO pure-Rust local-file store exists — inmem|libsql are the only backends). GO on config; sqld provisioning = DECISION.
3. **Vault has ZERO keyslots — unreachable init (THE real unlock blocker):** `Engine::init_vault` exists but is exposed by **no proto RPC** and `secretctl` has **no `init` verb**. So unlock can only fail. `usb_possessed=false`/"unlock failed" are downstream of an empty vault, not a wrong credential. Fix: add `Vault.Init` RPC (proto + grpc/conv) + `secretctl init` subcommand (apply-gated, owner-only, forced argon2 floor). GO.
4. **Socket-path mismatch:** manifest wires `SECRETCTL_SOCK=.../control.sock`; real socket is `.../secretd.sock`. Fix in manifest. GO.

## Engine API: already sufficient (init_vault/unlock/lock exist, sync, fail-closed). Delta is proto/daemon/CLI wiring only — engine-first invariant working as designed.

## Placement
- crates/secretd: sd-notify READY=1 in serve(); implement Vault.Init over Engine::init_vault.
- crates/secrets-proto: add `Vault.Init`/`InitReq{passphrase?, enroll_usb, usb_partition_uuid, apply}` (passphrase over owner-gated UDS; USB keyfile via UsbProbe seam, never on wire; argon2 forced to floor m=1GiB,t=4,p=4).
- crates/secretctl: `init` subcommand (--passphrase-stdin, --enroll-usb, --usb-partuuid, --apply).
- manifest/env-ctl.toml: ship secretd.toml, fix SECRETCTL_SOCK, harden verify (serving probe, non-fatal), add sqld prerequisite.

## Invariants: PASS — sd-notify pure-Rust (no-c clean); libSQL remote only (one ring rustls); engine unchanged/non-printing; Vault.Init apply-gated + refuses re-init + owner-only (SO_PEERCRED); USB enroll proves keyfile possession; no drift; locks regen (Cargo.lock + envctl.lock/secretd.lock).

## DECISIONS NEEDED
### D1 (blocking) — unattended unlock posture (north-star = no human in loop; THREAT-MODEL FS-S22 = on-box refuses start w/o USB keyslot)
- **U1 dedicated USB token (architect-recommended):** unattended while stick inserted; honors FS-S22/Profile A. Needs a SPARE usb stick (NOT the Ubuntu installer).
- **U2 0600 key-file passphrase:** fully unattended, no token, but unlock credential on same disk as ciphertext (A12 downgrade; explicit audited waiver).
- **U3 interactive passphrase per boot:** strongest, but NOT unattended (rejects north-star).
### D2 (blocking) — durability needs a loopback `sqld` (external binary, not a workspace crate; install via release binary to avoid its C build). Approve adding a `sqld` component (install + user unit ordered Before=env-ctl.service)? Else stay in-memory (NOT durable — secrets lost on restart).
### D3 (minor) — Init RPC placement Vault.Init vs Lock.Init → recommend Vault.Init (orchestrator takes recommendation).

## Work breakdown (leaf-first; ⊙ independently verifiable)
1. ⊙ proto Vault.Init/InitReq + codegen. 2. ⊙ secretd sd-notify READY/STOPPING. 3. ⊙ grpc/conv Vault.Init over init_vault (apply-gated). 4. ⊙ secretctl init verb. 5. manifest: secretd.toml + socket fix + verify hardening + sqld prereq. 6. provisioning (posture-dependent: enroll U1/U2). 7. locks regen. 8. gates no-c/shape/enable.

## Risks
- sqld not yet installed → durability blocked until provisioned (D2).
- Unattended unlock is a real security tradeoff (D1).
- rtk wraps cargo/git → use `rtk proxy`; baseline-stash pre-existing fmt/clippy drift.
