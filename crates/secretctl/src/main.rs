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
        Cmd::Run(_) => {
            anyhow::bail!("`env-ctl run` is not wired in Phase 6 (data-plane is Phase 8)");
        }
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
                    println!("{} v{} broker_only={}", item.name, item.version, item.broker_only);
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
                    println!("{}", serde_json::json!({ "name": item.name, "enabled": item.enabled }));
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
