# env-ctl research — Secrets-at-rest on a VPS + remote unlock

> Scope: how env-ctl's local-box secrets vault + credential broker should behave when the daemon
> runs on an **untrusted-hypervisor VPS** instead of the operator's physical box. Focus: at-rest
> protection of `vault.db`, and the **remote-unlock** problem (no USB to insert on a headless cloud
> host). Verified against the live web (June 2026) and the env-ctl design docs in
> `/home/drdave/Desktop/env-ctl/docs/`.
>
> Conventions: ✓ verified · ⚠️ verified-but-nuanced/caveated · ❌ refuted/unverifiable.
> "Cutoff" notes flag anything the assistant knowledge cutoff (Jan 2026) could not confirm against a live page.

---

## TL;DR — recommendation for env-ctl

env-ctl's core security argument is **physical**: the unlock key lives on a USB partition the operator
can physically remove, draining all relay bearers within 24h. **A VPS has no USB slot the operator
controls**, so the moment you move the daemon to a cloud host you have to replace "operator yanks the
stick" with some remote-unlock primitive — and every such primitive trusts something env-ctl currently
does not (the hypervisor, a cloud KMS, or a network path to the operator box).

Ranked for env-ctl, by real-world deployability against an **untrusted hypervisor**:

1. **Keep the daemon on the operator's physical box; expose only the data-plane relay to the VPS.**
   This preserves the USB-pull guarantee end-to-end and is the smallest deviation from the current
   design. The VPS holds *no* DEK — it is a relay client holding a `<=24h` peer-bound bearer. Strongly
   preferred when feasible.
2. **AWS Nitro Enclave + attestation-gated KMS** (AWS-only). The hypervisor *cannot* read enclave
   memory (hardware boundary, not a vTPM), and the SEV-SNP "Heracles" page-move attack does not apply.
   Requires a real refactor: DEK wrapped under KMS, released only to a measured enclave. Not portable.
3. **Cloud KMS auto-unseal** (AWS/GCP/Azure). Zero operator attendance, audited (e.g. CloudTrail).
   Trades hypervisor-isolation for *provider* trust — a reasonable, explicit downgrade. Network path to
   KMS becomes an availability + attack surface.
4. **Operator-box signs short-lived unlock tokens.** Preserves USB-gating *on the operator side*; the
   VPS requests a non-replayable, VPS-bound, short-TTL unlock token. Requires the operator box to be
   hardened and reachable; network path must be TLS-pinned.
5. **Passphrase-only** (no USB factor). Last resort. Single factor; an attacker with `vault.db` brute-
   forces argon2id offline. Manual unlock on every reboot. Acceptable only for throwaway/test boxes.

**Do not** rely on a **vTPM** for the at-rest key on an untrusted hypervisor (no hardware boundary —
see below), and **do not** use Shamir k-of-n as the unseal path for an unattended 24/7 service (manual
ceremony, timing). AMD SEV-SNP is viable *only if* the provider has deployed the Heracles mitigation
(AMD spec 1.58, May 2025) — verify before depending on it.

This is a "no single best answer" space; the right pick depends on whether you can keep the DEK off the
cloud host at all (option 1) and, if not, which third party you are willing to trust.

---

## Key facts (with inline sources)

### At-rest crypto env-ctl already uses (good as-is)

- **XChaCha20-Poly1305** (crate `chacha20poly1305 = "0.10"`): RFC 8439 AEAD with a 24-byte (extended)
   nonce, integrity-binding per record via the Poly1305 tag; pure Rust. ✓
   <https://docs.rs/chacha20poly1305/latest/chacha20poly1305/> · <https://datatracker.ietf.org/doc/html/rfc8439>
- **Argon2id** (crate `argon2 = "0.5"`), constructed `Argon2::new(Algorithm::Argon2id, Version::V0x13, params)`
   — never `default()` — with env-ctl's floor **m=1 GiB, t=4, p=4**. ✓
   <https://docs.rs/argon2/latest/argon2/>
