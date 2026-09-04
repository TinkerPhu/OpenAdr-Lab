//! `HeaterEmergencyMode` — split into its own file (Spec A Phase 2a, R-file-size)
//! to keep `heater.rs` under the file-size cap after adding the new trait wiring.

use serde::{Deserialize, Serialize};

/// Which safety-envelope override is active for this tick, if any.
///
/// `temp_min_c`/`temp_max_c` are a comfort/service band, not the asset's true physical
/// limits (see `docs/architecture/VEN_ARCHITECTURE.md`'s Heater section). Outside that band
/// there is a wider safety envelope — ambient temperature on the low side (no physical harm
/// ever), `temp_safety_max_c` on the high side (a real hard ceiling) — that only an active VTN
/// emergency directive should unlock. No such directive is wired in yet; today this is settable
/// via `SimInjectState` (manual/test/demo) or automatically by the deviation arbiter's heater
/// lever (`controller::arbiter`) once enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeaterEmergencyMode {
    /// Normal operation: comfort band enforced as today (emergency heat at temp_min_c,
    /// forced off at temp_max_c).
    Normal,
    /// Emergency curtailment: suppress the forced-on emergency heat at temp_min_c,
    /// letting the tank drift toward ambient. temp_max_c ceiling is unaffected.
    Curtail,
    /// Emergency energy absorption: suppress the forced-off ceiling at temp_max_c,
    /// allowing heating up to temp_safety_max_c instead. temp_min_c floor is unaffected.
    Absorb,
}

impl HeaterEmergencyMode {
    /// Resolve from the two independent SimInjectState override flags. Curtail wins if
    /// both are somehow set (callers shouldn't set both truthy).
    pub fn from_overrides(curtail: Option<bool>, absorb: Option<bool>) -> Self {
        if curtail.unwrap_or(false) {
            Self::Curtail
        } else if absorb.unwrap_or(false) {
            Self::Absorb
        } else {
            Self::Normal
        }
    }
}

impl Default for HeaterEmergencyMode {
    fn default() -> Self {
        Self::Normal
    }
}
