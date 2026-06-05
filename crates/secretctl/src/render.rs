//! Renders the daemon's `Event` stream + unary RPC results to a TTY (envctl `c_ok`/`c_warn`/`c_step`
//! style) or as NDJSON under `--json`. The proto `Event` is prost-derived (not serde), so the JSON
//! shape is built by hand from the oneof.
use envctl_secrets_proto::v1;

// ---- tiny ANSI helpers (mirrors envctl's c_ok/c_warn/c_step) --------------------------------

fn c_ok(s: &str) -> String {
    format!("\x1b[32m{s}\x1b[0m") // green
}
fn c_warn(s: &str) -> String {
    format!("\x1b[33m{s}\x1b[0m") // yellow
}
fn c_step(s: &str) -> String {
    format!("\x1b[36m{s}\x1b[0m") // cyan
}

/// Render one streamed `Event` to stdout, as NDJSON when `json` is set.
pub fn render_event(ev: &v1::Event, json: bool) {
    if json {
        println!("{}", event_to_json(ev));
        return;
    }
    let Some(kind) = &ev.kind else {
        return;
    };
    use v1::event::Kind;
    let line = match kind {
        Kind::VaultUnlocked(e) => c_ok(&format!("vault unlocked (factor: {})", e.factor)),
        Kind::VaultLocked(_) => c_ok("vault locked"),
        Kind::SecretWritten(e) => c_ok(&format!("secret written: {} v{}", e.name, e.version)),
        Kind::RelayMinted(e) => c_ok(&format!(
            "relay minted: {} ({}) expires {}",
            e.relay, e.kind, e.expires_at
        )),
        Kind::RelaySwapped(e) => {
            let verb = if e.allowed { "allowed" } else { "refused" };
            c_step(&format!(
                "relay swap {}: {} {} {}",
                verb, e.method, e.host, e.relay
            ))
        }
        Kind::GuardRefused(e) => c_warn(&format!("refused: {} ({})", e.subject, e.reason)),
        Kind::CaIssued(e) => c_ok(&format!("ca issued: {} ({})", e.cn, e.serial)),
        Kind::LeafMinted(e) => c_ok(&format!("leaf minted: {} for {}", e.sni, e.relay)),
        Kind::Log(e) => {
            let s = if e.stream == 1 { "stderr" } else { "stdout" };
            c_step(&format!("[{}:{}] {}", e.source, s, e.line))
        }
        Kind::ChildExited(e) => c_step(&format!("child exited: {}", e.code)),
        Kind::RunFinished(e) => {
            let (f, r) = e
                .summary
                .as_ref()
                .map(|s| (s.failed.len(), s.refused.len()))
                .unwrap_or((0, 0));
            c_ok(&format!("run finished (failed: {f}, refused: {r})"))
        }
    };
    println!("{line}");
}

/// Build a small `serde_json` object for one `Event` (the oneof mapped by hand).
pub fn event_to_json(ev: &v1::Event) -> String {
    use v1::event::Kind;
    let v = match &ev.kind {
        Some(Kind::VaultUnlocked(e)) => {
            serde_json::json!({ "type": "vault_unlocked", "factor": e.factor })
        }
        Some(Kind::VaultLocked(_)) => serde_json::json!({ "type": "vault_locked" }),
        Some(Kind::SecretWritten(e)) => {
            serde_json::json!({ "type": "secret_written", "name": e.name, "version": e.version })
        }
        Some(Kind::RelayMinted(e)) => serde_json::json!({
            "type": "relay_minted", "relay": e.relay, "kind": e.kind, "expires_at": e.expires_at
        }),
        Some(Kind::RelaySwapped(e)) => serde_json::json!({
            "type": "relay_swapped", "relay": e.relay, "host": e.host, "method": e.method,
            "allowed": e.allowed, "token_id": e.token_id, "client_uid": e.client_uid,
            "client_label": e.client_label
        }),
        Some(Kind::GuardRefused(e)) => {
            serde_json::json!({ "type": "guard_refused", "subject": e.subject, "reason": e.reason })
        }
        Some(Kind::CaIssued(e)) => serde_json::json!({
            "type": "ca_issued", "serial": e.serial, "cn": e.cn, "not_after": e.not_after
        }),
        Some(Kind::LeafMinted(e)) => serde_json::json!({
            "type": "leaf_minted", "sni": e.sni, "relay": e.relay, "not_after": e.not_after
        }),
        Some(Kind::Log(e)) => serde_json::json!({
            "type": "log", "source": e.source, "stream": e.stream, "line": e.line
        }),
        Some(Kind::ChildExited(e)) => serde_json::json!({ "type": "child_exited", "code": e.code }),
        Some(Kind::RunFinished(e)) => {
            let (failed, refused) = e
                .summary
                .as_ref()
                .map(|s| (s.failed.clone(), s.refused.clone()))
                .unwrap_or_default();
            serde_json::json!({ "type": "run_finished", "failed": failed, "refused": refused })
        }
        None => serde_json::json!({ "type": "unknown" }),
    };
    v.to_string()
}

// ---- unary result renderers -----------------------------------------------------------------

pub fn render_status(r: &v1::StatusResp, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "unlocked": r.unlocked, "usb_possessed": r.usb_possessed,
                "active_relays": r.active_relays, "secret_count": r.secret_count
            })
        );
    } else {
        let lock = if r.unlocked {
            c_ok("unlocked")
        } else {
            c_warn("locked")
        };
        println!(
            "{lock}  usb_possessed={}  active_relays={}  secret_count={}",
            r.usb_possessed, r.active_relays, r.secret_count
        );
    }
}

pub fn render_get(r: &v1::GetSecretResp, json: bool) {
    if json {
        let value = if r.revealed {
            Some(String::from_utf8_lossy(&r.value).to_string())
        } else {
            None
        };
        println!(
            "{}",
            serde_json::json!({ "revealed": r.revealed, "value": value })
        );
        return;
    }
    if r.revealed {
        // Owner-only reveal, peercred-gated channel: print the value verbatim.
        print!("{}", String::from_utf8_lossy(&r.value));
    } else {
        println!("{}", c_step("(metadata only; value not revealed)"));
    }
}

pub fn render_mint(r: &v1::MintResp, json: bool) {
    if json {
        // The bearer is owner-only (peercred-gated channel). It is NOT the real key.
        println!(
            "{}",
            serde_json::json!({
                "bearer": r.bearer, "token_id": r.token_id, "expires_at": r.expires_at
            })
        );
    } else {
        println!("{}", c_ok(&format!("minted bearer (token {})", r.token_id)));
        println!("{}", r.bearer);
        println!("{}", c_step(&format!("expires {}", r.expires_at)));
    }
}

pub fn render_revoke(r: &v1::RevokeResp, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({ "count_revoked": r.count_revoked, "dry_run": r.dry_run })
        );
    } else if r.dry_run {
        println!(
            "{}",
            c_warn(&format!(
                "dry-run: would revoke {} (use --apply)",
                r.count_revoked
            ))
        );
    } else {
        println!("{}", c_ok(&format!("revoked {}", r.count_revoked)));
    }
}

pub fn render_audit(r: &v1::AuditQueryResp, json: bool) {
    if json {
        for e in &r.entries {
            println!(
                "{}",
                serde_json::json!({
                    "at": e.at, "actor": e.actor, "action": e.action, "target": e.target,
                    "relay": e.relay, "token_id": e.token_id, "hash": e.hash
                })
            );
        }
    } else {
        for e in &r.entries {
            println!("{} {} {} {}", c_step(&e.at), e.action, e.target, e.detail);
        }
    }
}
