//! What the grid is telling this site, for one plan cycle.
//!
//! These five travelled as five separate values through three flat parameter
//! lists in a row — `tasks::planning::cycle`'s 26 arguments to
//! `build_solve_request`, `SolveRequest`'s 23 fields, and `run_planner`'s 21
//! parameters — so nothing in the planner's signature said which of its
//! inputs were the grid's side of the conversation.
//!
//! The grouping is not invented here: `state/grid_signals.rs` already holds
//! exactly these accessors (`planned_tariffs`, `capacity_state`,
//! `planned_capacity_limits`, `alert_windows`, `simple_windows`), so the
//! concept was named one layer up and flattened again at this boundary. This
//! restores the name the codebase had already chosen.
//!
//! Deliberately *not* included, though they are arguably grid data:
//! `diurnal_import_eur_kwh`/`diurnal_co2_g_kwh` (GB-42). Those come from the
//! history store rather than the VTN, and exist to fill gaps in `tariffs`
//! rather than to state a limit — a different source and a different job. If
//! a later pass finds they read better in here, moving them is additive.

use crate::entities::capacity::{AlertWindow, CapacitySnapshot, OadrCapacityState, SimpleWindow};
use crate::entities::tariff_snapshot::TariffTimeSeries;

/// No `Serialize`/`Default`: `TariffTimeSeries` derives neither, and adding
/// them to it to decorate this struct would be the tail wagging the dog.
/// `SolveRequest`, which owns one of these, derives nothing either.
#[derive(Debug, Clone)]
pub struct GridSignals {
    /// Prices over the planning horizon, after the stale-rate policy has
    /// filled any gap the VTN left (`controller::milp_planner::stale_rates`).
    pub tariffs: TariffTimeSeries,
    /// The limits in force right now, as of the last poll.
    pub capacity: OadrCapacityState,
    /// GB-48: priority-resolved capacity-limit schedule
    /// (`planned_capacity_limits`) — the planner caps each slot by the
    /// tightest limit overlapping it.
    pub capacity_schedule: Vec<CapacitySnapshot>,
    /// WP3.1 (BL-04): active grid-alert windows — planner clamps the
    /// contractual import cap to 0 for slots overlapping any of these.
    pub alert_windows: Vec<AlertWindow>,
    /// WP3.2: active SIMPLE load-shed windows (levels 1–3) — planner clamps
    /// the per-slot import cap per level; see `SimpleWindow`.
    pub simple_windows: Vec<SimpleWindow>,
}
