//! secretctl — the `env-ctl` CLI. A thin gRPC client over the daemon's Unix socket; it drains the
//! `Event` stream and pretty-prints (or `--json`). Destructive verbs default to dry-run (`--apply`
//! to act, `--confirm` for root-of-trust). The bearer/value printing is owner-only and only on the
//! peercred-gated channel; the real key is never printed (the daemon never sends it).
mod cli;
mod render;

use std::io::Read;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use cli::{CaCmd, Cli, Cmd, RelayCmd, SecretCmd};
use envctl_secrets_proto::v1;
use hyper_util::rt::TokioIo;
use tonic::transport::{Endpoint, Uri};
use tonic::Streaming;

type VaultClient = v1::vault_client::VaultClient<tonic::transport::Channel>;
type RelayClient = v1::relay_client::RelayClient<tonic::transport::Channel>;
type LockClient = v1::lock_client::LockClient<tonic::transport::Channel>;
type AuditClient = v1::audit_client::AuditClient<tonic::transport::Channel>;

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    rt.block_on(run(args))
}

/// Resolve the control socket: `--socket` override, else `$XDG_RUNTIME_DIR/env-ctl/secretd.sock`.
/// secretctl does NOT depend on the engine, so this path is recomputed inline (mirrors
/// `Paths::resolve().control_socket()`).
fn socket_path(override_path: &Option<String>) -> anyhow::Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(PathBuf::from(p));
    }
    let runtime = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(r) => PathBuf::from(r).join("env-ctl"),
        None => {
            // Fall back to the XDG state dir, matching the engine's Paths::resolve fallback.
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("neither XDG_RUNTIME_DIR nor HOME is set"))?;
            std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/state"))
                .join("env-ctl")
        }
    };
    Ok(runtime.join("secretd.sock"))
}

