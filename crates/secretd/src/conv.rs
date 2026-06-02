//! Pure proto <-> engine conversions. No engine changes; no I/O. These functions are the single
//! boundary where the wire types (prost-generated `env_ctl.v1`) meet the engine's domain types.
//!
//! The two load-bearing invariants live here only as *shape*: the engine guarantees the raw bearer
//! and any real key never enter a `SecretEvent`, so `event_to_proto` has no field to leak them
//! through — the `RelayMinted` proto twin carries no bearer, by construction.
//!
//! Some conversions (the policy/method/audit-entry maps) are the TOTAL conversion surface the design
//! specifies, but are not all reached in Phase 6 because their RPCs (`Relay.Create`, `Vault.List`,
//! `Audit.Query`) return `Unimplemented` here — they are consumed when those paths land. We allow
//! dead_code at module scope (mirroring the engine's own scaffold discipline) rather than delete a
//! spec-mandated, tested-by-construction mapping.
#![allow(dead_code)]
use envctl_secrets::broker::{Method, Provider, RelayKind, RelayPolicy, SwapMode};
use envctl_secrets::keyslot::Factor;
use envctl_secrets::{AuditRecord, SecretEvent, SecretMeta};
use envctl_secrets_proto::v1;
use tonic::Status;

// ---- small enum string maps (engine -> stable wire strings) --------------------------------

/// `keyslot::Factor` -> the `"usb"|"passphrase"` strings the proto `VaultUnlocked.factor` carries.
pub fn factor_str(f: Factor) -> String {
    match f {
        Factor::Usb => "usb",
        Factor::Passphrase => "passphrase",
    }
    .to_string()
}

/// `broker::RelayKind` -> `"named"|"ephemeral"`.
pub fn relaykind_str(k: RelayKind) -> String {
    match k {
        RelayKind::Named => "named",
        RelayKind::Ephemeral => "ephemeral",
    }
    .to_string()
}

// ---- provider <-> ProviderKind --------------------------------------------------------------

/// Inbound: proto `ProviderKind` (an `i32` on the wire) -> engine `Provider`. `PROVIDER_UNSPECIFIED`
/// (and any unknown discriminant) maps to `Generic` — default-safe (Generic's canonical upstream set
/// is empty, so a Generic relay is default-deny in the engine's swap fence).
pub fn provider_from_proto(p: i32) -> Provider {
    match v1::ProviderKind::try_from(p).unwrap_or(v1::ProviderKind::Generic) {
        v1::ProviderKind::Anthropic => Provider::Anthropic,
        v1::ProviderKind::Openai => Provider::Openai,
        v1::ProviderKind::Github => Provider::Github,
        v1::ProviderKind::Generic | v1::ProviderKind::ProviderUnspecified => Provider::Generic,
    }
}

/// Outbound: engine `Provider` -> proto `ProviderKind` discriminant (`i32`).
pub fn provider_to_proto(p: Provider) -> i32 {
    let k = match p {
        Provider::Anthropic => v1::ProviderKind::Anthropic,
        Provider::Openai => v1::ProviderKind::Openai,
        Provider::Github => v1::ProviderKind::Github,
        Provider::Generic => v1::ProviderKind::Generic,
    };
    k as i32
}

// ---- method strings -> Vec<Method> (default-deny on unknown) --------------------------------

/// Parse a single case-insensitive HTTP method string into the engine `Method`. Unknown strings
/// are rejected (`None`) so the caller can fail closed with `invalid_argument`.
pub fn method_from_str(s: &str) -> Option<Method> {
    Some(match s.to_ascii_lowercase().as_str() {
        "get" => Method::Get,
        "head" => Method::Head,
        "post" => Method::Post,
        "put" => Method::Put,
        "patch" => Method::Patch,
        "delete" => Method::Delete,
        "connect" => Method::Connect,
        "options" => Method::Options,
        _ => return None,
    })
}

/// Parse a `method_allow` list; any unknown method is a hard `invalid_argument` (default-deny at the
/// boundary). An empty list stays empty (the engine's `decide` is then default-deny on method).
pub fn methods_from_proto(list: &[String]) -> Result<Vec<Method>, Status> {
    list.iter()
        .map(|m| {
            method_from_str(m).ok_or_else(|| Status::invalid_argument(format!("unknown method '{m}'")))
        })
        .collect()
}

// ---- AddSecretReq -> (SecretMeta, body) ------------------------------------------------------

