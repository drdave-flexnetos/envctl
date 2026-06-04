# env-ctl research — libSQL/Turso: embedded vs sqld vs remote client

> Research date: 2026-06-02 · Sources verified against the live web on this date. Version/API claims below were confirmed at the time of writing; re-verify before locking implementation decisions. Anything not directly confirmed is flagged **[UNVERIFIED]**.

---

## TL;DR — recommendation for env-ctl

**libSQL cannot satisfy env-ctl's locked "pure Rust / no C deps" decision for the embedded store.** All embedded modes (`core`, `replication`) bundle and compile C SQLite via `libsql-ffi`. Only the `remote` feature is pure Rust — and it has *no local storage*, which is a non-starter for a local-first secrets vault.

Concrete guidance:

1. **Do not adopt libSQL embedded** unless the operator explicitly waives the no-C tenet (env-ctl DESIGN-NOTES OI-1). The bundled `sqlite3.c` becomes part of the secrets-vault trust boundary.
2. **Prefer a pure-Rust embedded store** (`redb` is the leading candidate) for the local vault. This keeps decisions 1 (pure Rust) and 3 (no C deps) intact and passes the CI gate.
3. **Do not rely on any libSQL/Turso built-in encryption-at-rest.** It is not shipping in a usable, default state (see below). env-ctl's app-layer XChaCha20-Poly1305 must remain the sole confidentiality mechanism regardless of backend.
4. **If a server/sync tier is ever needed**, sqld defaults to *no auth / publicly writable* and has *no usable at-rest encryption* — treat the sqld node as untrusted storage and keep all secrets encrypted app-side before they leave the broker.
5. **Turso Database (the Rust rewrite)** is pure-Rust and promising but **beta** — not for a secrets vault yet. Revisit in a later phase.

Net: libSQL is the wrong fit *as an embedded store* for env-ctl's tenets today. Use it (if at all) only as a remote/sync tier in front of an app-encrypted blob, and default-plan around a pure-Rust local store.

---

## Key facts (with inline source URLs)

### libSQL Rust crate — modes and the C dependency

- The `libsql` Rust crate exposes **three operational modes via feature flags**:
  - `core` — local/embedded database, **bundles and compiles C SQLite**.
  - `replication` — embedded replicas with HTTP sync (also pulls in the C engine).
  - `remote` — **pure-Rust HTTP client**, no C code, no local storage.
  - Sources: <https://docs.rs/libsql>, <https://lib.rs/crates/libsql>
- The default feature set enables the C-backed engine. A **pure-Rust build requires `default-features = false` + `remote` only**.
  - Sources: <https://lib.rs/crates/libsql>, <https://docs.rs/libsql>
- The C engine is delivered through **`libsql-ffi`**, which contains a bundled `sqlite3.c` (~8.9 MB) compiled by a `build.rs` at build time. This is confirmed in env-ctl's own DESIGN-NOTES (CF-1 / OI-1) and matches the crate's FFI structure.
  - Sources: env-ctl DESIGN-NOTES.md (OI-1, CF-1) at `/home/drdave/Desktop/env-ctl/docs/DESIGN-NOTES.md`; crate structure at <https://docs.rs/libsql>

### Embedded replicas (libSQL feature)

- **Read-your-own-writes** consistency: the writing replica sees its writes immediately; other replicas observe them on the next sync interval.
  - Source: <https://docs.turso.tech/features/embedded-replicas/introduction>
- Replication is **WAL frame-based** — each frame is a single page change, integrity chained via a rolling checksum over the previous frame.
  - Source: <https://blog.canoozie.net/libsql-replication/>
- Reads are **local file reads** (microsecond latencies) with discrete WAL sync from the primary; an **offline mode** allows local writes that are pushed later.
  - Sources: <https://docs.turso.tech/features/embedded-replicas/introduction>, <https://turso.tech/blog/introducing-offline-writes-for-turso>