/// Connect a tonic `Channel` to the daemon over the UDS. tonic's `Channel` cannot bind a UDS
/// directly, so we use the classic `service_fn` connector: the URI is ignored; every connection is
/// a fresh `UnixStream` to `sock`, wrapped in `TokioIo` to satisfy hyper's IO traits.
async fn connect(sock: PathBuf) -> anyhow::Result<tonic::transport::Channel> {
    // The scheme/authority are placeholders; the connector ignores them and dials the UDS.
    let endpoint = Endpoint::try_from("http://[::]:0").context("building the endpoint")?;
    let channel = endpoint
        .connect_with_connector(tower::service_fn(move |_: Uri| {
            let sock = sock.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(sock).await?;
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await
        .context("connecting to the daemon socket (is secretd running?)")?;
    Ok(channel)
}

fn provider_to_proto(s: &str) -> i32 {
    let k = match s.to_ascii_lowercase().as_str() {
        "anthropic" => v1::ProviderKind::Anthropic,
        "openai" => v1::ProviderKind::Openai,
        "github" => v1::ProviderKind::Github,
        "generic" => v1::ProviderKind::Generic,
        _ => v1::ProviderKind::Generic,
    };
    k as i32
}

fn mode_to_proto(s: &str) -> i32 {
    let m = match s.to_ascii_lowercase().as_str() {
        "base-url" | "base_url" | "baseurl" => v1::DataPlaneMode::BaseUrlRepoint,
        "proxy" | "mitm" => v1::DataPlaneMode::HttpsProxyMitm,
        "native" | "subtoken" => v1::DataPlaneMode::NativeSubtoken,
        _ => v1::DataPlaneMode::BaseUrlRepoint,
    };
    m as i32
}

fn read_stdin_string() -> anyhow::Result<String> {
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s)?;
    Ok(s.trim_end_matches(['\n', '\r']).to_string())
}

fn read_stdin_bytes() -> anyhow::Result<Vec<u8>> {
    let mut v = Vec::new();
    std::io::stdin().read_to_end(&mut v)?;
    Ok(v)
}

/// Drain a server-streamed `Event` response, rendering each event.
async fn drain(mut stream: Streaming<v1::Event>, json: bool) -> anyhow::Result<()> {
    while let Some(ev) = stream.message().await? {
        render::render_event(&ev, json);
    }
    Ok(())
}

async fn run(args: Cli) -> anyhow::Result<()> {
    let sock = socket_path(&args.socket)?;
    let json = args.json;

    match args.cmd {
        Cmd::Status => {
            let mut c = LockClient::new(connect(sock).await?);
            let r = c.status(v1::StatusReq {}).await?.into_inner();
            render::render_status(&r, json);
        }
        Cmd::Init {
            passphrase_stdin,
            enroll_usb,
            usb_partuuid,
            apply,
        } => {
            // `--usb-partuuid` is required when enrolling a USB keyslot (the daemon also re-checks,
            // fail-closed). Catch it client-side for a friendlier error.
            if enroll_usb
                && usb_partuuid
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
            {
                anyhow::bail!("--enroll-usb requires --usb-partuuid <UUID>");
            }
            let passphrase = if passphrase_stdin {
                Some(read_stdin_string()?)
            } else {
                None
            };
            let mut c = VaultClient::new(connect(sock).await?);
            let stream = c
                .init(v1::InitReq {
                    passphrase,
                    enroll_usb,
                    usb_partition_uuid: usb_partuuid.unwrap_or_default(),
                    apply,
                })
                .await?
                .into_inner();
            drain(stream, json).await?;
        }
        Cmd::Unlock { passphrase_stdin } => {
            let passphrase = if passphrase_stdin {
                Some(read_stdin_string()?)
            } else {
                None
            };
            let mut c = LockClient::new(connect(sock).await?);
            let stream = c.unlock(v1::UnlockReq { passphrase }).await?.into_inner();
            drain(stream, json).await?;
        }
        Cmd::Lock => {
            let mut c = LockClient::new(connect(sock).await?);
            let stream = c.lock_now(v1::LockReq {}).await?.into_inner();
            drain(stream, json).await?;
        }
        Cmd::Secret { cmd } => secret(cmd, sock, json).await?,
        Cmd::Relay { cmd } => relay(cmd, sock, json).await?,
        Cmd::Ca { cmd } => ca(cmd, sock, json).await?,
        Cmd::Audit(a) => {
            let mut c = AuditClient::new(connect(sock).await?);
            let req = v1::AuditQueryReq {
                actor: a.actor,
                relay: a.relay,
                since: a.since,
                until: a.until,
                limit: a.limit.unwrap_or(0),
            };
            let r = c.query(req).await?.into_inner();
            render::render_audit(&r, json);
        }
        Cmd::Run(a) => run_child_cmd(a, sock, json).await?,
    }
    Ok(())
}

async fn secret(cmd: SecretCmd, sock: PathBuf, json: bool) -> anyhow::Result<()> {
    let mut c = VaultClient::new(connect(sock).await?);
    match cmd {
        SecretCmd::Add {
            name,
            provider,
            value_stdin,
            note,
            overwrite,
            broker_only,
        } => {
            let value = if value_stdin {
                read_stdin_bytes()?
            } else {
                anyhow::bail!("secret add requires --value-stdin");
            };
            let req = v1::AddSecretReq {
                name,
                provider: provider_to_proto(&provider),
                value,
                note: note.unwrap_or_default(),
                overwrite,
                broker_only,
            };
            let stream = c.add(req).await?.into_inner();
            drain(stream, json).await?;
        }
        SecretCmd::Get {
            name,
            reveal,
            apply,
            confirm,
        } => {
            let req = v1::GetSecretReq {
                name,
                reveal,
                apply,
                confirm,
            };
            let r = c.get(req).await?.into_inner();
            render::render_get(&r, json);
        }
        SecretCmd::List { provider } => {
            let req = v1::ListSecretReq {
                provider: provider.as_deref().map(provider_to_proto),
            };
            let r = c.list(req).await?.into_inner();
            for item in &r.items {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({ "name": item.name, "version": item.version,
                            "broker_only": item.broker_only })
                    );
                } else {
                    println!(
                        "{} v{} broker_only={}",
                        item.name, item.version, item.broker_only
                    );
                }
            }
        }
        SecretCmd::Rm {
            name,
            apply,
            confirm,
        } => {
            let req = v1::RmSecretReq {
                name,
                apply,
                confirm,
            };
            let stream = c.rm(req).await?.into_inner();
            drain(stream, json).await?;
        }
        SecretCmd::Rotate {
            name,
            value_stdin,
            apply,
        } => {
            let new_value = if value_stdin {
                read_stdin_bytes()?
            } else {
                Vec::new()
            };
            let req = v1::RotateReq {
                name,
                new_value,
                apply,
            };
            let stream = c.rotate(req).await?.into_inner();
            drain(stream, json).await?;
        }
    }
    Ok(())
}

