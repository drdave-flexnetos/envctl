# 03 — Guardian Report: Fix secretd

## Verdict: PASS-WITH-NOTES (0 blocking, 2 notes) — independently re-run

## Gates
- `ci/gates/no-c.sh` = 0 — `rustls=0.23.40 on ring=0.17.14; zero aws-lc/openssl/C-SQLite`.
- `ci/gates/shape.sh` = 0; `ci/gates/enable.sh` = 0.

## No-C after sd-notify (critical) — PASS
`cargo metadata`: `sd-notify 0.5.0` `links: None`, no build script (deps libc/sendfd, FFI-only, no C). `libsql 0.9.30` `links: None`; workspace pins `default-features=false, features=["remote"]`. `libsql-sys`/`rusqlite` absent. Only banned-substr hit = `libsql-sqlite3-parser` (accepted lemon.c→Rust codegen, links None). One rustls, ring-only. **sqld is NOT a Cargo dep** (external binary).

## Build/test/lint — PASS
build --workspace = 0; test (secretd+secrets-engine) = 0 (**129 passed, 1 ignored** = libsql durability test needs live sqld); fmt --check = 0; clippy -D warnings = 0.

## Engine purity / parity — PASS
`git diff -- crates/secrets-engine crates/engine` EMPTY — engine untouched; fix is proto/daemon/CLI/manifest wiring over existing `Engine::init_vault`. Daemon emits tracing + SecretEvent::Log, no println!. secretctl is the sole front-end (CLI↔daemon↔engine parity), exercised by e2e.

## Security heart — PASS
- apply-gated: `grpc.rs` dry-run returns Ok() BEFORE init_vault (no DEK/keyslot/audit row). Test `e2e_vault_init_apply_gated_and_refusals`.
- owner-only: SO_PEERCRED OwnerGuard interceptor.
- argon2 forced server-side: InitReq carries NO KDF fields; daemon forces m=1GiB/t=4/p=4 (`forced_argon2_params`); test asserts ≥ floor.
- USB keyfile never on wire: InitReq carries only PARTUUID; `read_usb_keyfile` reads daemon-side, fails closed non-panicking.
- re-init refused (engine guard); unlock no oracle (unchanged).

## Daemon health (re-confirmed) — PASS
env-ctl.service active; sqld.service active; **NRestarts=0** (was 789). `secretctl status` → `locked secret_count=0` (healthy unprovisioned). `secretd.toml` libsql loopback `[store]`, no auth token in file; journal: `store backend = libSQL remote (durable)`, socket=secretd.sock.

## Lock — PASS
`lock --check` clean (50 components); sqld added + env-ctl requires sqld; sd-notify in Cargo.lock.

## Scope guard — PASS
Nothing committed; vault still locked/uninitialized; no n8n key; .meta.yaml untouched. (Classifier correctly blocked a live --apply probe — security paths verified by code+e2e.)

## NOTES (non-blocking)
1. Implementer log wording imprecise on sd-notify API (code correct: slice form).
2. **`RealUsbProbe::keyfile_for` is `todo!()`** — USB enrollment (U1) seam unimplemented; daemon fails closed cleanly. Full U1 provisioning (USB enroll + --apply + n8n key) is BLOCKED until RealUsbProbe lands AND the hardware USB is present (see runbook). The passphrase init path works today.
