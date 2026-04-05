/// Named thresholds for floating-point comparisons.
///
/// Group constants by the *physical quantity* they guard so every call site
/// can resolve to one name instead of an unexplained magic literal.
///
/// Test-only float-equality assertions (e.g. `(v - 5.0).abs() < 1e-9`) are
/// intentionally NOT covered here — they express "computed value equals
/// expected value after arithmetic", which is a different concern.

/// Power or capacity below this is treated as "effectively zero" in scheduling
/// and envelope decisions.  1 W — below any real meter resolution in a
/// home-energy system.  Replaces the inconsistent mix of `1e-6` and `1e-3`
/// that appeared on the same conditions in the planner rules.
pub const NEAR_ZERO_KW: f64 = 1e-3;

/// Undelivered energy below this is skipped in packet-selection loops.
/// 1 mWh — negligible relative to the smallest meaningful energy packet.
pub const NEAR_ZERO_KWH: f64 = 1e-6;

/// Packet is considered complete when remaining energy is within this margin.
/// 100 mWh — absorbs 1-second tick rounding at typical residential power levels.
pub const COMPLETION_TOL_KWH: f64 = 1e-4;

/// Minimum flexibility energy required to include an asset in the flexibility
/// envelope.  1 Wh — below this an envelope entry adds noise without useful
/// DR headroom.
pub const FLEX_ENERGY_MIN_KWH: f64 = 1e-3;

/// Minimum measured power to transition a packet from Scheduled → Active.
/// 10 W — ensures the asset is meaningfully delivering, not just sensor noise.
pub const ACTIVE_THRESHOLD_KW: f64 = 1e-2;

/// Denominator guard: prevents division by zero in ratio expressions.
/// Not a physical threshold — purely arithmetic safety.
pub const DIV_GUARD: f64 = 1e-9;
