# env-ctl research — tonic gRPC over UDS + SO_PEERCRED authz

> Scope: the **control plane** of env-ctl — a local gRPC API over a Unix-domain
> socket (UDS), authenticated by kernel-supplied peer credentials
> (`SO_PEERCRED`). This is distinct from the **relay data plane** (HTTPS +
> local CA). Verified against live sources on **2026-06-02**. Knowledge cutoff is
> Jan 2026; every version/API claim below was re-checked on the web on the
> research date. Items I could not verify are flagged **[UNVERIFIED]**.

---

## TL;DR — recommendation for env-ctl

1. **Stay on the pinned stack** (`tonic = "0.12"`, `tonic-build = "0.12"`,
   `prost = "0.13"`, `tower = "0.5"`, `hyper = "1.5"`, `tokio = "1.43"`,
   `rustix = "0.38"` with `["process","net"]`). This combination is internally
   consistent (hyper 1.x + tonic 0.12) and matches the workspace `Cargo.toml`.
2. **Pin tonic to `>=0.12.3`, never `0.12.0`–`0.12.2`.** Those three versions
   carry a remotely exploitable DoS (CVE-2024-47609 / GHSA-4jwc-w2hc-78qv).
   A bare `"0.12"` caret *will* resolve forward to a fixed release, but assert
   it in CI (`cargo audit` / a `>=0.12.3` floor) so a stale lockfile can't pin
   you to `.0`–`.2`.
3. **Get peer creds from the connection, not the request.** Use tonic's
   `UdsConnectInfo` (populated by `Server::serve_with_incoming` over a
   `tokio::net::UnixListener`), pull `UCred` out of request extensions, and
   enforce `ucred.uid() == owner_uid` in a **per-RPC interceptor**. Reject on
   mismatch with `Status::permission_denied`.
4. **`SO_PEERCRED` is the right primitive** for "only the vault owner may call
   the daemon": it is kernel-enforced, captured at `connect(2)` time, and
   cannot be spoofed by the client. It is the same mechanism systemd/D-Bus use.
5. **Know its one structural weakness (TOCTOU):** creds are frozen at connect
   time. A long-lived connection survives a setuid/exec on the peer. For a
   bearer-token broker holding <=24h USB-gated credentials, pair `SO_PEERCRED`
   with short-lived per-connection authz and re-check on security-relevant RPCs.
6. **`peercred.rs` is still a Phase-6 stub** (module doc only) — this is the
   doc to implement against.

---

## Key facts (with inline sources)

### gRPC-over-UDS in tonic 0.12
- tonic serves over an arbitrary `Incoming` stream of connections via
  `Server::serve_with_incoming(...)`; for UDS you feed it a
  `tokio::net::UnixListener` (wrapped as an incoming stream). The transport
  populates a per-connection `ConnectInfo` you can read from request
  extensions. — https://docs.rs/tonic/0.12.3/tonic/transport/server/struct.Server.html
- For UDS connections that connect-info type is **`UdsConnectInfo`**, with
  exactly two fields, confirmed live:
  - `peer_addr: Option<Arc<tokio::net::unix::SocketAddr>>` ("unnamed" for client
    sockets that did not `bind`)
  - `peer_cred: Option<tokio::net::unix::UCred>` (process credentials)
  — https://docs.rs/tonic/latest/tonic/transport/server/struct.UdsConnectInfo.html

### Peer credentials (`UCred`)
- `tokio::net::unix::UCred` exposes (verified live):
  - `uid() -> uid_t` (always present)
  - `gid() -> gid_t` (always present)
  - `pid() -> Option<pid_t>` — **`Option`**, populated only on Linux/Android/
    iOS/macOS/Solaris/Illumos/Cygwin; `None` elsewhere. On env-ctl's Linux
    target it will be `Some`, but treat it as optional in code.
  — https://docs.rs/tokio/latest/tokio/net/unix/struct.UCred.html
- `UCred` is obtained by tokio from `getsockopt(fd, SOL_SOCKET, SO_PEERCRED)`.
  The kernel fills it at `connect(2)`/`accept(2)` time; the peer cannot forge
  it. — https://man7.org/linux/man-pages/man7/unix.7.html (see `SO_PEERCRED`)
- Background on why `SO_PEERCRED` is trustworthy and the gotchas around it
  (set at connect time, `listen()` socket vs connection):
  http://welz.org.za/notes/on-peer-cred.html