async fn relay(cmd: RelayCmd, sock: PathBuf, json: bool) -> anyhow::Result<()> {
    let mut c = RelayClient::new(connect(sock).await?);
    match cmd {
        RelayCmd::Create {
            name,
            secret,
            provider,
            mode,
            upstream_base,
            hosts,
            paths,
            methods,
            expires,
            rate,
            quota,
            disabled,
        } => {
            let policy = v1::RelayPolicy {
                name,
                secret_name: secret,
                provider: provider_to_proto(&provider),
                mode: mode_to_proto(&mode),
                host_allow: hosts,
                path_allow: paths,
                method_allow: methods,
                expires_at: expires.unwrap_or_default(),
                rate_per_min: rate.unwrap_or(0),
                quota_total: quota.unwrap_or(0),
                enabled: !disabled,
                ephemeral: false,
                upstream_base: upstream_base.unwrap_or_default(),
            };
            let stream = c
                .create(v1::CreateRelayReq {
                    policy: Some(policy),
                })
                .await?
                .into_inner();
            drain(stream, json).await?;
        }
        RelayCmd::Revoke {
            name,
            apply,
            confirm,
        } => {
            let r = c
                .revoke(v1::RevokeRelayReq {
                    name,
                    apply,
                    confirm,
                })
                .await?
                .into_inner();
            render::render_revoke(&r, json);
        }
        RelayCmd::RevokeToken { token_id, apply } => {
            let r = c
                .revoke_bearer(v1::RevokeBearerReq { token_id, apply })
                .await?
                .into_inner();
            render::render_revoke(&r, json);
        }
        RelayCmd::List { all } => {
            let r = c
                .list(v1::ListRelayReq {
                    include_revoked: all,
                })
                .await?
                .into_inner();
            for item in &r.items {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({ "name": item.name, "enabled": item.enabled })
                    );
                } else {
                    println!("{} enabled={}", item.name, item.enabled);
                }
            }
        }
        RelayCmd::Mint { name, ttl } => {
            // TTL string -> seconds (default 0 => engine clamps against policy + the 24h ceiling).
            let ttl_secs = match ttl {
                Some(s) => s
                    .parse::<u64>()
                    .with_context(|| format!("invalid --ttl '{s}' (expected seconds)"))?,
                None => 0,
            };
            let r = c
                .mint(v1::MintReq {
                    relay: name,
                    ephemeral: false,
                    provider: v1::ProviderKind::Generic as i32,
                    ttl_secs,
                    client_pid: 0,
                })
                .await?
                .into_inner();
            render::render_mint(&r, json);
        }
    }
    Ok(())
}

async fn ca(_cmd: CaCmd, _sock: PathBuf, _json: bool) -> anyhow::Result<()> {
    // Certs.* is Phase 4+; the daemon returns Unimplemented for every Ca verb.
    anyhow::bail!("`env-ctl ca` is not available in Phase 6 (Certs are Phase 4+)")
}

// ---- `env-ctl run` (PR-2b): mint a bearer + run the child with the daemon-built injection --------

