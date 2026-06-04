//! Presence-gate abstraction (audit F14, SERVER-MODE §5.1).
//!
//! The engine's sole egress gate input is generalized from a USB-specific
//! `usb_absent_since_ms` to a topology-agnostic `gate_absent_since_ms`, fed by a single
//! [`PresenceGate`]. Profile A (the daemon runs on the box that physically holds the USB)
//! implements it with a USB-possession probe; Profile B (VPS) would implement it with an
//! operator-box presence-token verifier. `decide()` consumes ONLY the resolved
//! `gate_absent_since_ms`, so there is one choke point and a substitute factor can never be
//! bolted beside `decide()` (REQ-SEC-13).

/// Resolved presence-gate state.
///
/// `decide()` treats [`GateState::Unproven`] EXACTLY like [`GateState::AbsentSince`]`(now)` —
/// immediate deny, no grace — so an unproven/unconfigured/misread gate fails closed
/// (SERVER-MODE §5.1, REQ-SEC-13).
///
/// Representation note: the absent-since timestamp is carried as wall-clock epoch
/// milliseconds (`i64`), consistent with every other engine gate/clock input (`now_ms`,
/// `issuance_floor_ms`) and the `gate_absent_since_ms: Option<i64>` that `decide()` consumes.
/// This is a deliberate deviation from the illustrative `AbsentSince(Instant)` in the spec
/// sketch: an `Instant` has no epoch anchor and would force a lossy conversion at the single
/// mapping site ([`gate_absent_since_ms`]); the engine is wall-ms throughout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateState {
    /// The presence factor is currently proven (USB present / a valid operator-box token).
    Present,
    /// The factor has been absent since this wall-clock epoch-ms reading.
    AbsentSince(i64),
    /// The factor's state cannot be established (no probe, no valid token, gate
    /// misconfigured). Fails closed: mapped to `AbsentSince(now)` — immediate deny, no grace.
    Unproven,
}

/// The daemon-side presence factor. Profile A: a USB-possession probe. Profile B: an
/// operator-box presence-token verifier. Defined engine-side so the gate is a single typed
/// choke point the daemon injects into — the engine itself never opens a socket, reads a
/// USB, or verifies a token.
pub trait PresenceGate: Send + Sync {
    /// Resolve the current gate state. Implementations MUST fail closed
    /// ([`GateState::Unproven`]) on any uncertainty rather than guessing `Present`.
    fn resolve(&self) -> GateState;
}

/// Map a resolved [`GateState`] to the `gate_absent_since_ms` value `decide()` consumes.
///
/// [`GateState::Unproven`] collapses to `AbsentSince(now_ms)` (REQ-SEC-13: treated exactly
/// like absent-now), so any uncertainty denies immediately with no grace.
#[must_use]
pub fn gate_absent_since_ms(state: GateState, now_ms: i64) -> Option<i64> {
    match state {
        GateState::Present => None,
        GateState::AbsentSince(ms) => Some(ms),
        GateState::Unproven => Some(now_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_opens_the_gate() {
        // Proven presence => no absence timestamp => `decide()`'s gate check passes.
        assert_eq!(gate_absent_since_ms(GateState::Present, 1_000), None);
    }

    #[test]
    fn absent_since_passes_the_timestamp_through() {
        assert_eq!(gate_absent_since_ms(GateState::AbsentSince(42), 1_000), Some(42));
    }

    #[test]
    fn unproven_is_absent_now_fail_closed() {
        // REQ-SEC-13: Unproven is treated EXACTLY like AbsentSince(now) — Some(now) drives the
        // GateAbsent deny in `decide()`, with no grace. A VPS/misconfigured gate denies here.
        assert_eq!(gate_absent_since_ms(GateState::Unproven, 1_000), Some(1_000));
    }
}