### Reading creds yourself (rustix path — for the relay/manual-accept case)
- `rustix::net::sockopt::socket_peercred(fd) -> io::Result<UCred>` is the
  pure-Rust wrapper around `getsockopt(... SO_PEERCRED)`. Signature confirmed
  live: `pub fn socket_peercred<Fd: AsFd>(fd: Fd) -> Result<UCred>`. Requires
  the **`net`** *and* **`linux_kernel`** features; Linux-only semantics.
  — https://docs.rs/rustix/latest/rustix/net/sockopt/fn.socket_peercred.html
- env-ctl's workspace already enables `rustix = { "0.38", features =
  ["process","net"] }` (Cargo.toml line 27). For `socket_peercred` you also
  need `linux_kernel` — verify it's pulled in (it is part of rustix's default
  Linux backend selection; assert with a build test). **[VERIFY at impl time:
  whether `linux_kernel` is implied by your target+features or must be listed.]**

### Interceptors — where authz actually runs
- tonic provides a blanket `Interceptor` impl for any
  `FnMut(Request<()>) -> Result<Request<()>, Status>`; the trait method is
  `fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status>`.
  — https://docs.rs/tonic/latest/tonic/service/trait.Interceptor.html
- **Critical nuance:** the interceptor sees `Request<()>` (headers/extensions
  only, *not* the message body). That's fine for authz — the `UdsConnectInfo`
  lives in request *extensions*, which the interceptor can read. Use
  `request.extensions().get::<UdsConnectInfo>()`.
  — https://docs.rs/tonic/latest/tonic/struct.Request.html
- `Request` extensions are clonable and survive into the interceptor and the
  handler, so you can stamp a derived `AuthContext { uid, pid }` once and read
  it in handlers. — https://docs.rs/tonic/latest/tonic/struct.Request.html

### Versions / security (verified live 2026-06-02)
- **CVE-2024-47609 / GHSA-4jwc-w2hc-78qv** — remotely exploitable DoS: an
  uncovered error on `accept` of a tcp/tls stream causes the accept loop to
  exit cleanly, killing the server. **Affects `v0.12.0`–`v0.12.2`; fixed in
  `0.12.3`.** Published 2024-10-01.
  - https://github.com/hyperium/tonic/security/advisories/GHSA-4jwc-w2hc-78qv
  - https://advisories.gitlab.com/pkg/cargo/tonic/CVE-2024-47609/
  - Note: this is on the **TCP/TLS accept path**. A pure-UDS control plane with
    a custom accept loop is less exposed, but env-ctl's relay/HTTPS plane and
    any tonic-managed accept loop are in scope — fix by version regardless.

---

## Current versions / APIs (state of the world on 2026-06-02)

| Crate | env-ctl pin | Latest published | Notes |
|---|---|---|---|
| tonic | `0.12` (→ `0.12.x`) | **0.14.6** (2026-05-07) | latest per docs.rs/crate/tonic/latest |
| tonic-build | `0.12` | tracks tonic | keep lockstep with `tonic` |
| prost | `0.13` | (compatible w/ tonic 0.12/0.13) | |
| hyper | `1.5` | 1.x | tonic 0.12 already on hyper 1.x |
| tokio | `1.43` | 1.x | provides `UnixListener`/`UCred` |
| rustix | `0.38` | 0.38/0.39 line | `socket_peercred` needs `net`+`linux_kernel` |

tonic release timeline (verified live, crates.io):
- **0.12.0 — 2024-07-08** (resolves the prior conflicting "June 18 / July 8"
  notes: crates.io says **July 8, 2024**)
- **0.12.3 — 2024-09-26** (first version without CVE-2024-47609)
- **0.13.0 — 2025-03-25**
- **0.14.0 — 2025-07-28**; latest **0.14.6 — 2026-05-07**
- Source: https://crates.io/crates/tonic/versions and
  https://docs.rs/crate/tonic/latest

**Why not upgrade to 0.13/0.14 now?** tonic 0.13 carried a hyper-1.0
trait-compatibility break: raw `tokio::net::UnixStream` (and other I/O types)
must be wrapped in `hyper_util::rt::TokioIo` to satisfy hyper's `Read`/`Write`
traits — code that compiled on 0.12 fails on 0.13 without the wrapper.
- https://docs.rs/hyper-util/latest/hyper_util/rt/struct.TokioIo.html
- The class of break (hyper 1.0 trait migration in tonic 0.13) is real and
  well-documented in the hyper/tonic 1.0 transition; **[UNVERIFIED]** the exact
  GitHub issue number for env-ctl's specific UnixStream case — confirm against
  the tonic 0.13 changelog before any upgrade. The mechanical fix (`TokioIo`)
  is stable and small, but it's churn you don't need before Phase 6 ships.

---

## Security tradeoffs

### What `SO_PEERCRED` gives you (strengths)
- **Unspoofable identity.** uid/gid/pid are written by the kernel at connection
  setup; a malicious local client cannot lie about them.
  https://man7.org/linux/man-pages/man7/unix.7.html
- **No secrets on the wire.** No tokens, no TLS handshake, no key management on
  the control plane — the socket *is* the credential boundary.
- **Same model as systemd/D-Bus/PolicyKit**, i.e. a well-trodden Linux pattern
  for "is the caller allowed to talk to this privileged daemon."
  http://welz.org.za/notes/on-peer-cred.html

### Weaknesses to design around
1. **TOCTOU / credentials are frozen at connect time.** If a peer process
   changes identity (setuid, exec into another binary) *after* connecting, the
   `UCred` you cached still reflects the original. For a long-lived control
   channel this is a real gap. — discussed in tonic's own UDS/peercred thread:
   https://github.com/hyperium/tonic/issues/365
   - **Mitigation for env-ctl:** prefer short-lived control connections; re-read
     creds per RPC (the interceptor already runs per call); for the highest-value
     operations (unlock, mint relay bearer) you may additionally re-`stat` the
     peer's `/proc/<pid>` start-time + exe to detect pid reuse. **[UNVERIFIED:
     exact pid-reuse hardening recipe — design it explicitly.]**
2. **Filesystem permissions are your *other* gate.** `SO_PEERCRED` tells you who
   connected; it does **not** stop them from connecting. Set the socket to
   `0600`/`0700` owned by the vault user and place it in a private dir
   (e.g. `$XDG_RUNTIME_DIR/env-ctl/control.sock`, mode 0700 on the dir).
   Defense in depth: bind perms + uid check, not either alone.
3. **uid==0 (root) bypasses everything.** A local root can `connect` and will
   pass any uid check (or ptrace the daemon, read the socket, etc.). This is an
   inherent limit of any same-host authz; document it as out-of-threat-model.
4. **pid is advisory, not an authz key.** Use `uid` (and optionally `gid`) for
   the decision; use `pid` only for logging/diagnostics, given reuse + the
   `Option` typing.
5. **DoS surface (CVE-2024-47609)** on the accept loop — addressed by the
   `>=0.12.3` floor above.
6. **[UNVERIFIED] historical kernel `getsockopt` race for `SO_PEERCRED`.** Some
   notes reference a locking/UAF race fixed in older 5.x kernels; I could not
   pin the exact CVE/kernel rows on the research date. env-ctl targets Ubuntu
   26.04 (kernel 7.x), far newer than any such fix, so this is not a practical
   concern here — but do not cite specific 5.x version numbers without
   re-verifying.

---

## Concrete guidance for the env-ctl implementation

Target file: `crates/secretd/src/peercred.rs` (currently a Phase-6 stub —
module doc only, no code).

### 1. Bind + lock down the socket (before serving)
- Create `$XDG_RUNTIME_DIR/env-ctl/` mode `0700`, owned by the vault uid.
- `UnixListener::bind(path)`; immediately `chmod` the socket to `0600`.
- On startup, `unlink` a stale socket only after confirming no live daemon
  (avoid the classic "two daemons" race).

### 2. Wire connect-info into tonic (0.12 path)
```rust
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::server::Server;
// note: in tonic 0.12 + hyper 1.x this Just Works; in 0.13+ you must wrap the
// accepted UnixStream in hyper_util::rt::TokioIo (see version table above).

