# env-ctl research — USB key detection by PARTUUID on Linux

> Scope: detecting presence/absence of the unlock-USB by partition UUID on Ubuntu 26.04, and turning that signal into a real cryptographic gate for env-ctl's vault unlock + relay-bearer lifecycle. Verified against live sources as of **June 2026**. Assistant knowledge cutoff was Jan 2026; every version/API claim below was re-checked on the web and dated.

---

## TL;DR — recommendation for env-ctl

**PARTUUID is a pre-filter, not the lock.** Treat the partition UUID as a fast, deterministic "is the right USB plugged in?" *hint*. The actual unlock gate must be **possession of the keyfile material on that partition**, proven cryptographically (keyslot unwrap or keyed-MAC match) — never "the UUID matched, therefore unlock." This matches env-ctl CF-4 ("UsbPresent PROVES keyfile possession cryptographically").

Concrete asks for the implementation (Phase 2):

1. **Poll `/dev/disk/by-partuuid/<stored_partuuid>` as the authoritative mechanism.** It is a kernel/udev-maintained symlink, pure filesystem access, zero mandatory C dependency. Poll at startup and on an interval. See [detection mechanisms](#key-facts-with-inline-source-urls).
2. **Pin a GPT PARTUUID, reject FS-UUID and reject MBR pseudo-PARTUUIDs** (env-ctl OI-5). GPT PARTUUIDs are stable random 128-bit GUIDs; MBR pseudo-PARTUUIDs (`SSSSSSSS-PP`) are derived from the disk signature + partition number and **change if the partition number changes** — too weak/fragile to bind to.
3. **Possession proof, not name match:** read the keyfile into `Zeroizing<Vec<u8>>`, then either (a) attempt a LUKS keyslot unwrap, or (b) compute `keyed-BLAKE3(mac_key, keyfile)` and compare to a vault-resident MAC stored under the DEK. The UUID only tells you *which* device to read.
4. **Optional udev acceleration, off by default** (env-ctl R8): spawn a `udev::MonitorBuilder` listener on the `block` subsystem to *nudge* a re-poll on hotplug events. Linking `libudev` (C) is acceptable **only** because it is non-mandatory; polling alone must always work.
5. **On removal → drain, don't kill.** USB-gone (removal event or absent-from-poll) starts the relay-bearer drain grace, then refuses new egress and re-locks. Exact grace window is **OI-4 (open)**.
6. **Harden the clock** that bounds bearers/relock against rollback (OI-6): monotonic floor + `CLOCK_BOOTTIME` cross-check.

You do **not** need, and should not add, a `udev`/`libudev` crate as a *required* dependency. Polling `/dev/disk/by-partuuid/` plus an optional `blkid` shell-out or `libblkid` link covers detection.

---

## Key facts (with inline source URLs)

### How PARTUUID is exposed on Linux

- **`/dev/disk/by-partuuid/` symlinks are created by udev** as part of persistent block-device naming. Each partition with a recognizable partition-table UUID gets a stable symlink there, pointing at the kernel device node (e.g. `../../sda3`). This directory is the canonical lookup surface. ([wiki.archlinux.org/title/Persistent_block_device_naming](https://wiki.archlinux.org/title/Persistent_block_device_naming))
- **`blkid -s PARTUUID -o value /dev/sdX3`** prints just the PARTUUID. `-s/--match-tag` filters to the named tag; `-o value` prints values without tag names, so the output is directly parseable. ([man7.org/linux/man-pages/man8/blkid.8.html](https://man7.org/linux/man-pages/man8/blkid.8.html))
- **PARTUUID vs UUID is a real distinction.** UUID is the *filesystem* UUID (changes on reformat); PARTUUID is the *partition-table* UUID (survives reformat, lives in the GPT/MBR table). For a "this is the right physical key" check, PARTUUID is the correct one to bind. ([wiki.archlinux.org/title/Persistent_block_device_naming](https://wiki.archlinux.org/title/Persistent_block_device_naming))

### GPT vs MBR PARTUUID format (the OI-5 distinction)

- **GPT PARTUUID** is a true 128-bit GUID stored per-partition in the GPT header — stable, random, unique. This is what env-ctl should pin. ([wiki.archlinux.org/title/Persistent_block_device_naming](https://wiki.archlinux.org/title/Persistent_block_device_naming), [en.wikipedia.org/wiki/GUID_Partition_Table](https://en.wikipedia.org/wiki/GUID_Partition_Table))
- **MBR/DOS has no native partition UUIDs.** libblkid synthesizes a *pseudo*-PARTUUID of the form **`SSSSSSSS-PP`**, where `SSSSSSSS` is the zero-filled 32-bit MBR **disk signature** (byte offset 440) and `PP` is the zero-filled partition number in hex. Example: disk sig `076c4a2a`, partition 3 → `076c4a2a-03`. **Crucially, this pseudo-PARTUUID changes if the partition number changes**, and the 32-bit disk signature is far weaker than a 128-bit GUID. ([wiki.archlinux.org/title/Persistent_block_device_naming](https://wiki.archlinux.org/title/Persistent_block_device_naming), [unix.com.hr/2025/04/24/partuuid-and-uuid/](https://unix.com.hr/2025/04/24/partuuid-and-uuid/))

### Hotplug / event-driven detection

- **udev hotplug rides `NETLINK_KOBJECT_UEVENT`.** The kernel broadcasts uevents on add/remove/change; `systemd-udevd` receives them directly from the kernel and re-broadcasts processed events to userspace listeners. This is the mechanism behind any event-driven (vs polling) detection. ([man7.org/linux/man-pages/man7/udev.7.html](https://man7.org/linux/man-pages/man7/udev.7.html))
- **At `ACTION=="remove"`, live sysfs attributes are gone** — the sysfs node has been torn down, so `ATTR{}`/`ATTRS{}` matches don't work. Device identity at removal must be matched via **environment variables/properties** that were set during the earlier `add` (e.g. `ENV{ID_*}`), which the kernel/udev carry in the event. Practical consequence: do not expect to re-read PARTUUID from sysfs on the removal event; correlate by the properties present in the event. ([man7.org/linux/man-pages/man7/udev.7.html](https://man7.org/linux/man-pages/man7/udev.7.html), [reactivated.net/writing_udev_rules.html](https://www.reactivated.net/writing_udev_rules.html))
- This is exactly why **polling is the more robust authoritative signal** for env-ctl: a poll just asks "does `/dev/disk/by-partuuid/<uuid>` resolve right now?" with no dependence on event ordering, attribute availability, or a long-lived netlink socket.

### Turning presence into a cryptographic gate

- **LUKS `cryptsetup luksOpen` accepts a binary keyfile** via `--key-file`; the keyfile is read up to the compiled-in max size, **newlines do not terminate input** (so arbitrary binary keys work), and `--keyfile-size` bounds how many bytes are consumed. This is the canonical path if env-ctl backs USB unlock with a LUKS keyslot rather than (or in addition to) an app-layer keyslot. ([man7.org/linux/man-pages/man8/cryptsetup.8.html](https://man7.org/linux/man-pages/man8/cryptsetup.8.html))
- **Keyed-MAC alternative:** BLAKE3 exposes `keyed_hash(key: &[u8;32], data)` as a PRF/MAC. env-ctl can store `keyed-BLAKE3(mac_key, keyfile)` inside the vault (under the DEK) and re-verify it at unlock; a UUID match with a MAC mismatch is a spoofed/cloned device. ([docs.rs/blake3/latest/blake3/fn.keyed_hash.html](https://docs.rs/blake3/latest/blake3/fn.keyed_hash.html))
- **Key-derivation for the USB-KEK** uses HKDF-SHA256 (RFC 5869), with a domain-separated `info` parameter so the USB-derived KEK is cryptographically distinct from the passphrase-derived KEK; both wrap the same DEK. ([docs.rs/hkdf/latest/hkdf/struct.Hkdf.html](https://docs.rs/hkdf/latest/hkdf/struct.Hkdf.html), [datatracker.ietf.org/doc/html/rfc5869](https://datatracker.ietf.org/doc/html/rfc5869))

---

## Current versions / APIs (re-verified June 2026)

| Component | Current version | Notes / source |
|---|---|---|
| **`udev` crate** (Rust, optional) | **0.9.3** (published 2025-01-23) | Provides `MonitorBuilder` ("Monitors for device events") + `MonitorSocket` ("active monitor that can receive events") + `MonitorSocketIter`. Pure-Rust bindings over the C `libudev`. ([docs.rs/udev/0.9.3](https://docs.rs/udev/0.9.3/udev/), [crates.io/crates/udev](https://crates.io/crates/udev)) |
| **`libudev` crate** (Rust) | **0.3.0** (published **2021-01-17**) | ⚠️ **Effectively unmaintained** (~3.5 yrs since last release). Do **not** confuse "0.3.0 Rust crate" with the C `libudev` library's own version. Prefer the `udev` crate (0.9.3) if you link udev at all. ([crates.io/crates/libudev](https://crates.io/crates/libudev)) |
| **`blake3` crate** | **1.8.5** (latest stable) | env-ctl pins **1.5** in Cargo.toml; `keyed_hash` API is stable across 1.x, so the pin is fine — just note 1.8.5 is current upstream. ([crates.io/crates/blake3](https://crates.io/crates/blake3), [docs.rs/blake3/latest](https://docs.rs/blake3/latest/blake3/fn.keyed_hash.html)) |
| **`hkdf` crate** | **0.12.x** | env-ctl pins `0.12`. `extract()`/`expand()` + domain-separated `info`, RFC 5869. ([docs.rs/hkdf/latest](https://docs.rs/hkdf/latest/hkdf/struct.Hkdf.html)) |
| **`libblkid`** (util-linux) | PARTUUID/partition probing present since ≥ **BLKID_2.36** symver | Symbols `blkid_partition_get_uuid`, `blkid_probe_get_partitions`, `blkid_partlist_*`, `blkid_parttable_get_id` confirmed in current `libblkid.sym`; version stanzas run through **BLKID_2_40**. ([github.com/util-linux/util-linux — libblkid/src/libblkid.sym](https://raw.githubusercontent.com/util-linux/util-linux/master/libblkid/src/libblkid.sym)) |
| **`blkid(8)` CLI** | util-linux | `-s PARTUUID -o value` parseable extraction confirmed. ([man7.org/linux/man-pages/man8/blkid.8.html](https://man7.org/linux/man-pages/man8/blkid.8.html)) |
| **`cryptsetup`** | 2.x | `luksOpen --key-file` binary keyfile + `--keyfile-size`. ([man7.org/linux/man-pages/man8/cryptsetup.8.html](https://man7.org/linux/man-pages/man8/cryptsetup.8.html)) |

> **Correction logged:** an earlier research pass claimed a "libudev Rust crate 0.13.0" and "blake3 1.8.2" — both wrong. The Rust `libudev` crate tops out at **0.3.0** (0.13.x refers to the *C* library, not the crate); current `blake3` is **1.8.5**. Use this table.

---

## Security tradeoffs

- **PARTUUID is not a secret and not unforgeable.** GPT PARTUUIDs are readable by anyone with the device and are trivially clonable: an attacker who images the USB (or just writes the same GUID into a GPT) produces a device that passes the name check. Therefore **UUID match ≠ authorization.** This is why CF-4 demands a cryptographic possession proof on top. ([wiki.archlinux.org/title/Persistent_block_device_naming](https://wiki.archlinux.org/title/Persistent_block_device_naming))
- **PARTUUID persists across detach/reattach** and across reformat of the filesystem (it lives in the partition table, not the FS). Good for stability, but it also means a *cloned* device reattaching is indistinguishable by UUID alone — only the keyfile/keyslot possession proof distinguishes the genuine key. (Tracked as a threat consideration — THREAT-MODEL A5.) ([wiki.archlinux.org/title/Persistent_block_device_naming](https://wiki.archlinux.org/title/Persistent_block_device_naming))
- **MBR pseudo-PARTUUID is brittle and weaker:** only 32 bits of disk signature entropy and it changes when the partition number changes — pinning to it risks both false-negatives (re-partition) and weaker uniqueness. Reject it (OI-5). ([unix.com.hr/2025/04/24/partuuid-and-uuid/](https://unix.com.hr/2025/04/24/partuuid-and-uuid/))
- **Polling vs udev-events tradeoff:** events are lower-latency but (a) require a netlink listener / `libudev` C dependency, (b) lose live sysfs attributes at removal (must correlate by carried ENV properties), and (c) can be missed if the daemon restarts mid-event. Polling is slightly higher-latency but dependency-free and self-healing — the right *authoritative* choice; events are an *optimization*. ([man7.org/linux/man-pages/man7/udev.7.html](https://man7.org/linux/man-pages/man7/udev.7.html))
- **Keyfile handling is the real attack surface:** the keyfile bytes are high-value. Read into `Zeroizing<Vec<u8>>`, never log, never persist, and zero promptly after deriving the KEK / computing the MAC. A keyfile (vs a passphrase) trades "something you know" for "something you have" — strong against shoulder-surfing/keylogging, weak against physical theft of the USB. Pair with the passphrase keyslot for two-factor.
- **Time-of-check / time-of-use:** a USB present at the *poll* may be yanked microseconds later. Don't gate a long operation on a single past poll; gate egress at the *moment* of egress and treat the relay-bearer lifetime (≤24h, USB-gated) as the bound. Harden the clock against rollback (OI-6).

---

## Concrete guidance for the env-ctl implementation

**Detection module (`usb`/`presence`), Phase 2:**

1. **Authoritative poll.** Store the pinned **GPT** PARTUUID in vault config. On startup and on a timer, `stat`/`readlink` `/dev/disk/by-partuuid/<uuid>`; resolve to the canonical device node. No C dependency on this path.
2. **Validate it's GPT, not MBR.** Reject any pinned value matching the `SSSSSSSS-PP` MBR pseudo shape; require a full 128-bit GUID (OI-5). Optionally cross-check via `libblkid` `blkid_parttable_get_type == "gpt"` if you link libblkid.
3. **Possession proof (the gate).** Read the on-USB keyfile into `Zeroizing<Vec<u8>>`. Then **either**:
   - LUKS path: `cryptsetup luksOpen --key-file <keyfile> --keyfile-size N`, **or**
   - App-layer path: `keyed-BLAKE3(mac_key, keyfile)` compared (constant-time) against the vault-resident MAC; on match, `HKDF-SHA256(extract/expand, info="env-ctl/usb-kek/v1")` → USB-KEK → unwrap DEK.
   - **Never** unlock on UUID match alone.
4. **Optional udev accelerator (off by default, R8).** Behind a cargo feature / runtime flag, spawn `udev::MonitorBuilder` filtered to the `block` subsystem; on any `add`/`remove`/`change`, trigger an immediate re-poll. At `remove`, match the device by carried `ENV{ID_*}` properties (sysfs is gone), then fall through to the poll for authoritative truth.
5. **Removal → drain.** On detected absence (poll miss or removal event), start the relay-bearer **drain grace** (length TBD, OI-4), refuse new egress immediately after grace, and re-lock the DEK in memory once the USB is confirmed gone. Re-presentation of a USB must re-run the full possession proof, not just re-match the UUID.
6. **Clock hardening (OI-6).** Bound bearer TTLs and the drain window using a monotonic floor cross-checked against `CLOCK_BOOTTIME` so a wall-clock rollback can't extend a ≤24h USB-gated bearer.

**Dependency posture:** keep detection pure-Rust (filesystem poll). If you link C, prefer `udev = "0.9"` (maintained) over `libudev = "0.3"` (stale), and keep it feature-gated so the no-mandatory-C tenet holds (CF-2 / R8). CI should continue to gate against unexpected `*-sys` linkage.

---

## Open questions

1. **Auto-relock / drain grace window (OI-4).** Not finalized. A "~5 min" drain has been floated but not adopted; the ≤24h bearer cap is the hard outer bound. **Open.**
2. **Possession-proof concretization.** LUKS-keyslot unwrap *vs* vault-resident keyed-BLAKE3 MAC (or both) is Phase-2 scope — design is settled (CF-4), code is pending. **Open (impl).**
3. **`blkid` Rust crate.** No maintained, current Rust `blkid` wrapper was confirmed; env-ctl's Cargo.toml does not list one. Plan of record: poll `/dev/disk/by-partuuid/` (pure Rust) and, if richer probing is needed, shell out to `blkid(8)` or link `libblkid` directly rather than depend on an unvetted crate. **Open (tooling choice).**
4. **Multi-USB / key-rotation UX.** How env-ctl enrolls a *second* USB (backup key) and revokes a lost one — distinct PARTUUIDs, distinct keyslots — is not yet specified. **Open.**
5. **Cloned-device detection beyond MAC.** A perfect bit-for-bit clone passes both UUID and keyfile-MAC checks; defending against that needs hardware-bound secrets (e.g., a security-key/TPM-sealed factor), which is out of current scope. **Open (threat A5).**

---

### Could-not-verify flags

- **Arch Wiki page (Persistent block device naming)** was intermittently served behind an Anubis anti-bot challenge during this pass; the PARTUUID/MBR-format claims attributed to it were corroborated via independent sources ([unix.com.hr](https://unix.com.hr/2025/04/24/partuuid-and-uuid/), [Wikipedia GPT](https://en.wikipedia.org/wiki/GUID_Partition_Table)) and the `man7` udev/blkid pages, which loaded cleanly. Treat the Arch Wiki citations as corroborated-but-mirror-verified.
- **Exact `libblkid` version that first shipped a given PARTUUID symbol:** the symbol-file confirms partition/PARTUUID functions exist and that version stanzas run BLKID_2.15 → BLKID_2_40, but mapping each individual symbol to its introducing release was not done per-symbol; "≥2.36 has full PARTUUID support" is asserted at the granularity of the stanza set, not per-function.