- Embedded replicas reached **GA in 2024** (production-ready).
  - Source: <https://docs.turso.tech/features/embedded-replicas/introduction>

### Hrana protocol (client wire protocol)

- Supports **WebSocket and HTTP** transports.
  - Source: <https://github.com/tursodatabase/libsql/blob/main/docs/HRANA_3_SPEC.md>
- **JSON** encoding (Hrana 1/2 compatibility) and **Protobuf** (Hrana 3); supports **SQL stream multiplexing** over a single connection.
  - Source: <https://github.com/tursodatabase/libsql/blob/main/docs/HRANA_3_SPEC.md>

### sqld server (self-hosted libSQL server)

- Inter-node replication between sqld instances uses **gRPC with TLS**.
  - Source: <https://github.com/tursodatabase/libsql/releases>
- **Authentication is opt-in via JWT** (Ed25519 public-key signing). The **default posture is NO auth — publicly readable and writable** unless an auth key is explicitly configured (e.g. `--auth-jwt-key-file`).
  - Sources: <https://hubertlin.me/posts/2024/11/self-hosting-turso-libsql/>, <https://docs.turso.tech/sdk/authentication>
- Conflict resolution in offline/sync scenarios is **Last-Push-Wins** (the first push to land succeeds); custom strategies are possible via application-level hooks.
  - Source: <https://turso.tech/blog/introducing-databases-anywhere-with-turso-sync>

### Encryption-at-rest — the important caveat

- libSQL's built-in encryption-at-rest was **announced (Feb 2024)** but **disabled in `libsql-server` (commit `71a7cfc`, Sept 2024)** and, per the tracking issue, **not re-enabled** as of the latest verified state.
  - Sources: <https://github.com/tursodatabase/libsql/issues/1756>, <https://turso.tech/blog/fully-open-source-encryption-for-sqlite-b3858225>
- **SQLite3MultipleCiphers** (AES-256, PBKDF2) is a **separate, external C library**, *not* a built-in libSQL capability — using it would add another C dependency.
  - Source: <https://utelle.github.io/SQLite3MultipleCiphers/>
- The newer **`turso` crate / Turso Database** engine (distinct from `libsql`) takes a different encryption approach, but it is **beta**.
  - Source: <https://github.com/tursodatabase/turso>