/// Map the daemon's proto `ResolvedInjection` back into the engine's `inject::ResolvedInjection`. The
/// DAEMON is authoritative: it built the env delta (the bearer-only child env) via the engine's
/// `injection_template` and shipped the resolved shape over the peercred-gated UDS. This is a pure
/// field-for-field transcription — secretctl re-derives NO env/key logic; all of that stays in the
/// engine. The empty-string `proxy_url`/`base_url` proto sentinel maps back to `None`.
fn injection_from_proto(p: &v1::ResolvedInjection) -> envctl_secrets::inject::ResolvedInjection {
    use envctl_secrets::broker::Provider;
    use envctl_secrets::inject::DataPlaneMode;
    let provider = match v1::ProviderKind::try_from(p.provider).unwrap_or(v1::ProviderKind::Generic)
    {
        v1::ProviderKind::Anthropic => Provider::Anthropic,
        v1::ProviderKind::Openai => Provider::Openai,
        v1::ProviderKind::Github => Provider::Github,
        v1::ProviderKind::Generic | v1::ProviderKind::ProviderUnspecified => Provider::Generic,
    };
    let mode =
        match v1::DataPlaneMode::try_from(p.mode).unwrap_or(v1::DataPlaneMode::BaseUrlRepoint) {
            v1::DataPlaneMode::HttpsProxyMitm => DataPlaneMode::HttpsProxyMitm,
            v1::DataPlaneMode::NativeSubtoken => DataPlaneMode::NativeSubtoken,
            v1::DataPlaneMode::BaseUrlRepoint | v1::DataPlaneMode::ModeUnspecified => {
                DataPlaneMode::BaseUrlRepoint
            }
        };
    let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
    envctl_secrets::inject::ResolvedInjection {
        provider,
        mode,
        env: p.env.clone().into_iter().collect(),
        ca_env_keys: p.ca_env_keys.clone(),
        proxy_url: opt(&p.proxy_url),
        base_url: opt(&p.base_url),
    }
}

/// Build the `MintReq` for an `env-ctl run` from its args (pure; unit-tested). The relay name is the
/// first `--relay` (else the provider name, else "default"); the provider defaults to generic
/// (default-deny in the engine). `client_pid = 0` selects uid-primary binding (OQ1); `ttl_secs = 0`
/// lets the engine clamp against the policy + the 24h ceiling.
fn mint_req_for_run(a: &cli::RunArgs) -> v1::MintReq {
    let provider = a.provider.as_deref().unwrap_or("generic");
    let relay = a
        .relays
        .first()
        .cloned()
        .or_else(|| a.provider.clone())
        .unwrap_or_else(|| "default".to_string());
    v1::MintReq {
        relay,
        ephemeral: a.ephemeral,
        provider: provider_to_proto(provider),
        ttl_secs: 0,
        client_pid: 0,
    }
}