let listener = UnixListener::bind(&sock_path)?;
let incoming  = UnixListenerStream::new(listener);

Server::builder()
    .add_service(SecretsServer::with_interceptor(svc, owner_uid_guard(owner_uid)))
    .serve_with_incoming(incoming)
    .await?;
```
The transport stamps each request's extensions with `UdsConnectInfo`.

### 3. The authz interceptor (uid gate)
```rust
use tonic::{Request, Status};
use tonic::transport::server::UdsConnectInfo;

fn owner_uid_guard(owner_uid: u32)
    -> impl FnMut(Request<()>) -> Result<Request<()>, Status> + Clone
{
    move |req: Request<()>| {
        let info = req.extensions()
            .get::<UdsConnectInfo>()
            .ok_or_else(|| Status::permission_denied("no peer credentials"))?;
        let ucred = info.peer_cred
            .ok_or_else(|| Status::permission_denied("no SO_PEERCRED"))?;
        if ucred.uid() != owner_uid {
            return Err(Status::permission_denied("uid mismatch"));
        }
        // optional: stamp an AuthContext into extensions for handlers
        Ok(req)
    }
}
```
- `peer_cred` is `Option<UCred>` — **fail closed** if it's `None` (do not assume
  presence).
- Compare `uid()` only; log `pid()` if `Some`.

### 4. Alternative / supplement: rustix on a manual accept loop
If you run a custom accept loop (e.g. for the relay plane, or to harden the DoS
surface noted in CVE-2024-47609), read creds directly off the accepted FD:
```rust
use std::os::fd::AsFd;
use rustix::net::sockopt::socket_peercred;
let ucred = socket_peercred(stream.as_fd())?; // needs rustix net + linux_kernel
```
This bypasses tonic's connect-info plumbing and is the most direct,
fewest-moving-parts way to get `SO_PEERCRED`. Keep `["process","net"]` (already
pinned) and confirm `linux_kernel` is active for your target.

### 5. CI / supply-chain gates
- `cargo audit` in CI (catches CVE-2024-47609 if the lock ever drifts below
  0.12.3).
- Keep the existing **no-C gate** (`! cargo tree | grep -q libsql-ffi`) — note
  per current Cargo.toml the libSQL row is **removed/REOPENED (OI-1)** and
  Phase 0 ships only the in-memory store, so the control-plane work here does
  not depend on the store backend decision.
- Pin `tonic` and `tonic-build` to the **same** minor; a mismatch produces
  confusing codegen/runtime trait errors.

---

## Open questions

1. **`linux_kernel` rustix feature** — is it implied by env-ctl's target +
   `["process","net"]`, or must it be added explicitly for `socket_peercred`?
   (Add a tiny build/integration test that actually calls it.) **[VERIFY]**
2. **Per-RPC vs per-connection authz.** The interceptor runs per RPC, so the uid
   check is effectively re-evaluated each call from cached connect-time creds —
   acceptable? Or do high-value RPCs (unlock, mint relay bearer) need an active
   re-probe of `/proc/<pid>` start-time to defeat pid reuse / TOCTOU?
3. **0.13/0.14 upgrade gate.** When (if) you upgrade past 0.12, schedule the
   `TokioIo` wrapper change for the UDS *and* the relay accept paths, re-verify
   MSRV (workspace is `rust-version = "1.80"`), and re-run `cargo audit`.
   Confirm the exact 0.13 changelog entry for the UnixStream/hyper-1.0 break
   before doing it. **[UNVERIFIED issue number]**
4. **Root-in-threat-model?** Decide and document whether local root is
   considered hostile (it can bypass uid authz and ptrace the daemon); if so,
   `SO_PEERCRED` alone is insufficient and you need additional isolation
   (seccomp, separate user namespace, etc.).
5. **Historical `SO_PEERCRED` kernel race** — only relevant if env-ctl ever
   targets pre-5.15 kernels; not applicable to Ubuntu 26.04 (7.x). Don't cite
   specific 5.x fix versions without re-verification. **[UNVERIFIED]**

---

### Sources
- tonic `UdsConnectInfo`: https://docs.rs/tonic/latest/tonic/transport/server/struct.UdsConnectInfo.html
- tonic `Server` / `serve_with_incoming`: https://docs.rs/tonic/0.12.3/tonic/transport/server/struct.Server.html
- tonic `Interceptor` trait: https://docs.rs/tonic/latest/tonic/service/trait.Interceptor.html
- tonic `Request` (extensions): https://docs.rs/tonic/latest/tonic/struct.Request.html
- tonic UDS/peercred TOCTOU discussion: https://github.com/hyperium/tonic/issues/365
- tonic latest version / docs.rs: https://docs.rs/crate/tonic/latest
- tonic versions (release dates): https://crates.io/crates/tonic/versions
- CVE-2024-47609 advisory: https://github.com/hyperium/tonic/security/advisories/GHSA-4jwc-w2hc-78qv
- CVE-2024-47609 (GitLab DB): https://advisories.gitlab.com/pkg/cargo/tonic/CVE-2024-47609/
- tokio `UCred`: https://docs.rs/tokio/latest/tokio/net/unix/struct.UCred.html
- rustix `socket_peercred`: https://docs.rs/rustix/latest/rustix/net/sockopt/fn.socket_peercred.html
- hyper-util `TokioIo`: https://docs.rs/hyper-util/latest/hyper_util/rt/struct.TokioIo.html
- Linux `unix(7)` (`SO_PEERCRED`): https://man7.org/linux/man-pages/man7/unix.7.html
- "On peer cred" notes: http://welz.org.za/notes/on-peer-cred.html
- env-ctl workspace pins: /home/drdave/Desktop/env-ctl/Cargo.toml
- env-ctl Phase-6 stub: /home/drdave/Desktop/env-ctl/crates/secretd/src/peercred.rs
