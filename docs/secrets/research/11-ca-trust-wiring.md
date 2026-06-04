# env-ctl research — Per-tool CA trust-store wiring

> Scope: how to make local tooling (curl, git, Node, Python, Rust, plus Snap/Flatpak)
> trust env-ctl's **local MITM CA** for the HTTPS relay data plane, with minimum
> blast radius and clean reversibility. Verified against live docs Feb–Jun 2026.
> Knowledge-cutoff-sensitive version claims are explicitly flagged UNVERIFIED.

---

## TL;DR recommendation for env-ctl

1. **Prefer env-var-per-tool, child-process-only injection.** env-ctl already
   spawns relayed tools; inject the CA path per tool into the child env only.
   This is reversible (process dies → trust gone), auditable, and never touches
   the global system store. Use:
   - `NODE_EXTRA_CA_CERTS` (Node) — additive, safe.
   - `REQUESTS_CA_BUNDLE` (Python `requests`) — documented precedence winner.
   - `GIT_SSL_CAINFO` (git) — overrides `http.sslCAInfo` config.
   - `CURL_CA_BUNDLE` (curl) — documented as supported; prefer it over the
     ambiguous `SSL_CERT_FILE`/`SSL_CERT_DIR` pair.
   - Rust/`reqwest`: no env knob in env-ctl's own code — compile with the
     `rustls` default and load explicit roots (see below). Note `rustls-native-certs`
     *does* honor `SSL_CERT_FILE` first, which is relevant when env-ctl relays a
     Rust binary it did not build.

2. **System bundle = last resort, default OFF.** When a tool can't be reached by
   env var, write **only** a discrete owned file
   `/usr/local/share/ca-certificates/env-ctl-local-mitm-ca.crt`, back up
   `/etc/ssl/certs/ca-certificates.crt` (timestamped) first, then run
   `update-ca-certificates`. Revert = delete the owned file +
   `update-ca-certificates --fresh`. Do **not** hand-edit `/etc/ca-certificates.conf`.

3. **Confinement reality:** Strict **Snap** apps will *not* see the system store
   (sandbox). **Flatpak** apps *will* (p11-kit bridge). Document this limitation
   honestly rather than pretending the system bundle is universal.

4. **Pin, don't poison:** for env-ctl's own Rust relay/clients, prefer explicit
   roots (`webpki-roots` or an explicit PEM) over OS-store discovery, so a
   poisoned/compromised system store cannot widen trust silently.

---

## Key facts (with inline source URLs)

### Node.js
- `NODE_EXTRA_CA_CERTS` and `NODE_USE_SYSTEM_CA` are **additive / complementary** —
  when combined, Node trusts bundled CAs + system CAs + the extra-certs file, with
  "no precedence conflicts."
  <https://nodejs.org/learn/http/enterprise-network-configuration>
- `NODE_EXTRA_CA_CERTS` is **ignored** when Node runs setuid-root or with Linux
  file capabilities (security hardening).
  <https://github.com/nodejs/node/commit/913c4910c7>
- ⚠ The specific version floor for `NODE_USE_SYSTEM_CA` (claimed v22.15.0+ /
  v23.9.0+ / v24.0.0+) is **UNVERIFIED** against live nodejs.org docs, which are
  version-generic. Verify on the changelog before relying on it.

### Python `requests`
- Trust bundle precedence is a documented 3-tier fallback: **`REQUESTS_CA_BUNDLE`
  first → `CURL_CA_BUNDLE` fallback → bundled `certifi`**.
  <https://requests.readthedocs.io/en/latest/user/advanced/>
- **Gotcha:** *Prepared requests* do **not** auto-merge env vars; you must call
  `session.merge_environment_settings()` explicitly, or `REQUESTS_CA_BUNDLE`
  is silently ignored. Same doc, "Prepared Requests" section.
- If `verify` points at a **directory**, it must be `c_rehash`-processed. Same doc.
- ⚠ Behavior when **both** `REQUESTS_CA_BUNDLE` and `CURL_CA_BUNDLE` are set is
  only **partially verified**: docs say former-first/latter-fallback, but no
  source-code audit was done to rule out hidden interaction.

### curl
- curl supports `CURL_CA_BUNDLE`, `SSL_CERT_FILE`, and `SSL_CERT_DIR`.
  <https://curl.se/docs/sslcerts.html>
- ⚠ **Precedence among them is NOT formally documented.** Two prior research
  passes contradicted each other (one claimed `SSL_CERT_FILE` wins when both
  file+dir set; the other admitted no documented order). Treat as **UNVERIFIED** —
  confirm empirically or by auditing `lib/sslcerts.c`. Safe path for env-ctl:
  set **only** `CURL_CA_BUNDLE`.

### OpenSSL
- `SSL_CERT_FILE` and `SSL_CERT_DIR` are recognized, security-sensitive env vars;
  a directory passed via `SSL_CERT_DIR` must be **`c_rehash`-hashed**
  (SHA1-based subject-hash symlink naming — exact algorithm not fully
  documented in the man page, flagged ⚠).
  <https://docs.openssl.org/master/man7/openssl-env/>