/// `env-ctl run -- <cmd> [args...]`: mint a peer-bound ephemeral bearer, then spawn the child with the
/// daemon-built env injection overlaid (the bearer + base-url/proxy repoint) — the real key NEVER
/// enters the child. Engine-driven: secretctl is a thin driver that mints over gRPC, then calls
/// `Engine::run_child` in-process, draining its `Event`s to the existing renderer. The process exits
/// with the child's true exit code.
///
/// Peer-binding (OQ1): we mint with `client_pid = 0` (uid-primary binding). The relay decision
/// (`broker::decide`) enforces the bound uid at swap time, and the PR-2a proxy resolves the request's
/// peer uid (not pid) from the loopback connection; the child runs as the same uid as secretctl, so
/// the uid binding holds with no exec gymnastics. (decide's pid check only fires for a non-None bound
/// pid, and the PR-2a proxy sends `peer_pid: None`, so binding a pid would deny the swap.)
async fn run_child_cmd(a: cli::RunArgs, sock: PathBuf, json: bool) -> anyhow::Result<()> {
    // Fail-closed: an empty argv has no program to run (the engine also refuses, but catch it early
    // for a friendlier error and to avoid an unnecessary mint).
    if a.argv.is_empty() {
        anyhow::bail!(
            "`env-ctl run` requires a command: env-ctl run [--relay R] -- <cmd> [args...]"
        );
    }

    // Mint a peer-bound ephemeral bearer + receive the daemon-built injection (PR-2b populates it).
    let mut c = RelayClient::new(connect(sock).await?);
    let resp = c.mint(mint_req_for_run(&a)).await?.into_inner();

    // FAIL-CLOSED: without a populated injection (e.g. the daemon's relay proxy never bound) we have
    // no proxy to repoint the child at — refuse rather than spawn with a half-built env.
    let proto_injection = resp.injection.ok_or_else(|| {
        anyhow::anyhow!(
            "the daemon returned no child-env injection (is the relay proxy listening?); refusing to \
             run the child without a repointed, bearer-only env"
        )
    })?;
    let injection = injection_from_proto(&proto_injection);

    // Build the engine plan. The bearer is uid-bound, so no pid hint is needed (OQ1).
    let plan = envctl_secrets::inject::ChildEnvPlan {
        injection,
        child_pid_hint: None,
    };

    // Drive the engine in-process. `run_child` overlays ONLY the injection env (bearer, never the real
    // key) onto the inherited parent env, streams the child's stdout/stderr as `Event`s, and returns
    // the child's true exit code. We render those events through the same renderer the rest of the CLI
    // uses, then exit with the child's code.
    let (sink, rx) = envctl_secrets::EventSink::channel();

    // Open an in-process engine over the real seams. The engine is non-printing — it emits Events that
    // we render below. (run_child needs no vault/USB; it only spawns the child with the overlay env.)
    let paths = envctl_secrets::paths::Paths::resolve().context("resolving engine paths")?;
    let engine = envctl_secrets::Engine::open(paths).context("opening the in-process engine")?;

    let argv = a.argv.clone();
    // run_child is a SYNC, blocking call (it waits on the child); run it off the async reactor.
    let render_handle = std::thread::spawn(move || {
        for ev in rx {
            render_secret_event(&ev, json);
        }
    });
    let code = tokio::task::spawn_blocking(move || engine.run_child(plan, argv, &sink))
        .await
        .context("joining the child-run task")?
        .context("running the child")?;
    // The sink dropped when `run_child` returned; the render thread drains the rest and exits.
    let _ = render_handle.join();

    // Exit with the child's true exit code (POSIX 128+signal already folded by the engine).
    std::process::exit(code);
}

