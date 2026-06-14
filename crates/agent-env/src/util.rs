//! Small time helpers — ported verbatim from kasetto v3.2.0 `src/fsops/mod.rs`
//! (ledger XC-04).
//!
//! Used by the lock/report layer to stamp `updated_at`. A clock skew before the UNIX
//! epoch is treated as `0` (kasetto's `unwrap_or(0)`), so these never error.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the UNIX epoch (saturating to `0` on a pre-epoch clock).
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// [`now_unix`] rendered as a decimal string.
pub fn now_unix_str() -> String {
    now_unix().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_unix_is_after_2020() {
        // 2020-01-01T00:00:00Z — guards against a zeroed/saturated clock regression.
        assert!(now_unix() > 1_577_836_800);
    }

    #[test]
    fn now_unix_str_matches_now_unix() {
        let s = now_unix_str();
        let parsed: u64 = s.parse().expect("decimal");
        // The two calls straddle at most one second.
        assert!(now_unix() - parsed <= 1);
    }
}