- ⚠️ **RFC 9106 nuance:** RFC 9106's *frontend-server* recommendation (≈0.5 s on 2 GHz / 2 cores) is
   the **first** recommended option **t=1, p=4, m=2 GiB** (and a **second** option t=3, p=4, m=64 MiB
   for memory-constrained hosts). env-ctl's **m=1 GiB, t=4, p=4** is **not** a copy of the RFC's
   server option — it is deliberately tuned for **offline brute-force resistance against a stolen
   `vault.db`** (THREAT-MODEL A12), which is the right axis for an at-rest vault key, not interactive
   login latency. Treat the env-ctl floor as a *deliberate* choice, not "the RFC value."
   <https://datatracker.ietf.org/doc/rfc9106/>
- **Zeroize** (`zeroize = "1.8"`): `core::ptr::write_volatile` + a compiler/atomic fence to defeat
   dead-store elimination. ✓ Residual (honestly stated in THREAT-MODEL): the **argon2 1 GiB arena**
   and **tonic/hyper receive buffers** holding the passphrase/secret in transit cannot be fully
   zeroized. <https://docs.rs/zeroize/latest/zeroize/>
- **Process hardening** for "key never to disk": `mlockall(MCL_CURRENT|MCL_FUTURE)` + raised
   `RLIMIT_MEMLOCK` + `MADV_DONTDUMP` + `RLIMIT_CORE=0`; daemon refuses to start if `mlockall` fails.
   On a VPS, recommend **no swap or encrypted swap** at install. ✓
   <https://man7.org/linux/man-pages/man2/mlock.2.html>
- **OWASP Top 10:2025** moved *Cryptographic Failures* to **A04** (it was A02 in 2021); category still
   covers weak/missing crypto, leaked keys, and poor implementation — the at-rest design maps to it. ✓
   <https://owasp.org/Top10/2025/A04_2025-Cryptographic_Failures/>

### The VPS threat model (why "untrusted hypervisor" is the crux)

- **VM escape / cross-VM leakage is a live 2025 reality.** "VMScape" (**CVE-2025-40300**) is an
   incomplete branch-predictor-isolation (Spectre-BTI) gap affecting **all AMD Zen** and some Intel
   CPUs — the first demonstrated end-to-end cross-VM leak with no host modification. Linux deployed an
   IBPB-based mitigation (Sept 2025). On shared cloud silicon, "another tenant's VM can leak your
   memory" is no longer hypothetical. ✓
   <https://comsec.ethz.ch/research/microarch/vmscape-exposing-and-exploiting-incomplete-branch-predictor-isolation-in-cloud-environments/>
- **Cold-boot / RAM-remanence attacks** still recover keys from powered-down or rebooted RAM; the
   defense is a second factor (PIN/USB) or continuous in-RAM key locking, not "TPM with no PIN." This
   matters because a VPS DEK lives in host RAM the operator does not physically control. ✓
   <https://en.wikipedia.org/wiki/Cold_boot_attack>
- ⚠️ **vTPM ≠ hardware TPM.** A virtual TPM's isolation rests on the **hypervisor**, not a hardware
   boundary; if the hypervisor is compromised (or the cloud admin is malicious/careless), vTPM-sealed
   keys are at risk. The accurate framing is "**vTPM inherits the hypervisor's trust** — no hardware
   isolation," **not** the over-strong "vTPM gives no additional security" (it still resists *passive/
   accidental* threats). Recent research proposes SGX-sealed vTPMs to close this gap. Do not seal the
   env-ctl DEK to a vTPM on an untrusted host.
   <https://trustedcomputinggroup.org/about/what-is-a-virtual-trusted-platform-module-vtpm/>
- ⚠️ **AMD SEV-SNP is not a blanket fix.** SEV-SNP's threat model treats the hypervisor/host as
   untrusted, but **"Heracles" (CCS 2025)** is a chosen-plaintext attack: a malicious hypervisor moves
   encrypted guest pages in DRAM; because re-encryption is deterministic, it builds an oracle that
   leaks guest memory **including crypto keys and passwords**. The mitigation is to **disable dynamic
   page moves** (AMD spec **1.58**, May 2025). SEV-SNP is acceptable for env-ctl **only on a provider
   that has deployed spec 1.58+** — which adds a provider-patch-level attestation requirement.
   <https://heracles-attack.github.io/> · <https://heracles-attack.github.io/Heracles-CCS2025.pdf> ·
   <https://www.amd.com/content/dam/amd/en/documents/developer/lss-snp-attestation.pdf>

### Remote-unlock primitives (how others solve "no USB on a headless box")