### Git
- `GIT_SSL_CAINFO` (env var) **takes precedence** over `git config http.sslCAInfo`;
  the env-var form is the recommended temporary/per-invocation override.
  <https://www.scivision.dev/git-ssl-certificate/>

### Ubuntu system trust store
- Drop a `.crt` into `/usr/local/share/ca-certificates/`, run
  `update-ca-certificates`; it is compiled into
  `/etc/ssl/certs/ca-certificates.crt`. Remove the file + re-run (optionally
  `--fresh`) to revert.
  <https://ubuntu.com/server/docs/how-to/security/install-a-root-ca-certificate-in-the-trust-store/>
- ca-certificates **2.82 (Feb 2026)** added new CAs (incl. Microsoft CA 2023 for
  Secure Boot) and removed obsolete ones. The legacy **Microsoft CA 2011 expires
  June 2026**; not updating blocks package management on 26.04 LTS after ~Q4 2026.
  <https://launchpad.net/ubuntu/+source/ca-certificates/+changelog> ·
  <https://discourse.ubuntu.com/t/microsoft-uefi-ca-rotation-what-it-means-for-ubuntu-users-and-vendors/82652>

### Snap confinement
- Strict snaps are **unlikely to trust** certs in the system store due to
  confinement.
  <https://ubuntu.com/server/docs/how-to/security/install-a-root-ca-certificate-in-the-trust-store/> ·
  <https://forum.snapcraft.io/t/using-the-system-certificate-authorities/10732/>
- ⚠ Newer snapd reportedly mounts host `/etc/ssl` into strict snaps (partial
  mitigation for certs in `/etc/ssl`, **not** `/usr/local/share`). Extent by
  snap/version/path is **not canonically documented** — test on 26.04.

### Flatpak / p11-kit
- Flatpak spawns a **p11-kit** server copy and bind-mounts it into the sandbox, so
  host-trusted anchors (e.g. `/etc/pki/ca-trust/source/anchors/` + `update-ca-trust`)
  are visible inside Flatpaks — fundamentally **more permissive than Snap**.
  <https://datalabtechtv.com/posts/yotld-part-3-flatpak-trusted-certs/> ·
  <https://p11-glue.github.io/p11-glue/p11-kit/manual/trust.html>

### Rust TLS ecosystem
- `rustls-native-certs` 0.8.x checks **`SSL_CERT_FILE` first** on all platforms,
  then falls back to the platform store (schannel / Keychain / openssl-probe).
  <https://github.com/rustls/rustls-native-certs>
- `rustls-platform-verifier` uses OS verifiers; on Linux it uses webpki +
  `rustls-native-certs` + `openssl-probe`.
  <https://github.com/rustls/rustls-platform-verifier> ·
  <https://docs.rs/rustls-platform-verifier/latest/rustls_platform_verifier/>
- **`reqwest` v0.13.0 (released 2025-12-30) defaults to rustls**; `native-tls`
  is now opt-in. `add_root_certificate()` is **deprecated** in favor of
  `tls_certs_merge()` / `tls_certs_only()`.
  <https://seanmonstar.com/blog/reqwest-v013-rustls-default/> ·
  <https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html> ·
  <https://crates.io/crates/reqwest>

---

## Current versions / APIs (as of Jun 2026)

| Component | Version / state | Trust knob | Notes |
|---|---|---|---|
| Node.js | `NODE_USE_SYSTEM_CA` present (⚠ floor unverified) | `NODE_EXTRA_CA_CERTS`, `NODE_USE_SYSTEM_CA` | both additive; ignored if setuid/caps |
| Python `requests` | current | `REQUESTS_CA_BUNDLE` → `CURL_CA_BUNDLE` → certifi | prepared reqs need `merge_environment_settings()` |
| curl | current | `CURL_CA_BUNDLE` (+ `SSL_CERT_FILE`/`SSL_CERT_DIR`) | precedence ⚠ undocumented |
| OpenSSL | master | `SSL_CERT_FILE`, `SSL_CERT_DIR` | dir needs `c_rehash` |
| git | current | `GIT_SSL_CAINFO` | env wins over `http.sslCAInfo` |
| Ubuntu ca-certificates | **2.82 (Feb 2026)** | `/usr/local/share/ca-certificates/` + `update-ca-certificates` | MS CA 2011 expires Jun 2026 |
| `reqwest` | **0.13.0 (2025-12-30)** | `tls_certs_merge()` / `tls_certs_only()` | rustls default; `add_root_certificate()` deprecated |
| `rustls-native-certs` | 0.8.x | `SSL_CERT_FILE` (first) | platform store fallback |
| `rustls-platform-verifier` | current | OS verifier | Linux = webpki + native-certs |

---

## Security tradeoffs

