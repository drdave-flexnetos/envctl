//! Manual runtime smoke for the Cognitum Seed possession factor (`seed-factor`).
//!
//! Drives the crate's PUBLIC API against a *live* Seed — the real library surface, not a unit
//! test. Exercises the USB-possession seam (`RealUsbProbe::keyfile_for`, a direct pure-Rust HTTPS
//! call to the Seed's REST custody API, TLS-pinned to the Cognitum CA) and the presence gate
//! (`SeedPresenceGate::resolve`, random-challenge + ring verify).
//!
//! ```bash
//! ENVCTL_SEED_PUBKEY=<64-hex device key> \
//!   cargo run -p envctl-secrets-engine --example seed_factor_probe --features seed-factor
//! ```
//! Overrides: `ENVCTL_SEED_API` (default `https://169.254.42.1:8443`), `ENVCTL_SEED_CA`
//! (default `/usr/local/share/ca-certificates/cognitum-ca.crt`), `ENVCTL_SEED_TOKEN[_FILE]`,
//! `ENVCTL_SEED_KEK_CONTEXT`.

#[cfg(feature = "seed-factor")]
fn main() {
    use envctl_secrets::broker::{PresenceGate, SeedPresenceGate};
    use envctl_secrets::{RealUsbProbe, UsbProbe};

    let target =
        std::env::var("ENVCTL_SEED_API").unwrap_or_else(|_| "https://169.254.42.1:8443".into());
    println!("== seed-factor runtime probe (target {target}) ==");

    // 1) USB possession seam → deterministic KEK material from the Seed's device key.
    let probe = RealUsbProbe;
    match probe.keyfile_for("verify-partuuid") {
        Some(bytes) => {
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            println!("[1] keyfile_for -> Some({} bytes)\n    {hex}", bytes.len());
            let again = probe.keyfile_for("verify-partuuid");
            let identical = again.as_deref().map(Vec::as_slice) == Some(bytes.as_slice());
            println!("[1b] second call identical? {identical}");
        }
        None => println!("[1] keyfile_for -> None (Seed unreachable / unpaired / malformed)"),
    }

    // 2) Presence gate (Profile S): random challenge, ring-verified against the pinned pubkey.
    let has_key = std::env::var("ENVCTL_SEED_PUBKEY").is_ok();
    let gate = SeedPresenceGate::from_env();
    println!(
        "[2] ENVCTL_SEED_PUBKEY set? {has_key} -> resolve() = {:?}",
        gate.resolve()
    );
}

#[cfg(not(feature = "seed-factor"))]
fn main() {
    eprintln!("seed_factor_probe: build with --features seed-factor");
}
