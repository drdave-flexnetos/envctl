# Implementation log: Fix secretd (crash loop + durable store + reachable vault init + socket mismatch)

Status: **GREEN** — all 4 root causes fixed in code+config; daemon brought up HEALTHY (proof captured). Provisioning (USB enroll + vault init/unlock + n8n key) is BLOCKED on hardware (U1 dedicated USB), per scope.

## Changes
- `crates/secrets-proto/proto/control.proto`: added `Vault.Init` RPC (streams `Event`) + `InitReq { optional passphrase=1; enroll_usb=2; usb_partition_uuid=3; apply=4 }`. Codegen regenerated via the crate's protox build.
- `Cargo.toml` (workspace): added `sd-notify = "0.5.0"` to `[workspace.dependencies]` (pure-Rust systemd readiness).
- `crates/secretd/Cargo.toml`: added `sd-notify = { workspace = true }`.
- `crates/secretd/src/main.rs`: in `serve()`, after `bind_uds` + before `server::serve`, send `sd_notify::notify(&[NotifyState::Ready])` (the crash-loop fix); send `NotifyState::Stopping` after serve returns. Both best-effort (log on err), no-op when `$NOTIFY_SOCKET` unset. (sd-notify 0.5 API: `notify(state)` — single arg.)
- `crates/secretd/src/conv.rs`: added `forced_argon2_params()` (returns `Argon2Params::default()` = hardened floor m=1 GiB,t=4,p=4) and `init_usb_uuid(&InitReq)` (validates enroll_usb requires a non-empty partuuid; fail-closed `invalid_argument`). Added `#[cfg(test)] mod tests` (4 tests).
- `crates/secretd/src/grpc.rs`: implemented `VaultSvc::init` + `InitStream` over `Engine::init_vault` — apply-gated (apply=false ⇒ DRY-RUN preview Log, mutates nothing), owner-only (existing SO_PEERCRED interceptor), forces server-side Argon2, inherits the engine's refuse-if-already-initialized guard. Added `read_usb_keyfile()` helper that fails closed (non-panicking) for the unimplemented USB hardware seam.
- `crates/secretd/tests/e2e.rs`: added `e2e_vault_init_apply_gated_and_refusals` (dry-run mutates nothing → unlock fails; enroll_usb w/o partuuid ⇒ InvalidArgument; apply passphrase init → unlock works; re-init ⇒ refused) + `drain_expect_err` helper.
- `crates/secretctl/src/cli.rs`: added `Cmd::Init { passphrase_stdin, enroll_usb, usb_partuuid, apply }`.
- `crates/secretctl/src/main.rs`: wired `Cmd::Init` → `Vault.Init` (client-side guard: enroll_usb requires usb-partuuid).
- `manifest/env-ctl.toml`: (a) install+fix idempotently write `~/.config/env-ctl/secretd.toml` `[store] backend="libsql" url="http://127.0.0.1:8080"` (token NEVER in file); (b) **bashrc socket fix** `control.sock` → `secretd.sock`; (c) **verify hardened** — keep `secretd --self-check` as the hard predicate, add a NON-FATAL serving probe (is-active + `secretctl status` round-trip; never asserts unlocked, never triggers fix); (d) `requires += "sqld"`; unit gets `Wants=/After=sqld.service`.
- `manifest/sqld.toml` (NEW): loopback `sqld` component — installs from the **release tarball** (`libsql-server-<target>.tar.xz`, NOT cargo — keeps its C-SQLite out of the trust boundary), user systemd unit `sqld --http-listen-addr 127.0.0.1:8080 -d <datadir>`, detect/install/verify/fix/remove + sandbox.
- `manifest/envctl.lock`: regenerated (50 components).
- `Cargo.lock`: regenerated (adds `sd-notify 0.5.0`).
- `_workspace/secretd-provisioning-runbook.md` (NEW): the BLOCKED-on-hardware provisioning runbook.

## Engine API (parity contract)
No engine changes (engine-first invariant already satisfied — `init_vault`/`unlock`/`lock` exist, sync, fail-closed, non-printing). Delta is proto/daemon/CLI/manifest wiring over the existing `Engine::init_vault`. There is no GUI front-end for the secrets stack (secretctl is the sole front-end); parity is CLI↔daemon↔engine and is exercised by the e2e test + the live round-trip.

## Tests added
- `conv::tests::forced_argon2_is_at_or_above_the_floor` — daemon's forced params ≥ engine floors.
- `conv::tests::init_usb_uuid_none_when_not_enrolling` / `init_usb_uuid_requires_uuid_when_enrolling` / `init_usb_uuid_trims_and_returns_uuid` — boundary validation (fail-closed on missing UUID).
- `e2e::e2e_vault_init_apply_gated_and_refusals` — REAL server: dry-run mutates nothing, enroll_usb-without-uuid ⇒ InvalidArgument, applied passphrase init ⇒ unlock succeeds, re-init ⇒ "already initialized" refusal.