- **Env-var, child-only injection (recommended):** smallest blast radius, dies
  with the process, fully auditable. Cost: must be wired per tool, and each tool
  has its own env var / precedence quirks.
- **System bundle write:** one place, broad reach — but it widens trust for the
  **whole machine and all users**, not just env-ctl's relay. Reversible via
  owned-file deletion, but a live window of machine-wide MITM trust exists while
  installed. Snaps still won't honor it.
- **`/etc/ca-certificates.conf` `!`-prefix deselection:** valid but requires
  hand-editing a shared file → harder to audit and to cleanly revert than an
  owned discrete file. env-ctl should **avoid** it.
- **OS-store discovery in env-ctl's own Rust code:** convenient but means a
  poisoned system store silently widens what the relay/clients trust. Prefer
  **explicit pinned roots** (`webpki-roots` frozen set, or an explicit PEM for the
  local CA only) so trust is exactly what env-ctl intends.
- **Node setuid/caps caveat:** if env-ctl ever relays a privileged Node binary,
  `NODE_EXTRA_CA_CERTS` is silently dropped — don't rely on it there.
- **Directory-form trust (`SSL_CERT_DIR`):** requires `c_rehash`; a malformed dir
  fails open/closed unpredictably. Prefer single-file `CURL_CA_BUNDLE` /
  `REQUESTS_CA_BUNDLE` / `GIT_SSL_CAINFO`.

---

## Concrete guidance for the env-ctl implementation

1. **Per-tool env injection (default path).** When relaying a tool, set in the
   child env only:
   - Node: `NODE_EXTRA_CA_CERTS=<ca.pem>`
   - Python `requests`: `REQUESTS_CA_BUNDLE=<ca.pem>`
   - git: `GIT_SSL_CAINFO=<ca.pem>`
   - curl: `CURL_CA_BUNDLE=<ca.pem>` (only this one; skip `SSL_CERT_FILE`/`_DIR`)
   - Generic OpenSSL consumers: `SSL_CERT_FILE=<ca.pem>` (single file, no rehash).
   Keep `<ca.pem>` as a **single PEM** containing the local CA (leaf →
   intermediate → root order if it is a chain; ordering is backend-dependent ⚠,
   see <https://support.globalsign.com/ca-certificates/root-certificates/root-intermediate-certificate-bundles>).

2. **env-ctl's own Rust relay/clients.** Build on `reqwest` 0.13 (rustls default).
   Load the local CA explicitly via `ClientBuilder::tls_certs_merge()` (or
   `tls_certs_only()` to trust *only* the local CA on the relay leg). Do **not**
   depend on `rustls-native-certs` OS discovery for the relay; if a dependency
   pulls it in, remember it reads `SSL_CERT_FILE` first.

3. **System bundle fallback (opt-in, default OFF).**
   - Write `/usr/local/share/ca-certificates/env-ctl-local-mitm-ca.crt` (owned).
   - Timestamp-backup `/etc/ssl/certs/ca-certificates.crt` first.
   - `update-ca-certificates`.
   - **Revert:** delete owned file + `update-ca-certificates --fresh`.
   - Never touch `/etc/ca-certificates.conf`. (Matches DESIGN-NOTES CF-7;
     more auditable than `!`-deselection.)

4. **Confinement handling.** For strict **Snap** targets, rely on env vars or use
   non-snapped equivalents; document that the system bundle is invisible to them.
   For **Flatpak** (not the primary Ubuntu target, but for completeness): place
   anchors in the host p11-kit trust store + `update-ca-trust`.

5. **Version/maintenance watch.** Keep ca-certificates ≥ 2.82 on the box (MS CA
   2011 expiry, Jun 2026) so package management and the system store stay valid;
   env-ctl's CA file is independent of this but shares the same `update-ca-certificates`
   pipeline.

---

## Open questions (flagged, need verification)

1. **curl precedence** when `CURL_CA_BUNDLE` + `SSL_CERT_FILE` + `SSL_CERT_DIR`
   are all set — undocumented; needs empirical test or `lib/sslcerts.c` audit.
2. **Node `NODE_USE_SYSTEM_CA` version floor** — confirm exact LTS versions on
   nodejs.org changelog.
3. **`requests` dual-var behavior** (`REQUESTS_CA_BUNDLE` + `CURL_CA_BUNDLE`) —
   confirm no hidden interaction via source.
4. **`merge_environment_settings()` field coverage** — does it merge only the CA
   bundle or also `verify` and proxies? Source review needed.
5. **Snap host-`/etc/ssl` mount** — which snapd versions / cert paths actually
   become visible on Ubuntu 26.04; test locally.
6. **`c_rehash` exact hash algorithm + dir layout** for `SSL_CERT_DIR` — believed
   SHA1 subject-hash symlinks; confirm before using directory-form trust.
7. **Cert chain ordering** per backend (nginx vs raw OpenSSL vs reqwest/rustls) —
   partially verified; confirm for the single-PEM env-ctl ships.