- **AWS Nitro Enclaves attestation → KMS.** The Nitro hypervisor signs an attestation document
   containing the enclave's **PCR** measurements; a KMS key policy can gate `Decrypt`/`GenerateDataKey`
   on PCR condition keys (e.g. `kms:RecipientAttestation:ImageSha384`) and return
   `CiphertextForRecipient` encrypted to the enclave's public key — decryptable **only inside** the
   measured enclave. The parent EC2 instance (and the hypervisor) never sees the plaintext. This is the
   strongest "release a key to a remote box only if it is the code I expect" primitive. ✓
   <https://docs.aws.amazon.com/enclaves/latest/user/kms.html> ·
   <https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/nitrotpm.html>
- **HashiCorp Vault seal/unseal — the canonical prior art.** Default Shamir split is **5 shares,
   threshold 3** (unchanged for years); production deployments overwhelmingly use **auto-unseal**
   (AWS KMS / GCP Cloud KMS / Azure Key Vault / Transit) precisely because the manual Shamir ceremony
   is impractical for unattended restarts. Lesson for env-ctl: a 24/7 service wants auto-unseal, not a
   quorum ritual. ✓
   <https://developer.hashicorp.com/vault/docs/concepts/seal>
- **SPIFFE-style credential broker** (arXiv **2504.14761**, Apr 2025): decouples *identity* (a SPIFFE
   workload ID) from *access* (a policy engine — OPA/Cedar) to mint **scoped, short-lived** credentials
   for CI/CD. This is the closest published pattern to env-ctl's own broker model and validates the
   "mint short-lived, policy-scoped, audited tokens rather than hand out the real key" approach. ✓
   <https://arxiv.org/html/2504.14761v1>
- **Bearer vs sender-constrained tokens.** RFC 6750 bearer tokens are **replayable by design** — anyone
   holding one can use it. RFC 8705 (mTLS) **sender-constrains** a token by binding it to the client's
   certificate, so a stolen token is useless without the matching key. env-ctl already does the moral
   equivalent ("**peer-bound** bearer"); if a relay bearer ever crosses a network to a VPS, prefer an
   explicitly sender-constrained binding over a plain bearer. ✓ (RFC numbers verified; the linked
   write-up is a community explainer — verify against the RFCs directly.)
   <https://datatracker.ietf.org/doc/html/rfc6750> · <https://datatracker.ietf.org/doc/html/rfc8705>

### Pure-Rust store alternative (relevant to OI-1, amplified by the VPS context)

- **`redb`** is a pure-Rust embedded **ACID** key-value store, **1.0 stable since June 2023** — a viable
   no-C backend for env-ctl's encrypted records, expressing `db/schema.sql` as keyspaces. This matters
   doubly on a VPS, where minimizing the native/C attack surface (`libsql-ffi` bundles an 8.9 MB C
   `sqlite3.c`) is even more valuable. ✓
   <https://github.com/cberner/redb>

### Refuted / could-not-verify

- ❌ **"Over 90% of hardcoded GitHub secrets stay valid 5 days after disclosure."** Could not verify;
   no such "90% in 5 days" figure appears in GitGuardian's reporting. The verifiable story is a
   **long-tail validity** problem: GitGuardian's *State of Secrets Sprawl 2026* reports a large jump in
   leaked secrets (tens of millions in 2025, a ~34% YoY rise) and that a majority of secrets confirmed
   valid in 2022 were **still exploitable years later**. Cite the long-tail framing, not the 5-day stat.
   ⚠️ <https://blog.gitguardian.com/the-state-of-secrets-sprawl-2026/>

---

## Current versions / APIs (verified against env-ctl `Cargo.toml`, June 2026)