## Build/test status (commands run)
- `cargo build --workspace` → PASS.
- `cargo test -p envctl-secretd -p envctl-secrets-engine` → PASS (secretd e2e 4/4 + conv 4/4 in 70-pass lib bin + self_check 2/2; engine 15/15 + sub-suites).
- `cargo test -p envctl-secretd --test libsql_e2e -- --ignored` against a fresh loopback sqld (port 8099) → PASS (1/1 durability: init/unlock/put/get persist over libSQL remote).
- `cargo fmt` (touched crates) → clean; `cargo clippy --workspace -- -D warnings` → PASS (0 warnings).
- Gates: `no-c.sh` exit 0 (rustls=0.23.40 ring=0.17.14, zero aws-lc/openssl/C-SQLite — sd-notify confirmed clean in the graph), `shape.sh` exit 0, `enable.sh` exit 0.
- Lock: BEFORE = drift (env-ctl changed, sqld added) exit 1; AFTER `envctl lock` → `envctl.lock matches the manifest (50 components)` exit 0. `Cargo.lock` carries `sd-notify 0.5.0`. (No separate `secretd.lock` artifact exists in this repo; lock state = `Cargo.lock` + `manifest/envctl.lock`.)

## no-C confirmation after sd-notify
`bash ci/gates/no-c.sh` → PASS. `cargo metadata` shows `sd-notify` resolved as a pure registry crate; the gate's resolved-graph assertion stays clean (no banned crate, single ring-only rustls). sqld is an EXTERNAL binary (release tarball), not a Cargo dep — nothing C linked into the workspace.

## sqld install method
Prebuilt **release binary** from `tursodatabase/libsql` (`libsql-server-v0.24.32`, asset `libsql-server-x86_64-unknown-linux-gnu.tar.xz`), placed at `~/.local/bin/sqld`, run as a user systemd unit on `127.0.0.1:8080` with data dir `~/.local/share/sqld`. Verified: `sqld 0.24.32`.

## Daemon-now-active evidence (proof of the fix)
- `systemctl --user is-active env-ctl.service` → **active** ; `sqld.service` → **active**.
- `systemctl --user show env-ctl.service -p NRestarts` → **NRestarts=0** (no crash loop; was 789).
- `secretctl status` → `locked  usb_possessed=false  active_relays=0  secret_count=0` (HEALTHY — vault unprovisioned; locked is healthy). JSON: `{"active_relays":0,"secret_count":0,"unlocked":false,"usb_possessed":false}`.
- journal: `store backend = libSQL remote (durable) url=http://127.0.0.1:8080` + `secretd listening socket=.../secretd.sock owner_uid=1000`.
- Live `secretctl init --passphrase-stdin` (no --apply) → emits DRY-RUN preview (`Argon2id m=1048576 KiB, t=4, p=4`), status stays `locked` (no mutation). Live `secretctl init --enroll-usb --apply` (no partuuid) → fail-closed refusal.
- `envctl auto-detect` → `sqld [healthy] wired`, `env-ctl [healthy] wired`.

## Deviations
- None on design. One documented hardware boundary: the engine's `RealUsbProbe::keyfile_for` is `todo!()` in this build (USB is the deliberate hardware gate). The daemon's `read_usb_keyfile` therefore fails closed with a clean refusal for `--enroll-usb --apply` rather than calling the panicking seam — the full wire/CLI/daemon plumbing is built and exercised; only the actual USB keyfile read awaits the hardware step. This is consistent with scope ("build the path, do NOT enroll").
- "secretd.lock" from the plan does not exist as a repo artifact; lock state is `Cargo.lock` + `manifest/envctl.lock` (both synced).

## BLOCKED provisioning gate
USB enroll + `secretctl init --apply` + unlock + n8n-key storage require the user's **dedicated USB token (U1)** inserted AND the `RealUsbProbe` hardware seam implemented. Runbook: `_workspace/secretd-provisioning-runbook.md` (marked BLOCKED on hardware). Per scope: NOT done here.

## Handoff notes (for the invariant-guardian)
- **Fail-closed to verify:** `Vault.Init` apply-gating — `crates/secretd/src/grpc.rs::init` emits a DRY-RUN Log and returns `Ok(())` WITHOUT calling `init_vault` when `apply=false`; the engine's own `init_vault` refuses re-init and re-validates the Argon2 floor. Covered by `e2e_vault_init_apply_gated_and_refusals` (dry-run-then-unlock-fails proves no mutation; re-init refusal; InvalidArgument on missing partuuid).
- **No KDF over the wire:** `InitReq` has NO argon2 fields; the daemon forces `conv::forced_argon2_params()` server-side (test `forced_argon2_is_at_or_above_the_floor`).
- **USB keyfile never on the wire:** `InitReq` carries only the PARTUUID selector; the keyfile is read daemon-side via the seam (`read_usb_keyfile`), which currently fails closed (hardware gate) — verify it does NOT panic and does NOT accept keyfile bytes from the client.
- **no-c after sd-notify:** re-run `ci/gates/no-c.sh` — must stay PASS; confirm sqld is NOT a Cargo dep (it's a release binary).
- **Unlock stays a single generic failure (no oracle):** unchanged; `Vault.Init` does not add an oracle.
