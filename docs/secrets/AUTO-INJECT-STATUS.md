# Auto-Injection Seam — Status & Handoff (2026-06-13)

Cold-start checkpoint for the **secrets auto-injection** feature session. This is a *feature*
checkpoint (not the agenticOS `forge-loop` — that loop has its own `.handoff/loop/HANDOFF.md`,
carried by a separate session). State precedence: **Git > this doc**.

## What this delivers (owner intent)
envctl HOLDS the secrets and AUTO-INJECTS API keys into a tool's child process when it needs them —
the real vault key NEVER enters the child env, shell history, logs, errors, or git. The child only
ever holds a short-lived, peer-bound, USB-gated relay **bearer**; the **real key is swapped in only
at the local proxy's egress** (`relay_swap` → `DaemonUpstream::send`, `Zeroizing`).

## DONE — the full Phase-8 data-plane (5 PRs, all merged to develop `21a51fb`)
| PR | Slice | Crate(s) |
|----|-------|----------|
| #51 | Engine seam: `injection_template` (provider→env table) + `Engine::run_child` + `discover_profile` | secrets-engine |
| #58 | Relay **proxy listener** (loopback `127.0.0.1:0`, bearer→real-key swap, webpki-roots upstream) | secretd |
| #60 | `secretctl run` + grpc Mint injection (BaseUrlRepoint end-to-end) | secretd, secretctl |
| #63 | Vault-backed **local CA stack** (`mitm-ca`): `LocalCa`, `ca_init`, `issue_leaf_for_covered_host`, CF-5 | secrets-engine |
| #69 | **HTTPS_PROXY/CONNECT MITM proxy** (`mod mitm`: CONNECT→leaf→TLS-terminate→relay_swap) | secretd (+ engine `observed_sni`) |

**Both client classes now work** via `env-ctl run -- <tool>`:
- **Base-URL-repoint** (Claude/OpenAI SDKs): child gets `*_BASE_URL`=loopback proxy + `*_API_KEY`=bearer (+`LLM_API_KEY`).
- **HTTPS_PROXY tools that can't repoint** (`git`/`curl`): the proxy MITM-terminates their TLS with a
  vault-backed per-host leaf (trusted via the injected CA bundle), reads plaintext, swaps at egress.

Invariants held throughout (guardian-verified each PR): **no C in the trust boundary**, exactly one
rustls (ring-only), engine is the single non-printing library, fail-closed, **CF-5** (MITM leaves
only for relay-covered hosts, never operator-minted), key-never-in-child (structural + e2e-proven).
Reference: adopted the CONNECT/leaf/terminate *pattern* from `johnsonlee/rustyman` + re-authored
stalwart's `ResolvesServerCert` idiom **ring-only** — vendored nothing (soth-mitm/vproxy/pingora/
mitmproxy_rs disqualified by no-C / license). Dep added: `tokio-rustls 0.26 default-features=false
features=["ring","tls12"]` (tracks pinned rustls 0.23 — no second rustls, no aws-lc).

## NEXT / remaining (future work — none blocking; the seam is usable now)
1. **Live smoke test** (HUMAN, highest value): run `env-ctl run -- claude -p "hi"` against a real
   secretd + an unlocked vault holding a real provider key + the USB possession factor. The e2e
   tests prove the mechanism in-process; a real-daemon run validates the full path (daemon proxy
   bind, USB gate, `ca_init` for MITM). Requires interactive vault unlock + USB.
2. **NativeSubtoken mode** — `injection_template` emits the NativeSubtoken *shell*, but real minting
   is the `ProviderMint` seam (`mint_github.rs`; `NoMint` default → GitHub `ProviderMint` is the
   greenfield, HFTASK-0013). Wire it so GitHub gets a native scoped sub-token instead of a proxied bearer.
3. **Peer-binding upgrade (advisory → strict):** PR-2b chose **uid-only** binding (`client_pid=0`)
   because `decide.rs` checks pid only when bound and the proxy sends `peer_pid:None`. A future
   exec-replace path (secretctl `execvp`s the child so it inherits the minted pid) would enable
   strict pid binding — only worth it if same-uid trust is deemed insufficient.
4. **Operator `ca_issue`** — PR-3a implemented the CF-5 refusal of `mitm_leaf`; full operator
   NON-MITM leaf minting is a thin follow-up if an operator-CA surface is wanted.
5. **MITM polish:** per-SNI leaf caching across connections; non-443 CONNECT edge cases; broader
   provider auth-header coverage in `DaemonUpstream`/`auth_header_for`.

## Verify-on-resume
```
cd <fresh worktree off develop>
cargo build -p envctl-secrets-engine -p envctl-secretd -p envctl-secretctl
cargo test  -p envctl-secrets-engine -p envctl-secretd     # 138 engine + 39 secretd green
cargo build -p envctl-secretd --no-default-features        # MITM-less build (501 fallback) compiles
bash ci/gates/no-c.sh && bash ci/gates/shape.sh && bash ci/gates/enable.sh
```

## Pointers
- Design corpus: `docs/secrets/` (THREAT-MODEL, SERVER-MODE, DESIGN-NOTES, ROADMAP).
- Engine seam: `crates/secrets-engine/src/{inject.rs,ca.rs,lib.rs}` (`relay_swap`, `injection_template`,
  `run_child`, `LocalCa`, `issue_leaf_for_covered_host`).
- Proxy: `crates/secretd/src/proxy.rs` (`DaemonUpstream`, `mod mitm::{MitmCertResolver, handle_connect,
  handle_decrypted}`, `serve_proxy`). CLI: `crates/secretctl/src/main.rs` (`Cmd::Run`).
- ICM: topic `context-envctl` (5 entries, PR-by-PR) + `decisions-envctl` (OQ1 uid-binding, FINDING-0002).