/// Render an engine `SecretEvent` (the in-process variant from `run_child`) to the TTY or as NDJSON.
/// Mirrors `render::render_event`, which renders the PROTO twin; here the events come straight from
/// the engine, so we map the variants we expect from a child run (`Log`, `ChildExited`,
/// `RunFinished`, `GuardRefused`). NEVER prints a secret (the engine's events carry none).
fn render_secret_event(ev: &envctl_secrets::SecretEvent, json: bool) {
    // Reuse the proto renderer by converting the engine event to its proto twin (the single mapping
    // already under test in secretd's `conv::event_to_proto`). We inline the minimal subset here so
    // secretctl does not depend on secretd. Variants without a proto twin are dropped.
    use envctl_secrets::event::{SecretEvent, Stream};
    let line_json = |v: serde_json::Value| println!("{v}");
    match ev {
        SecretEvent::Log {
            source,
            stream,
            line,
        } => {
            let s = matches!(stream, Stream::Stderr);
            if json {
                line_json(serde_json::json!({
                    "type": "log", "source": source, "stream": if s {1} else {0}, "line": line
                }));
            } else {
                let label = if s { "stderr" } else { "stdout" };
                println!("\x1b[36m[{source}:{label}] {line}\x1b[0m");
            }
        }
        SecretEvent::ChildExited { code } => {
            if json {
                line_json(serde_json::json!({ "type": "child_exited", "code": code }));
            } else {
                println!("\x1b[36mchild exited: {code}\x1b[0m");
            }
        }
        SecretEvent::RunFinished { summary } => {
            if json {
                line_json(serde_json::json!({
                    "type": "run_finished", "failed": summary.failed, "refused": summary.refused
                }));
            } else {
                println!(
                    "\x1b[32mrun finished (failed: {}, refused: {})\x1b[0m",
                    summary.failed.len(),
                    summary.refused.len()
                );
            }
        }
        SecretEvent::GuardRefused { subject, reason } => {
            if json {
                line_json(serde_json::json!({
                    "type": "guard_refused", "subject": subject, "reason": reason
                }));
            } else {
                println!("\x1b[33mrefused: {subject} ({reason})\x1b[0m");
            }
        }
        // Other variants are not produced by `run_child`; ignore them.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(relays: Vec<&str>, provider: Option<&str>, ephemeral: bool) -> cli::RunArgs {
        cli::RunArgs {
            relays: relays.into_iter().map(str::to_string).collect(),
            provider: provider.map(str::to_string),
            ephemeral,
            no_profile: false,
            profile: None,
            argv: vec!["printenv".to_string(), "ANTHROPIC_API_KEY".to_string()],
        }
    }

    #[test]
    fn mint_req_uses_uid_primary_binding_and_relay_from_flag() {
        // OQ1: client_pid is always 0 (uid-primary binding); the relay name comes from --relay.
        let req = mint_req_for_run(&run_args(vec!["claude-main"], Some("anthropic"), true));
        assert_eq!(req.client_pid, 0, "must mint with client_pid=0 (uid-bound)");
        assert_eq!(req.relay, "claude-main");
        assert_eq!(req.provider, v1::ProviderKind::Anthropic as i32);
        assert!(req.ephemeral);
        assert_eq!(req.ttl_secs, 0, "ttl=0 lets the engine clamp");
    }

    #[test]
    fn mint_req_falls_back_to_provider_then_default_relay() {
        // No --relay, but --provider given => relay = provider name.
        let req = mint_req_for_run(&run_args(vec![], Some("openai"), false));
        assert_eq!(req.relay, "openai");
        assert_eq!(req.provider, v1::ProviderKind::Openai as i32);
        // Neither --relay nor --provider => relay "default", provider generic.
        let req = mint_req_for_run(&run_args(vec![], None, false));
        assert_eq!(req.relay, "default");
        assert_eq!(req.provider, v1::ProviderKind::Generic as i32);
        assert_eq!(req.client_pid, 0);
    }

    #[test]
    fn injection_from_proto_reconstructs_engine_plan() {
        // The daemon ships the resolved injection; secretctl transcribes it into the engine type that
        // `ChildEnvPlan` carries. The bearer-only env, mode, provider, and base_url survive intact; the
        // empty-string proxy_url sentinel maps back to None.
        const BEARER: &str = "bearer-abc";
        const BASE: &str = "http://127.0.0.1:9000";
        let mut env = std::collections::HashMap::new();
        env.insert("ANTHROPIC_BASE_URL".to_string(), BASE.to_string());
        env.insert("ANTHROPIC_API_KEY".to_string(), BEARER.to_string());
        let proto = v1::ResolvedInjection {
            provider: v1::ProviderKind::Anthropic as i32,
            mode: v1::DataPlaneMode::BaseUrlRepoint as i32,
            env,
            ca_env_keys: vec![],
            proxy_url: String::new(), // sentinel -> None
            base_url: BASE.to_string(),
        };
        let eng = injection_from_proto(&proto);
        use envctl_secrets::broker::Provider;
        use envctl_secrets::inject::DataPlaneMode;
        assert_eq!(eng.provider, Provider::Anthropic);
        assert_eq!(eng.mode, DataPlaneMode::BaseUrlRepoint);
        assert_eq!(
            eng.env.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some(BEARER)
        );
        assert_eq!(
            eng.env.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some(BASE)
        );
        assert_eq!(eng.base_url.as_deref(), Some(BASE));
        assert!(eng.proxy_url.is_none(), "empty proxy_url must map to None");
        assert!(eng.ca_env_keys.is_empty());

        // Build the plan the same way run_child_cmd does; the bearer is uid-bound, so no pid hint.
        let plan = envctl_secrets::inject::ChildEnvPlan {
            injection: eng,
            child_pid_hint: None,
        };
        assert!(plan.child_pid_hint.is_none());
        assert_eq!(
            plan.injection
                .env
                .get("ANTHROPIC_API_KEY")
                .map(String::as_str),
            Some(BEARER)
        );
    }
}