- **Implication for env-ctl:** do not depend on any of these for secrets confidentiality. App-layer XChaCha20-Poly1305 (already in env-ctl's design) must be the source of truth.

### Turso Database (the Rust rewrite, distinct from libSQL)

- **Pure-Rust** rewrite of SQLite with **concurrent writes (MVCC)**, claimed **~4x throughput** over single-writer SQLite.
  - Sources: <https://github.com/tursodatabase/turso>, <https://betterstack.com/community/guides/databases/turso-explained/>, <https://turso.tech/blog/beyond-the-single-writer-limitation-with-tursos-concurrent-writes>
- Provides `turso::sync` (local-first writes + push/pull) with **Last-Push-Wins** conflict resolution.
  - Source: <https://github.com/tursodatabase/turso>
- **Status: BETA — not production-ready.** Not recommended for a secrets vault at this time.
  - Source: <https://github.com/tursodatabase/turso>

### Comparison data point: LiteFS

- LiteFS (Fly.io's SQLite replication via FUSE) sustains roughly **50–100 writes/sec**, limited by FUSE overhead — useful as a sanity check on "SQLite-over-the-network" write ceilings.
  - Sources: <https://fly.io/docs/litefs/faq/>, <https://medium.com/@benbjohnson/thanks-for-publishing-benchmarks-for-litefs-cc6e99f4eb66>

---

## Current versions / APIs (verified 2026-06-02)

| Component | Version | Notes | Source |
|---|---|---|---|
| `libsql` Rust crate | **0.9.30** | Current on docs.rs/crates.io as of June 2026 (released ~early April 2026) | <https://docs.rs/crate/libsql/latest/>, <https://crates.io/crates/libsql> |
| `libsql-ffi` (C backend) | **0.9.30** | Bundles `sqlite3.c`; pulled by `core`/`replication` | <https://docs.rs/libsql> |
| `sqld` (libsql-server) | **v0.24.32** (Feb 14, 2025) | Latest tagged release; gRPC+TLS inter-node | <https://github.com/tursodatabase/libsql/releases> |
| `turso` crate / Turso DB | **Beta** | Pure-Rust rewrite; concurrent writes; `turso::sync` | <https://github.com/tursodatabase/turso> |

API shape (libSQL Rust): build mode is selected by Cargo features. Pure-Rust path = `libsql = { version = "0.9", default-features = false, features = ["remote"] }` → exposes the HTTP client only (no `Database::open` against a local file). Embedded path requires the C-bundled `core`/`replication` features and a C toolchain at build time.

---

## Security tradeoffs (relative to env-ctl's threat model)

| Dimension | libSQL embedded (`core`/`replication`) | libSQL `remote` | sqld self-hosted | Turso DB (beta) | Pure-Rust store (e.g. redb) |
|---|---|---|---|---|---|
| Pure Rust / no C | ❌ bundles `sqlite3.c` | ✅ | ❌ (server is C-backed) | ✅ | ✅ |
| Local storage (offline-first) | ✅ | ❌ network-only | n/a (server) | ✅ | ✅ |
| Built-in at-rest encryption | ❌ disabled/unimplemented | n/a | ❌ not default/usable | ⚠️ beta path | ❌ (use app-layer) |
| Default auth posture | n/a (local) | bearer to server | ❌ **open/writable by default** | n/a | n/a (local) |
| Attack surface for secrets vault | C engine inside trust boundary | network dependency | untrusted node unless hardened | immature codebase | minimal, Rust-only |
| SQL ergonomics | ✅ full SQLite SQL | ✅ (remote) | ✅ | ✅ (growing) | ❌ KV; needs query layer |

Key security points for env-ctl:

- **C inside the vault.** Any embedded libSQL path puts bundled SQLite C into the same process that handles plaintext secrets after decryption. A memory-safety bug in `sqlite3.c` is then a direct vault compromise vector — exactly what the no-C tenet exists to avoid.
- **sqld is untrusted-by-default.** Public-writable default + no usable at-rest encryption means a sqld node must be treated as adversarial storage: only ciphertext should ever reach it, and JWT auth + TLS must be explicitly configured.
- **No "free" encryption.** Regardless of backend choice, env-ctl's XChaCha20-Poly1305 app-layer encryption + argon2id keyslots remain mandatory. None of these backends safely replace it.
- **Remote feature breaks the local-only model.** It removes the C dependency at the cost of making every read/write a network call — incompatible with a fail-closed, local-first vault that must function with the USB key and no network.

---

## Concrete guidance for the env-ctl implementation

1. **Phase 0 (now):** keep the RAM-only `inmem-store` feature to unblock CI and validate the store trait, as already planned. No backend lock-in.
2. **Default plan — pure-Rust local store:** design the vault `Store` trait against **`redb`** (or another pure-Rust embedded engine). Accept the loss of SQL ergonomics and build a thin typed access layer instead of a SQL ORM. This is the only path that satisfies locked decisions 1 and 3 without an operator waiver.
3. **If SQL is deemed essential:** escalate OI-1 for an explicit, documented operator waiver of the no-C tenet, with a recorded risk acceptance covering bundled-SQLite CVEs landing inside the secrets-vault process. Do not silently adopt libSQL embedded.
4. **Server/sync tier (only if needed later):** if a multi-host model emerges, front sqld with app-encrypted blobs only, enforce JWT (Ed25519) + TLS, never run sqld with default open auth, and never assume sqld encrypts anything at rest.
5. **Keep app-layer crypto authoritative:** ciphertext-in, ciphertext-out at the storage boundary for every backend. The store sees opaque blobs; XChaCha20-Poly1305 + argon2id keyslots + USB-UUID unlock stay in the application layer.
6. **Watch Turso Database (Rust rewrite)** for a stable, audited release. If it GAs with proven encryption and concurrency, it could later become the pure-Rust SQL store env-ctl wants. Not now.

---

## Open questions (unverified or contradictory)

| Question | Status |
|---|---|
| Has libSQL's encryption-at-rest been re-enabled in `0.9.30` / `sqld v0.24.32`? | **Not found in release notes; assume still disabled.** **[UNVERIFIED]** — confirm against current `libsql-server` source before relying on it. |
| Is sqld at-rest encryption opt-in or absent in the latest release? | No evidence of usable built-in support; treat as **absent**. **[UNVERIFIED]** for any newer point release. |
| Exact cipher/algorithm intended for libSQL's (unimplemented) encryption feature? | Announcement said "encryption at rest" with no algorithm detail. **[UNVERIFIED]** |
| Does a `libsql` point release after 0.9.30 exist (June 2026+)? | 0.9.30 was current at research time; re-check crates.io. **[UNVERIFIED beyond 2026-06-02]** |
| Has the env-ctl team evaluated `redb` ergonomics for the vault schema? | Not yet evaluated; OI-1 lists it as the recommended (a) option but no spike done. **[OPEN]** |
| Published threat model for sqld hosting secrets? | None found in public docs; assume app-layer encryption is required. **[UNVERIFIED]** |

---

## Sources

- libsql 0.9.30 (docs.rs) — <https://docs.rs/crate/libsql/latest/>
- libsql (crates.io) — <https://crates.io/crates/libsql>
- libsql (lib.rs) — <https://lib.rs/crates/libsql>
- libSQL Rust API (docs.rs) — <https://docs.rs/libsql>
- Embedded Replicas (Turso docs) — <https://docs.turso.tech/features/embedded-replicas/introduction>
- libSQL Replication internals (Into the Stack) — <https://blog.canoozie.net/libsql-replication/>
- Hrana 3 spec (GitHub) — <https://github.com/tursodatabase/libsql/blob/main/docs/HRANA_3_SPEC.md>
- libSQL releases (GitHub) — <https://github.com/tursodatabase/libsql/releases>
- Databases Anywhere with Turso Sync — <https://turso.tech/blog/introducing-databases-anywhere-with-turso-sync>
- Offline Writes for Turso — <https://turso.tech/blog/introducing-offline-writes-for-turso>
- Self-hosting Turso libSQL (Hubert Lin) — <https://hubertlin.me/posts/2024/11/self-hosting-turso-libsql/>
- Authentication (Turso docs) — <https://docs.turso.tech/sdk/authentication>
- Enable encryption at rest in libsql-server (Issue #1756) — <https://github.com/tursodatabase/libsql/issues/1756>
- Fully Open Source Encryption for SQLite (Turso) — <https://turso.tech/blog/fully-open-source-encryption-for-sqlite-b3858225>
- SQLite3MultipleCiphers — <https://utelle.github.io/SQLite3MultipleCiphers/>
- Turso Database (Rust rewrite, GitHub) — <https://github.com/tursodatabase/turso>
- How Turso Eliminates SQLite's Single-Writer Bottleneck (Better Stack) — <https://betterstack.com/community/guides/databases/turso-explained/>
- Beyond the Single-Writer Limitation (Turso) — <https://turso.tech/blog/beyond-the-single-writer-limitation-with-tursos-concurrent-writes>
- LiteFS FAQ (Fly.io) — <https://fly.io/docs/litefs/faq/>
- LiteFS benchmarks note (Ben Johnson) — <https://medium.com/@benbjohnson/thanks-for-publishing-benchmarks-for-litefs-cc6e99f4eb66>
- env-ctl DESIGN-NOTES.md (local) — `/home/drdave/Desktop/env-ctl/docs/DESIGN-NOTES.md`
- env-ctl ARCHITECTURE.md (local) — `/home/drdave/Desktop/env-ctl/docs/ARCHITECTURE.md`