| Component | Pin in repo | Notes |
|---|---|---|
| `chacha20poly1305` | `0.10` | XChaCha20-Poly1305 AEAD, pure Rust ✓ |
| `argon2` | `0.5` | Argon2id, `Version::V0x13`; floor m=1 GiB/t=4/p=4 ✓ |
| `zeroize` | `1.8` (+`derive`) | volatile write + fence; arena/buffers are residuals ✓ |
| `rustls` | `0.23`, `default-features=false`, features `["ring","logging","std","tls12"]` | **ring** backend only (NOT aws-lc-rs) ✓ |
| `rcgen` | `0.13`, `default-features=false`, `["ring","pem"]` | local-CA leaf minting, ring path ✓ |
| `webpki-roots` | `0.26` | **frozen Mozilla** roots for upstream egress; never OS store / never local CA ✓ |
| `reqwest` | `0.12`, `default-features=false`, `["rustls-tls","http2","stream"]` | egress client, no native-tls ✓ |
| `tonic` / `tonic-build` | `0.12` | gRPC control plane over UDS ✓ |
| `hyper` / `hyper-util` | `1.5` / `0.1` | HTTPS relay data plane ✓ |
| `tokio` | `1.43` | async confined to `secretd` ✓ |
| store backend | **none pinned** — `inmem-store` test feature; CI gate `! cargo tree | grep libsql-ffi` | **OI-1 OPEN/blocking**: libSQL bundles C SQLite ✓ |

Cloud APIs to target if you adopt remote-unlock (verify call signatures at integration time, they
evolve): AWS KMS `Decrypt`/`GenerateDataKey` with `Recipient` + `kms:RecipientAttestation:*` condition
keys; Nitro `/dev/nsm` attestation document; GCP Cloud KMS `decrypt`/`encrypt`; Azure Key Vault
`unwrapKey`. (Cutoff: treat exact parameter names as "verify-at-build" — they were checked against
2025 AWS docs above but cloud APIs drift.)

---

## Security tradeoffs (the honest matrix)

| Remote-unlock method | Hypervisor untrusted? | Heracles (SEV-SNP page-move) applies? | DEK ever on VPS? | Fit for env-ctl |
|---|---|---|---|---|
| **Daemon stays on operator box; VPS is relay-only** | N/A (no key on VPS) | No | **No** | **Best** — preserves USB-pull end-to-end |
| **Nitro Enclave + attested KMS** | Defended (hardware boundary) | No (enclave isolated from DRAM page moves) | Only inside enclave | Strong, **AWS-only**, needs refactor |
| **Cloud KMS auto-unseal** | Accepted (trust provider KMS) | No (key in cloud) | Briefly, in host RAM | Good pragmatic 2nd; network = availability + attack surface |
| **Operator-box signs short-TTL unlock token** | Defended (key derivation on operator box) | No | Briefly, in host RAM | Good on-prem; network-dependent; needs replay-proof binding |
| **AMD SEV-SNP confidential VM** | Claimed; **unpatched = vulnerable** | **Yes** unless spec 1.58+ deployed | In encrypted guest RAM | Only with verified provider patch level |
| **vTPM-sealed DEK** | **Not defended** (no hardware boundary) | n/a | In host RAM | **Avoid** on untrusted hypervisor |
| **Shamir k-of-n unseal** | depends on share custody | n/a | Reconstructed in RAM | **Not for unattended services** (manual ceremony) |
| **Passphrase-only** | Weak (1-of-2 downgrade) | n/a | In host RAM | Last resort / test boxes only |

Cross-cutting tradeoffs:

- **Hypervisor trust vs availability.** Options that keep the hypervisor out of the TCB (Nitro Enclave,
   operator-box-only) cost you portability or add a network dependency. Cloud KMS buys hands-off uptime
   by *adding* the provider to your TCB.
- **The 1-of-2 downgrade is intrinsic, not a bug.** env-ctl's USB-or-passphrase unlock is **OR**, so an
   attacker who can induce "USB absent" forces the weaker passphrase factor (THREAT-MODEL **A12**). On a
   VPS there is **no USB factor at all**, so you are *permanently* in the passphrase (or KMS) regime
   unless you adopt enclave/operator-box unlock. The optional **require-both** keyslot
   (`KEK=KDF(usb_keyfile || passphrase)`) does not help a host with no USB.
- **Owner-session malware (A2) is bounded, not prevented**, on any host — and a VPS *widens* the owner
   session (cloud agents, snapshot/console access, provider support staff). Blast-radius controls (24h
   peer-bound bearers, host/path/method allowlists, quotas, durable audit, USB-pull drain) still apply,
   but the "pull the USB" backstop is gone unless option 1/2/4.
- **Clock rollback (OI-6).** The monotonic issuance floor lives in the **owner-writable DB**, so it
   resists *accidental* skew + external disk attackers but **not** owner-session malware. A VPS adds
   hypervisor-controlled time as a new rollback surface — another reason to keep bearer issuance on the
   operator box.

