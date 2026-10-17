//! The site's flexibility headroom as the capacity seam reports it: the instant value, its
//! retained samples and its forward trajectory. Split out of `plan.rs` (500-production-line cap):
//! none of it is part of a plan, which only carries the forecast slots it was solved with.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Live site-level flexibility available to the grid right now (§9).
///
/// Computed by `simulator::site_headroom::compute_site_headroom` — independent
/// of the active plan. Always queryable without triggering a planning cycle.
///
/// `up_kw`/`down_kw` are SIGNED now (`site-capacity-seam-unification`,
/// matching `CapacityCurveStep::power_kw`'s convention: positive = import,
/// negative = export) — literally the `t2 = 0` point of
/// `simulator::capacity_headroom::compute_site_capacity_curve`, called once
/// per direction, not an independent sum. `up_kw` = the site's net signed
/// power under a sustained Export commitment (negative when genuinely
/// exportable; can swing positive/net-importing when non-exportable
/// contributions like base load's draw exceed what's exportable — see
/// `merge_events`'s doc comment); `down_kw` = the net signed power under a
/// sustained Import commitment (positive in the normal case). Both are
/// clamped to the site's genuine physical/interconnection rating, not any
/// VTN directive (R-72).
///
/// Duration fields estimate how long the VEN can sustain the headroom based
/// on available storage energy. None if no storage assets are present.
#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
pub struct SiteHeadroom {
    pub ts: DateTime<Utc>,
    /// Net signed power (kW) under a sustained Export commitment. See this
    /// struct's own doc comment for the sign convention.
    pub up_kw: f64,
    /// Net signed power (kW) under a sustained Import commitment. See this
    /// struct's own doc comment for the sign convention.
    pub down_kw: f64,
    /// Estimated duration up_kw can be sustained, in seconds. None = no storage.
    pub up_duration_s: Option<u64>,
    /// Estimated duration down_kw can be sustained, in seconds. None = no storage.
    pub down_duration_s: Option<u64>,
}

/// One historical sample of a `SiteHeadroom`, retained in a bounded
/// in-memory ring (`AppState::flexibility_history`, BL-43) so the UI can plot a
/// live headroom band over time — `SiteHeadroom` itself is
/// instant-only and has no forward schedule (see its own doc comment).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SiteHeadroomSample {
    pub ts: DateTime<Utc>,
    pub up_kw: f64,
    pub down_kw: f64,
}

impl From<&SiteHeadroom> for SiteHeadroomSample {
    fn from(env: &SiteHeadroom) -> Self {
        Self {
            ts: env.ts,
            up_kw: env.up_kw,
            down_kw: env.down_kw,
        }
    }
}

/// One future slot of the forward-looking site headroom trajectory —
/// distinct from `SiteHeadroom`, which is instant-only.
///
/// Re-derived fresh every dispatcher tick (not read statically off
/// `PlanTimeSlot.planned_state_by_asset`, a stale solve-time-only snapshot):
/// each asset's REAL current state is re-simulated forward via
/// `Asset::simulate_forward`, driven by the active plan's own
/// `planned_kw_by_asset` schedule, so this trajectory self-corrects for any
/// drift between the plan's assumptions and reality on every tick — see
/// `simulator::capacity_headroom::compute_site_headroom_forecast`.
///
/// **`up_kw`/`down_kw` are ABSOLUTE, not a delta from the plan's own chosen
/// dispatch** (`unified-capacity-envelope-engine`, Spec E). **Signed now**
/// too (`site-capacity-seam-unification`), matching `SiteHeadroom`/
/// `CapacityCurveStep::power_kw`'s convention — see `SiteHeadroom`'s
/// own doc comment for the sign convention and why base load (now included)
/// can push `up_kw` positive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
pub struct SiteHeadroomForecastSlot {
    pub ts: DateTime<Utc>,
    /// Absolute max achievable Export at this slot (kW). Always ≥ 0.
    pub up_kw: f64,
    /// Absolute max achievable Import at this slot (kW). Always ≥ 0.
    pub down_kw: f64,
}
