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
// These conversions return `Result<_, tonic::Status>`; `Status` is intentionally large, so
// `result_large_err` fires across the whole boundary. Boxing gRPC status errors is non-idiomatic
// and would churn every call site for no real gain — allow it module-wide at the proto boundary.
#![allow(clippy::result_large_err)]
use envctl_secrets::broker::{Method, Provider, RelayKind, RelayPolicy, SwapMode};
use envctl_secrets::inject::{DataPlaneMode, ResolvedInjection};
use envctl_secrets::keyslot::{Argon2Params, Factor};
use envctl_secrets::{AuditRecord, SecretEvent, SecretMeta};
use envctl_secrets_proto::v1;
use tonic::Status;

// ---- Vault.Init: forced server-side Argon2 floor + USB-field validation ----------------------

/// The Argon2id KDF params the daemon FORCES for a fresh passphrase keyslot. The client NEVER
/// supplies KDF params over the wire (a hostile/downgraded client could otherwise request a weak
/// `m`/`t` — FS-S13); the daemon pins the hardened defaults (`Argon2Params::default()` =
/// m=1 GiB, t=4, p=4, well above `ARGON2_M_KIB_FLOOR`/`ARGON2_T_COST_FLOOR`). The engine's
/// `init_vault` re-validates these against its floors regardless, so this is belt-and-suspenders.
pub fn forced_argon2_params() -> Argon2Params {
    Argon2Params::default()
}

/// Validate the USB enrollment fields of an `InitReq` and return the PARTUUID to enroll, if any.
/// `enroll_usb` REQUIRES a non-empty `usb_partition_uuid` (the slot selector / identity, OI-5);
/// the keyfile itself is NEVER on the wire — the daemon reads it via the `UsbProbe` seam. An
/// `enroll_usb` without a UUID is a hard `invalid_argument` (fail-closed at the boundary).
pub fn init_usb_uuid(req: &v1::InitReq) -> Result<Option<String>, Status> {
    if !req.enroll_usb {
        return Ok(None);
    }
    let uuid = req.usb_partition_uuid.trim();
    if uuid.is_empty() {
        return Err(Status::invalid_argument(
            "enroll_usb requires a non-empty usb_partition_uuid",
        ));
    }
    Ok(Some(uuid.to_string()))
}

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
            method_from_str(m)
                .ok_or_else(|| Status::invalid_argument(format!("unknown method '{m}'")))
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

// ---- DataPlaneMode <-> proto DataPlaneMode ---------------------------------------------------

/// Engine `inject::DataPlaneMode` -> proto `DataPlaneMode` discriminant (`i32`). The engine's
/// `DataPlaneMode` is the data-plane shape (base-url / proxy-mitm / native-subtoken); the proto
/// twin mirrors it 1:1 (its `MODE_UNSPECIFIED` is reserved and never emitted here).
pub fn dataplane_mode_to_proto(m: DataPlaneMode) -> i32 {
    let k = match m {
        DataPlaneMode::BaseUrlRepoint => v1::DataPlaneMode::BaseUrlRepoint,
        DataPlaneMode::HttpsProxyMitm => v1::DataPlaneMode::HttpsProxyMitm,
        DataPlaneMode::NativeSubtoken => v1::DataPlaneMode::NativeSubtoken,
    };
    k as i32
}

/// Proto `DataPlaneMode` (an `i32` on the wire) -> engine `inject::DataPlaneMode`. `MODE_UNSPECIFIED`
/// (and any unknown discriminant) maps to `BaseUrlRepoint` — default-safe: the most constrained data
/// plane (no CA, no MITM), matching `swapmode_from_proto`'s default.
pub fn dataplane_mode_from_proto(mode: i32) -> DataPlaneMode {
    match v1::DataPlaneMode::try_from(mode).unwrap_or(v1::DataPlaneMode::BaseUrlRepoint) {
        v1::DataPlaneMode::HttpsProxyMitm => DataPlaneMode::HttpsProxyMitm,
        v1::DataPlaneMode::NativeSubtoken => DataPlaneMode::NativeSubtoken,
        v1::DataPlaneMode::BaseUrlRepoint | v1::DataPlaneMode::ModeUnspecified => {
            DataPlaneMode::BaseUrlRepoint
        }
    }
}