---

## Concrete guidance for the env-ctl implementation

1. **Default posture: do not run `secretd` on the VPS at all.** Keep the daemon (and the DEK, CA key,
   and real upstream keys) on the operator's physical box; let the VPS workload be a **data-plane relay
   client** holding a `<=24h` peer-bound bearer. This is the only option that preserves env-ctl's
   founding guarantee (`USB absent → new egress denied within the grace window, all relays drained
   within 24h`, FS-S5) without trusting a hypervisor or cloud KMS. Document it as the recommended
   topology.

2. **If `secretd` must run on the VPS, abstract the unlock factor behind the existing keyslot model.**
   Today there are two keyslots: USB (HKDF-SHA256 over a 64-byte CSPRNG keyfile) and passphrase
   (argon2id). Add a **third "remote-release" keyslot type** that wraps the same single DEK, releasing
   the wrapping key via one of: (a) attested KMS (Nitro), (b) cloud KMS, (c) an operator-box-signed
   token. Keep the DEK wrapping/zeroize machinery unchanged so A4/FS-S2 ("vault written only under the
   keyslot KDFs") still holds. Bind the new slot's metadata into `keyslot_aad` exactly like the others
   so a downgrade/add is detected (HF-3/OI-8).

3. **Replace "USB present" gating with a remotely-checkable possession proof on a VPS.** The current
   `UsbPresent` *cryptographically proves* keyfile possession (must unwrap the USB keyslot or match a
   vault-resident keyed MAC; UUID is only a pre-filter — CF-4). On a VPS, the equivalent "operator is
   present and consenting" signal should be a **fresh, short-TTL, VPS-bound, non-replayable token from
   the operator box** (bind to VPS instance ID + nonce + expiry; sender-constrain per RFC 8705 rather
   than a plain RFC 6750 bearer). Gate relay/leaf minting on a currently-valid such token, mirroring the
   USB-gating role. This keeps the **24h drain** semantics: if the operator box stops signing, new
   egress stops within the grace window (OI-4).

4. **Tighten OI-4 (USB-pull / now "operator-token-absent" grace) for cloud network jitter.** The
   proposed ~5 min grace was sized for a human yanking a stick; over a network it must tolerate transient
   outages without (a) failing legitimate long-running egress or (b) extending an attacker's window past
   intent. Make it configurable, default conservative, and **fail-closed on ambiguity**.

5. **Do not seal to a vTPM, and do not depend on SEV-SNP blindly.** If a confidential-VM is on the
   table, prefer **Nitro Enclaves** (hardware enclave boundary; Heracles N/A). If using SEV-SNP, make
   provider **spec-1.58+ (dynamic-page-moves-disabled)** an explicit, attested precondition; otherwise
   treat the host as a plain untrusted hypervisor and use option 1/4.

6. **Resolve OI-1 in favor of the pure-Rust backend before any VPS deploy.** `redb` (pure Rust, 1.0)
   keeps the no-C CI gate green and shrinks the native attack surface — more important on a shared cloud
   host. The canonical logical model stays `db/schema.sql`; `redb` expresses it as keyspaces. (If the
   operator instead waives no-C for libSQL, document that the VPS posture inherits SQLite's C surface.)

7. **CA / MITM on a VPS (OI-13).** The local CA (`rcgen` ring path, `<=90d` auto-renew while unlocked)
   and MITM leaves (`<=min(now+24h, relay validity)`, `relay_id` FK, persist no leaf private keys per
   OI-19) are *more* dangerous on a multi-tenant host. Before VPS deploy, finish the `ca rotate`/revert
   spec so it enumerates **every** trust-store wiring target and fails closed unless the old fingerprint
   is excised everywhere — a stale CA key on a cloud host is a fleet-wide MITM risk.

8. **Egress trust anchors are unchanged and correct.** Upstream egress validates against the **frozen
   Mozilla `webpki-roots` 0.26** bundle, never the OS store and never the local CA. Keep this on a VPS
   (cloud base images carry unpredictable OS trust stores). The local CA is for *interception* only,
   never for upstream verification.

---

## Open questions / flagged uncertainties

- **OI-1 (blocking):** pure-Rust `redb` vs explicit C waiver for libSQL. Must be ruled before a
   production VPS deploy (drives the no-C CI gate and the A4 "no C weak-default surface" claim).
- **Operator-token unlock protocol is unspecified.** Exact binding (VPS instance ID? attested boot
   measurement? mTLS client cert?), TTL, replay window, and what happens during an operator-box outage
   all need design. This is the load-bearing piece for option 4 and is **not yet in the design docs**.
- **OI-4 grace window over a network** — needs tuning against real long-running egress + cloud jitter;
   currently sized for physical USB removal.
- **OI-6 clock rollback on a VPS** — hypervisor-controlled time is a rollback surface the current
   owner-writable monotonic floor does not defend against. Open whether to require an external trusted
   time source (e.g. Roughtime / signed NTP) for bearer acceptance on cloud hosts.
- **Provider patch-level attestation for SEV-SNP** — there is no standard, easy way to *prove* a given
   cloud SEV-SNP host has spec 1.58+ deployed; the attestation report's TCB version must be checked, and
   the operator must trust the provider's reporting. Verify per-provider before relying on SEV-SNP.
- **Cloud API drift (cutoff caveat):** the AWS KMS/Nitro/GCP/Azure call shapes above were checked
   against 2025 docs but should be re-verified at integration time; the assistant's Jan 2026 cutoff
   cannot guarantee the latest parameter names.
- **GitGuardian "5-day / 90%" stat — refuted/unverifiable.** Use the long-tail framing from the
   *State of Secrets Sprawl 2026* report instead; do not cite the 5-day figure.
- **RFC 8705 explainer link** is a community write-up; cite RFC 6750 / RFC 8705 directly for any
   normative claim about bearer vs sender-constrained tokens.

---

### Sources

- chacha20poly1305 crate — <https://docs.rs/chacha20poly1305/latest/chacha20poly1305/>
- RFC 8439 (ChaCha20-Poly1305) — <https://datatracker.ietf.org/doc/html/rfc8439>
- argon2 crate — <https://docs.rs/argon2/latest/argon2/>
- RFC 9106 (Argon2) — <https://datatracker.ietf.org/doc/rfc9106/>
- zeroize crate — <https://docs.rs/zeroize/latest/zeroize/>
- mlock(2) — <https://man7.org/linux/man-pages/man2/mlock.2.html>
- OWASP Top 10:2025 A04 Cryptographic Failures — <https://owasp.org/Top10/2025/A04_2025-Cryptographic_Failures/>
- VMScape (CVE-2025-40300) — <https://comsec.ethz.ch/research/microarch/vmscape-exposing-and-exploiting-incomplete-branch-predictor-isolation-in-cloud-environments/>
- Cold boot attack — <https://en.wikipedia.org/wiki/Cold_boot_attack>
- TCG vTPM — <https://trustedcomputinggroup.org/about/what-is-a-virtual-trusted-platform-module-vtpm/>
- Heracles (CCS 2025) — <https://heracles-attack.github.io/> · <https://heracles-attack.github.io/Heracles-CCS2025.pdf>
- AMD SEV-SNP attestation — <https://www.amd.com/content/dam/amd/en/documents/developer/lss-snp-attestation.pdf>
- AWS Nitro Enclaves + KMS — <https://docs.aws.amazon.com/enclaves/latest/user/kms.html>
- AWS NitroTPM — <https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/nitrotpm.html>
- HashiCorp Vault seal/unseal — <https://developer.hashicorp.com/vault/docs/concepts/seal>
- SPIFFE credential broker (arXiv 2504.14761) — <https://arxiv.org/html/2504.14761v1>
- RFC 6750 (bearer) — <https://datatracker.ietf.org/doc/html/rfc6750>
- RFC 8705 (mTLS sender-constrained) — <https://datatracker.ietf.org/doc/html/rfc8705>
- redb — <https://github.com/cberner/redb>
- GitGuardian State of Secrets Sprawl 2026 — <https://blog.gitguardian.com/the-state-of-secrets-sprawl-2026/>
- env-ctl design docs — `/home/drdave/Desktop/env-ctl/docs/THREAT-MODEL.md`, `/home/drdave/Desktop/env-ctl/docs/DESIGN-NOTES.md`, `/home/drdave/Desktop/env-ctl/Cargo.toml`