/// Build the engine `SecretMeta` from an `AddSecretReq`. (`overwrite` is advisory: the engine
/// versions additively, always appending `max+1`, so it is dropped here.)
pub fn add_req_to_meta(req: &v1::AddSecretReq) -> SecretMeta {
    SecretMeta {
        name: req.name.clone(),
        provider: provider_from_proto(req.provider),
        note: req.note.clone(),
        broker_only: req.broker_only,
    }
}

// ---- CreateRelayReq / MintReq -> RelayPolicy -------------------------------------------------

/// Map the proto `DataPlaneMode` (+ the policy's `upstream_base` / a native ttl) to the engine
/// `SwapMode`. `MODE_UNSPECIFIED` defaults to a base-url repoint against `upstream_base`.
pub fn swapmode_from_proto(mode: i32, upstream_base: &str, native_ttl_secs: i64) -> SwapMode {
    match v1::DataPlaneMode::try_from(mode).unwrap_or(v1::DataPlaneMode::BaseUrlRepoint) {
        v1::DataPlaneMode::HttpsProxyMitm => SwapMode::ProxyMitm,
        v1::DataPlaneMode::NativeSubtoken => SwapMode::NativeSubToken {
            ttl_secs: native_ttl_secs,
        },
        v1::DataPlaneMode::BaseUrlRepoint | v1::DataPlaneMode::ModeUnspecified => {
            SwapMode::BaseUrlRepoint {
                upstream_base: upstream_base.to_string(),
            }
        }
    }
}

/// Default policy lifetime when a `CreateRelayReq` carries no parseable `expires_at`: 90 days.
const DEFAULT_POLICY_TTL_SECS: i64 = 90 * 24 * 60 * 60;

/// Best-effort parse of an RFC3339 `expires_at` into a policy TTL (seconds from now). A blank or
/// unparseable value falls back to `DEFAULT_POLICY_TTL_SECS`. (The WIRE bearer is always re-clamped
/// to <=24h by the engine regardless of this value.)
fn policy_ttl_from_expires(expires_at: &str) -> i64 {
    if expires_at.trim().is_empty() {
        return DEFAULT_POLICY_TTL_SECS;
    }
    match chrono::DateTime::parse_from_rfc3339(expires_at) {
        Ok(dt) => {
            let secs = dt.timestamp() - chrono::Utc::now().timestamp();
            if secs > 0 {
                secs
            } else {
                DEFAULT_POLICY_TTL_SECS
            }
        }
        Err(_) => DEFAULT_POLICY_TTL_SECS,
    }
}

/// `CreateRelayReq.policy` -> engine `RelayPolicy`. Unknown methods are rejected (default-deny).
pub fn policy_from_proto(p: &v1::RelayPolicy) -> Result<RelayPolicy, Status> {
    let provider = provider_from_proto(p.provider);
    let swap = swapmode_from_proto(p.mode, &p.upstream_base, DEFAULT_POLICY_TTL_SECS);
    Ok(RelayPolicy {
        relay_id: p.name.clone(),
        kind: if p.ephemeral {
            RelayKind::Ephemeral
        } else {
            RelayKind::Named
        },
        provider,
        secret_name: p.secret_name.clone(),
        swap,
        host_allow: p.host_allow.clone(),
        path_allow: p.path_allow.clone(),
        method_allow: methods_from_proto(&p.method_allow)?,
        policy_ttl_secs: policy_ttl_from_expires(&p.expires_at),
        rate_per_min: (p.rate_per_min != 0).then_some(p.rate_per_min),
        quota_total_requests: (p.quota_total != 0).then_some(p.quota_total),
        quota_total_bytes: None,
        enabled: p.enabled,
        revoked: false,
    })
}

/// Synthesize a minimal `RelayPolicy` for a `Mint` against a relay that has no stored policy (the
/// Phase-6 path: `Relay.Create` is Unimplemented, so a mint carries enough to stand up an ephemeral
/// or named policy from the request alone). `relay_mint` persists it as a side effect. The provider's
/// canonical upstream allowlist (engine `canonical_upstreams`) is the hard egress fence regardless,
/// so an over-broad synthesized `host_allow` cannot widen the actual reachable upstream set.
pub fn mint_req_to_policy(req: &v1::MintReq) -> RelayPolicy {
    let provider = provider_from_proto(req.provider);
    RelayPolicy {
        relay_id: req.relay.clone(),
        kind: if req.ephemeral {
            RelayKind::Ephemeral
        } else {
            RelayKind::Named
        },
        provider,
        secret_name: req.relay.clone(),
        swap: SwapMode::BaseUrlRepoint {
            upstream_base: String::new(),
        },
        host_allow: Vec::new(),
        path_allow: Vec::new(),
        method_allow: Vec::new(),
        policy_ttl_secs: DEFAULT_POLICY_TTL_SECS,
        rate_per_min: None,
        quota_total_requests: None,
        quota_total_bytes: None,
        enabled: true,
        revoked: false,
    }
}