/// Derive the data-plane `DataPlaneMode` from an engine `SwapMode` (the field the minted policy
/// carries). `BaseUrlRepoint`/`NativeSubToken`/`ProxyMitm` map 1:1 onto the inject mode the child
/// env is shaped for.
pub fn dataplane_mode_from_swap(swap: &SwapMode) -> DataPlaneMode {
    match swap {
        SwapMode::BaseUrlRepoint { .. } => DataPlaneMode::BaseUrlRepoint,
        SwapMode::ProxyMitm => DataPlaneMode::HttpsProxyMitm,
        SwapMode::NativeSubToken { .. } => DataPlaneMode::NativeSubtoken,
    }
}

// ---- ResolvedInjection <-> proto ResolvedInjection -------------------------------------------

/// Engine `inject::ResolvedInjection` -> proto `ResolvedInjection`. The daemon is authoritative: it
/// builds the env delta (the bearer-only child env) via `inject::injection_template` and ships the
/// resolved shape to the owner-only client. The proto `proxy_url`/`base_url` are plain strings, so an
/// absent (`None`) value is encoded as the empty string (the client re-derives `Option` on the way
/// back via `injection_from_proto`). NEVER carries the real key — only the bearer, by construction.
pub fn injection_to_proto(r: &ResolvedInjection) -> v1::ResolvedInjection {
    v1::ResolvedInjection {
        provider: provider_to_proto(r.provider),
        mode: dataplane_mode_to_proto(r.mode),
        env: r.env.clone().into_iter().collect(),
        ca_env_keys: r.ca_env_keys.clone(),
        proxy_url: r.proxy_url.clone().unwrap_or_default(),
        base_url: r.base_url.clone().unwrap_or_default(),
    }
}

