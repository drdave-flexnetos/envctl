# env-ctl research — Secret process hardening in Rust

> Scope: how the env-ctl daemon (libSQL store, app-layer XChaCha20-Poly1305, argon2id keyslots, USB-partition-UUID unlock, gRPC-over-UDS control plane + HTTPS relay data plane) should protect plaintext secrets *while they live in process memory* on Ubuntu 26.04.
> Last verified against the live web: **2026-06-02**. Crate versions and Linux man-page semantics re-checked on this date; anything not re-confirmed is flagged **[UNVERIFIED]**.

---

## TL;DR — recommendation for env-ctl

Layer four independent controls; assume each can fail.

1. **Zero on drop** — wrap every plaintext secret (USB unlock key, derived data-key, relay bearer, decrypted credential) in `secrecy::SecretBox` / `SecretString` backed by `zeroize`. This is your only portable, cross-platform guarantee. ([zeroize 1.8.2](https://docs.rs/zeroize/1.8.2/zeroize/), [secrecy 0.10.3](https://docs.rs/secrecy/0.10.3/secrecy/))
2. **Lock RAM** — `mlockall(MCL_CURRENT | MCL_FUTURE)` at daemon startup so secret pages never hit swap. Raise `RLIMIT_MEMLOCK` first (default cap is only 64 KiB for unprivileged processes). Use `rustix::mm` or `nix::sys::mman`. ([mlockall(2)](https://man7.org/linux/man-pages/man2/mlockall.2.html), [getrlimit(2)](https://man7.org/linux/man-pages/man2/getrlimit.2.html))
3. **Kill core dumps + ptrace** — `prctl(PR_SET_DUMPABLE, 0)` (this single call blocks both core dumps *and* `PTRACE_ATTACH` from non-root), set `RLIMIT_CORE = 0`, and `madvise(MADV_DONTDUMP)` on each secret buffer. The `secmem-proc` crate bundles this portably. ([PR_SET_DUMPABLE](https://man7.org/linux/man-pages/man2/pr_set_dumpable.2const.html), [secmem-proc](https://github.com/niluxv/secmem-proc))
4. **Never `fork()` after `mlockall`** — locks are *not* inherited across `fork(2)` and copy-on-write defeats the lock; use `posix_spawn`/`execve` for any subprocess. ([mlockall(2)](https://man7.org/linux/man-pages/man2/mlockall.2.html))

Accept up front that this stack **cannot** defend against cold-boot / suspend-to-disk RAM capture or Spectre/Meltdown-class transient-execution side channels. Those belong in the threat model as out-of-scope-for-userspace and pushed to disk encryption + CPU/firmware mitigations.

---

## Key facts (with inline source URLs)

### Memory zeroing
- `zeroize` zeros via `core::ptr::write_volatile` plus `core::sync::atomic::compiler_fence(Ordering::SeqCst)`, in pure Rust with no FFI/asm, so the writes are not optimized away. ([docs.rs/zeroize](https://docs.rs/zeroize/latest/zeroize/))
- zeroize's own docs are explicit that it **cannot** prevent leakage via cache timing, Spectre/Meltdown, or other microarchitectural covert channels — it only guarantees the bytes are overwritten. ([docs.rs/zeroize](https://docs.rs/zeroize/latest/zeroize/))
- `secrecy` is deliberately `#![forbid(unsafe_code)]` and therefore ships **no** `mlock`/`mprotect` support — it is a leak-prevention/zeroing wrapper, not a memory-locking layer. ([docs.rs/secrecy](https://docs.rs/secrecy/0.10.3/secrecy/), [lib.rs/secrecy](https://lib.rs/crates/secrecy))
- The long-standing request to add an optional `mlock` feature to `secrecy` (`SecretVec`/`SecretString`) is tracked in **iqlusioninc/crates issue #480 and remains open** as of this writing — do not assume secrecy will lock memory for you. ([issue #480](https://github.com/iqlusioninc/crates/issues/480))

### RAM locking (anti-swap)
- `mlockall(MCL_CURRENT | MCL_FUTURE)` locks all current and future pages; locked pages are never written to swap. ([mlockall(2)](https://man7.org/linux/man-pages/man2/mlockall.2.html))
- Locks are **not inherited by a child created via `fork(2)`** and are **cleared on `execve(2)`**. ([mlockall(2)](https://man7.org/linux/man-pages/man2/mlockall.2.html))
- `mlock` operates at page granularity (typically 4 KiB): two secrets sharing a page are locked/unlocked together. Lay secrets out on separate pages if you need independent lifetime control. ([mlock(2)](https://man7.org/linux/man-pages/man2/mlock.2.html))
- Default `RLIMIT_MEMLOCK` for an unprivileged process is **64 KiB**; a process with `CAP_IPC_LOCK` has no limit. Raise the limit (or grant the cap) before locking anything larger. ([getrlimit(2)](https://man7.org/linux/man-pages/man2/getrlimit.2.html))

### Core-dump prevention
- `prctl(PR_SET_DUMPABLE, 0)` makes the process non-dumpable, which **both** suppresses core dumps **and** prevents `PTRACE_ATTACH` by a non-root tracer. ([PR_SET_DUMPABLE](https://man7.org/linux/man-pages/man2/pr_set_dumpable.2const.html))
- `madvise(addr, len, MADV_DONTDUMP)` excludes a specific region from core dumps (Linux 3.4+). FreeBSD's analogue is `MADV_NOCORE`; macOS has no direct equivalent. ([madvise(2)](https://man7.org/linux/man-pages/man2/madvise.2.html))
- When `kernel.core_pattern` pipes to a handler (the systemd / apport default), the **kernel ignores `RLIMIT_CORE`** and hands the core to the handler regardless; the handler then applies its own policy. So `RLIMIT_CORE = 0` alone is *not* sufficient on a systemd box — you also need `PR_SET_DUMPABLE = 0` / `MADV_DONTDUMP`. ([systemd.io/COREDUMP](https://systemd.io/COREDUMP/))
- **CVE-2025-5054 (apport) and CVE-2025-4598 (systemd-coredump)**: race-condition local information-disclosure flaws (CVSS ~4.7) where a crashing SUID program can leak memory (e.g. `/etc/shadow` hashes) to the coredump handler. Discovered by Qualys TRU. Mitigation: run non-SUID and/or `sysctl fs.suid_dumpable=0`. ([Qualys TRU](https://blog.qualys.com/vulnerabilities-threat-research/2025/05/29/qualys-tru-discovers-two-local-information-disclosure-vulnerabilities-in-apport-and-systemd-coredump-cve-2025-5054-and-cve-2025-4598), [Ubuntu advisory](https://ubuntu.com/blog/apport-local-information-disclosure-vulnerability-fixes-available))

### Anti-tracing
- `secmem-proc` hardens a process by disabling core dumps and ptrace across Linux/FreeBSD/macOS/Windows and includes basic debugger detection. Its README is blunt that it only makes attacks *harder* and "can by no means promise any security." Treat it as defense-in-depth, not a guarantee. ([secmem-proc GitHub](https://github.com/niluxv/secmem-proc), [docs.rs/secmem-proc](https://docs.rs/secmem-proc/))

### Hardware / out-of-scope limits
- `mlock`/`mlockall` keep pages out of *swap* but do **not** protect against firmware- or hardware-level RAM capture: suspend-to-disk (S4) writes RAM to disk, and cold-boot attacks read DRAM directly. ([mlockall(2)](https://man7.org/linux/man-pages/man2/mlockall.2.html))
- Spectre/Meltdown-class transient-execution channels can in principle read data before it is zeroized; there is no practical pure-userspace Rust mitigation. Mitigation is CPU microcode + kernel mitigations, optional memory encryption (Intel TME / AMD SME), and process/CPU isolation. ([zeroize docs note this limitation](https://docs.rs/zeroize/latest/zeroize/))

---

## Current versions / APIs (verified 2026-06-02)

| Crate | Latest | Released | Relevant API | Notes |
|---|---|---|---|---|
| [`zeroize`](https://docs.rs/crate/zeroize/latest) | **1.8.2** | (current) | `Zeroize`, `ZeroizeOnDrop`, `Zeroizing<T>` | MSRV 1.72; pure Rust, no FFI. |
| [`secrecy`](https://docs.rs/crate/secrecy/latest) | **0.10.3** | 2024-10-09 | `SecretBox<T>`, `SecretString`, `ExposeSecret` | `#![forbid(unsafe_code)]`; **no** mlock. |
| [`nix`](https://docs.rs/crate/nix/latest) | **0.31.3** | **2026-05-11** | `nix::sys::mman::{mlockall, MlockAllFlags, madvise, MmapAdvise}` | MSRV 1.69; "May 2026" timing now **confirmed**. |
| [`rustix`](https://docs.rs/rustix/latest/rustix/mm/index.html) | latest | (current) | `rustix::mm::{mlockall, MlockAllFlags, madvise, Advice}` | Safe wrappers; `linux_raw` backend default (no libc needed). |
| [`prctl`](https://docs.rs/prctl/latest/prctl/) | **1.0.0** | (current) | `prctl::set_dumpable(bool) -> Result<(), i32>` | Thin Linux `prctl(2)` wrapper. |
| [`secmem-proc`](https://github.com/niluxv/secmem-proc) | latest | (current) | `secmem_proc::harden_process()` | Cross-platform; bundles dump+ptrace hardening + debugger detection. |
| [`os-memlock`](https://github.com/thatnewyorker/os-memlock) | latest | (current) | `mlock`, `munlock`, `madvise_dontdump` | Audit-friendly thin `unsafe` wrappers; zeroizes before `munlock`; FreeBSD `MADV_NOCORE`. |

> Earlier drafts of this research recorded "rustix does not document mlock/madvise" and treated nix 0.31.3 as unconfirmed — both were stale. As of 2026-06-02, `rustix::mm` documents safe `mlockall`/`madvise`, and `nix` 0.31.3 is live on docs.rs with a **2026-05-11** release date.

---

## Security tradeoffs

| Control | Buys you | Costs / gotchas |
|---|---|---|
| `zeroize` on drop | Plaintext bytes overwritten deterministically; portable | Cannot stop side-channel reads before drop; no protection while value is live in registers/cache |
| `mlockall` | Secrets never swapped to disk | Needs raised `RLIMIT_MEMLOCK` (default 64 KiB); locks lost across `fork`; page-granular; can pin large RSS |
| `PR_SET_DUMPABLE=0` | No core dump + blocks non-root `PTRACE_ATTACH` | Also blocks *legitimate* debugging/`gdb` attach during dev; loses ability to read `/proc/self/...` as a different uid |
| `MADV_DONTDUMP` | Per-buffer dump exclusion, finer than process-wide | Linux/FreeBSD only; no-op/unsupported on macOS |
| `RLIMIT_CORE=0` | Cheap, standard | **Ignored** when `core_pattern` pipes to a handler (systemd default) |
| `secmem-proc` | One call, cross-platform, debugger detection | "Hardening, not security" per its own README; detection is best-effort |
| Memory encryption (TME/SME) | Mitigates cold-boot/DRAM capture | Platform/BIOS dependent; not a userspace decision |

---

## Concrete guidance for the env-ctl implementation

**Startup ordering (do this before any secret enters memory):**

```rust
// 1. Resource limits — must precede loading any key material.
//    Set core size to 0 and memlock high enough for the locked working set.
setrlimit(Resource::CORE, 0, 0)?;                        // best-effort; see core_pattern caveat
setrlimit(Resource::MEMLOCK, want_bytes, want_bytes)?;   // > total secret RSS + headroom

// 2. Process hardening: no core dumps, no ptrace attach, debugger sniff.
secmem_proc::harden_process();                           // wraps PR_SET_DUMPABLE(0) etc.

// 3. Lock all pages, current and future.
rustix::mm::mlockall(MlockAllFlags::CURRENT | MlockAllFlags::FUTURE)?;
//    (or nix::sys::mman::mlockall(MlockAllFlags::MCL_CURRENT | MCL_FUTURE)?)
```

**Per-secret handling (USB unlock key, derived data-key, relay bearers, decrypted creds):**

```rust
let secret = SecretBox::new(Box::new(key_material));      // zeroized on drop (secrecy + zeroize)
os_memlock::madvise_dontdump(secret.as_bytes())?;         // exclude from any core dump
// Now: zeroized-on-drop + (process-wide) mlock'd + dump-excluded.
```

**env-ctl-specific points:**

- **USB-unlock timing is the critical window.** Run steps 1–3 *before* the USB-partition-UUID unlock path derives or reads any key material. If unlock runs before `mlockall`, the freshly read key can be paged out before it is ever locked. Gate the unlock RPC handler so it is unreachable until hardening completed. *(This window was not addressed in the source research — call it out in the design.)*
- **Bearer lifetime.** The ≤24 h USB-gated relay bearers should live in `SecretString`/`SecretBox` and be dropped (zeroized) the instant they expire or the USB token is removed — do not rely on process exit to clear them.
- **gRPC/relay threads must not `fork`.** The data-plane HTTPS relay and the UDS control plane should spawn subprocesses (if any) via `posix_spawn`/`Command` (which `execve`s) — never bare `fork`, because `mlockall` locks are not inherited and CoW pages defeat the lock. Audit any library in the relay path that might `fork`.
- **Run the daemon non-SUID.** Given CVE-2025-5054 / CVE-2025-4598, avoid the SUID coredump leak class entirely; also set `fs.suid_dumpable=0` as belt-and-suspenders on the deployment host.
- **Page isolation for the master/data keys.** Place the long-lived unlock key and derived data-key in their own page(s) (e.g. a dedicated locked allocation) so per-buffer `MADV_DONTDUMP` and lifetime control are not entangled with short-lived buffers.
- **Capability vs limit.** Decide deployment posture: either grant the daemon `CAP_IPC_LOCK` (clean, no `RLIMIT_MEMLOCK` math) or raise `RLIMIT_MEMLOCK` via systemd unit (`LimitMEMLOCK=`) sized to the locked working set. Prefer the systemd-unit route to keep the daemon unprivileged.
- **Disk encryption is part of this control.** Because `mlock` cannot stop suspend-to-disk RAM capture, env-ctl's threat model should require full-disk encryption (LUKS) on the host and, ideally, disable hibernation (S4) where the vault is unlocked.

---

## Open questions (flagged — not verified)

1. **Ubuntu 26.04 kernel + sysctl defaults.** Could not confirm the shipped kernel version, the default `kernel.yama.ptrace_scope`, or `kernel.perf_event_paranoid` for 26.04. These materially affect how much extra ptrace hardening env-ctl must do itself. **[UNVERIFIED]**
2. **systemd `LimitMEMLOCK` interaction on 26.04.** Verify the exact unit directive and whether 26.04's systemd default memlock differs from the 64 KiB kernel default. **[UNVERIFIED]**
3. **secrecy `mlock` feature.** Issue #480 is still open; if env-ctl wants memory-locking *inside* the secret wrapper rather than process-wide `mlockall`, it must implement it (e.g. via `os-memlock`) — secrecy will not provide it short-term. ([#480](https://github.com/iqlusioninc/crates/issues/480))
4. **`rustix` vs `nix` choice for MSRV.** Both expose safe `mlockall`/`madvise`; pick based on env-ctl's target MSRV and whether you already pull `libc` (rustix's `linux_raw` backend avoids it). Confirm against the project's pinned toolchain.
5. **Spectre/Meltdown residual risk.** No practical userspace mitigation exists; whether env-ctl's threat model (local box, single operator) needs memory encryption (Intel TME / AMD SME) is a design call, not a code fix. **[partially verified — limitation confirmed, mitigation efficacy not benchmarked]**
6. **Debugger-detection efficacy.** `secmem-proc`'s detection is best-effort and trivially bypassable by a root attacker; do not count it against a root-level threat. ([secmem-proc README](https://github.com/niluxv/secmem-proc))