// ---- SecretEvent -> proto Event (the oneof) --------------------------------------------------

/// Convert an engine `SecretEvent` to its proto `Event` twin. Returns `None` for the variants that
/// have NO Event-oneof twin in the proto (`Audit`, `RelayRotated`, `RelayRevoked`, `SecretRead`):
/// these are the cosmetic mirror of durable audit rows and are surfaced through `Audit.Query`, not
/// the control stream. The drain MUST skip `None` (filter), never panic.
pub fn event_to_proto(ev: SecretEvent) -> Option<v1::Event> {
    use v1::event::Kind;
    let kind = match ev {
        SecretEvent::VaultUnlocked { factor } => Kind::VaultUnlocked(v1::VaultUnlocked {
            factor: factor_str(factor),
        }),
        SecretEvent::VaultLocked => Kind::VaultLocked(v1::VaultLocked {}),
        SecretEvent::SecretWritten { name, version } => {
            Kind::SecretWritten(v1::SecretWritten { name, version })
        }
        SecretEvent::RelayMinted {
            relay,
            kind,
            expires_at,
        } => Kind::RelayMinted(v1::RelayMinted {
            relay,
            kind: relaykind_str(kind),
            expires_at,
        }),
        SecretEvent::RelaySwapped {
            relay,
            host,
            method,
            allowed,
            token_id,
            client_uid,
            client_label,
        } => Kind::RelaySwapped(v1::RelaySwapped {
            relay,
            host,
            method,
            allowed,
            token_id,
            client_uid,
            client_label,
        }),
        SecretEvent::GuardRefused { subject, reason } => {
            Kind::GuardRefused(v1::GuardRefused { subject, reason })
        }
        SecretEvent::CaIssued {
            serial,
            cn,
            not_after,
        } => Kind::CaIssued(v1::CaIssued {
            serial,
            cn,
            not_after,
        }),
        SecretEvent::LeafMinted {
            sni,
            relay,
            not_after,
        } => Kind::LeafMinted(v1::LeafMinted {
            sni,
            relay,
            not_after,
        }),
        SecretEvent::Log {
            source,
            stream,
            line,
        } => Kind::Log(v1::Log {
            source,
            stream: match stream {
                envctl_secrets::event::Stream::Stdout => 0,
                envctl_secrets::event::Stream::Stderr => 1,
            },
            line,
        }),
        SecretEvent::ChildExited { code } => Kind::ChildExited(v1::ChildExited { code }),
        SecretEvent::RunFinished { summary } => Kind::RunFinished(v1::RunFinished {
            summary: Some(v1::RunSummary {
                failed: summary.failed,
                refused: summary.refused,
            }),
        }),
        // No proto Event-oneof twin: dropped from the control stream (surfaced via Audit.Query).
        SecretEvent::Audit(_)
        | SecretEvent::RelayRotated { .. }
        | SecretEvent::RelayRevoked { .. }
        | SecretEvent::SecretRead { .. } => return None,
    };
    Some(v1::Event { kind: Some(kind) })
}

// ---- AuditRecord -> proto AuditEntry (for Audit.Query, when wired) ---------------------------

/// `AuditRecord` -> proto `AuditEntry`. `outcome` (Ok/Refused/Failed) has no dedicated proto field,
/// so it is folded into `detail`. `relay`/`token_id` are lifted out of the JSON `detail` when present.
pub fn audit_to_entry(rec: AuditRecord) -> v1::AuditEntry {
    let relay = rec
        .detail
        .get("relay")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| rec.subject.clone())
        .unwrap_or_default();
    let token_id = rec
        .detail
        .get("token_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    v1::AuditEntry {
        at: rec.ts,
        actor: rec.actor_uid.map(|u| u.to_string()).unwrap_or_default(),
        action: format!("{} ({:?})", rec.event_type, rec.outcome),
        target: rec.subject.clone().unwrap_or_default(),
        relay,
        detail: rec.detail.to_string(),
        prev_hash: hex_encode(&rec.prev_hash),
        hash: hex_encode(&rec.row_hash),
        token_id,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