/// Proto `ResolvedInjection` -> engine `inject::ResolvedInjection`. The inverse of
/// [`injection_to_proto`]: the empty-string `proxy_url`/`base_url` sentinel maps back to `None`. Used
/// by the client (`secretctl run`) to reconstruct the engine plan the daemon authoritatively built,
/// so all env/key logic stays in the engine and the daemon stays the single source of truth.
pub fn injection_from_proto(p: &v1::ResolvedInjection) -> ResolvedInjection {
    let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
    ResolvedInjection {
        provider: provider_from_proto(p.provider),
        mode: dataplane_mode_from_proto(p.mode),
        env: p.env.clone().into_iter().collect(),
        ca_env_keys: p.ca_env_keys.clone(),
        proxy_url: opt(&p.proxy_url),
        base_url: opt(&p.base_url),
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

#[cfg(test)]
mod tests {
    use super::*;
    use envctl_secrets::keyslot::{ARGON2_M_KIB_FLOOR, ARGON2_T_COST_FLOOR};

    fn init_req(enroll_usb: bool, uuid: &str) -> v1::InitReq {
        v1::InitReq {
            passphrase: Some("pp".to_string()),
            enroll_usb,
            usb_partition_uuid: uuid.to_string(),
            apply: false,
        }
    }

    #[test]
    fn forced_argon2_is_at_or_above_the_floor() {
        // The daemon forces a server-side floor; it must never sit below the engine's downgrade
        // floors (FS-S13). This is the belt to the engine's suspenders.
        let p = forced_argon2_params();
        assert!(
            p.m_kib >= ARGON2_M_KIB_FLOOR,
            "m_kib below floor: {}",
            p.m_kib
        );
        assert!(
            p.t_cost >= ARGON2_T_COST_FLOOR,
            "t_cost below floor: {}",
            p.t_cost
        );
        assert!(p.p_lanes >= 1);
    }

    #[test]
    fn init_usb_uuid_none_when_not_enrolling() {
        assert_eq!(init_usb_uuid(&init_req(false, "")).unwrap(), None);
        // A stray uuid without enroll_usb is ignored (no USB slot is enrolled).
        assert_eq!(init_usb_uuid(&init_req(false, "SOME-UUID")).unwrap(), None);
    }

    #[test]
    fn init_usb_uuid_requires_uuid_when_enrolling() {
        // FAIL-CLOSED: enroll_usb with no/blank partuuid is invalid_argument.
        let err = init_usb_uuid(&init_req(true, "")).unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        let err = init_usb_uuid(&init_req(true, "   ")).unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn init_usb_uuid_trims_and_returns_uuid() {
        assert_eq!(
            init_usb_uuid(&init_req(true, "  E2E-USB-1234 ")).unwrap(),
            Some("E2E-USB-1234".to_string())
        );
    }

    // ---- ResolvedInjection round-trip (engine <-> proto) -----------------------------------

    #[test]
    fn injection_to_proto_carries_bearer_base_url_and_provider() {
        // The daemon-built BaseUrlRepoint injection for Anthropic: child sees the loopback proxy as
        // ANTHROPIC_BASE_URL and the BEARER (never the real key) as ANTHROPIC_API_KEY.
        const BEARER: &str = "relay-bearer-DO-NOT-LEAK";
        const PROXY: &str = "http://127.0.0.1:54321";
        let r = envctl_secrets::inject::injection_template(
            Provider::Anthropic,
            BEARER,
            PROXY,
            "",
            DataPlaneMode::BaseUrlRepoint,
        );
        let p = injection_to_proto(&r);
        assert_eq!(p.provider, provider_to_proto(Provider::Anthropic));
        assert_eq!(
            p.mode,
            dataplane_mode_to_proto(DataPlaneMode::BaseUrlRepoint)
        );
        assert_eq!(
            p.env.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some(PROXY)
        );
        assert_eq!(
            p.env.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some(BEARER)
        );
        assert_eq!(p.base_url, PROXY);
        // BaseUrlRepoint carries no proxy_url / no CA keys.
        assert!(p.proxy_url.is_empty());
        assert!(p.ca_env_keys.is_empty());
    }

    #[test]
    fn injection_round_trips_through_proto() {
        // proto -> engine -> proto is lossless for every field (the empty-string proxy_url/base_url
        // sentinel maps to None and back). Cover a MITM injection so ca_env_keys + proxy_url are
        // populated and base_url is absent.
        const BEARER: &str = "bearer-xyz";
        const PROXY: &str = "http://127.0.0.1:7000";
        const CA: &str = "/run/envctl/mitm-ca.pem";
        let engine = envctl_secrets::inject::injection_template(
            Provider::Anthropic,
            BEARER,
            PROXY,
            CA,
            DataPlaneMode::HttpsProxyMitm,
        );
        let proto = injection_to_proto(&engine);
        let back = injection_from_proto(&proto);
        assert_eq!(back.provider, engine.provider);
        assert_eq!(back.mode, engine.mode);
        assert_eq!(back.env, engine.env);
        assert_eq!(back.ca_env_keys, engine.ca_env_keys);
        assert_eq!(back.proxy_url, engine.proxy_url);
        assert_eq!(back.base_url, engine.base_url);
        // Re-encoding the reconstructed engine value yields the identical proto.
        let proto2 = injection_to_proto(&back);
        assert_eq!(proto2.env, proto.env);
        assert_eq!(proto2.proxy_url, proto.proxy_url);
        assert_eq!(proto2.base_url, proto.base_url);
    }

    #[test]
    fn dataplane_mode_from_swap_maps_each_variant() {
        assert_eq!(
            dataplane_mode_from_swap(&SwapMode::BaseUrlRepoint {
                upstream_base: "https://api.anthropic.com".to_string()
            }),
            DataPlaneMode::BaseUrlRepoint
        );
        assert_eq!(
            dataplane_mode_from_swap(&SwapMode::ProxyMitm),
            DataPlaneMode::HttpsProxyMitm
        );
        assert_eq!(
            dataplane_mode_from_swap(&SwapMode::NativeSubToken { ttl_secs: 60 }),
            DataPlaneMode::NativeSubtoken
        );
    }
}
