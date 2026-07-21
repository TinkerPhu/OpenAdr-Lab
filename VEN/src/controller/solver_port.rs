// SolverPort trait — the boundary between the planning task/service and the
// MILP solver. Mirrors SimulatorPort/VtnPort: the trait and its request type
// are domain-level; the implementation (`MilpSolver`) lives in
// `controller::milp_planner`, which remains reachable only through this port.
use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::controller::milp_planner::AssetMilpContext;
use crate::controller::simulator_port::SimSnapshot;
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::AssetParams;
use crate::entities::capacity::{AlertWindow, OadrCapacityState, SimpleWindow};
use crate::entities::design_vocabulary::AssetHeuristics;
use crate::entities::device_session::{BaselineOverride, EvSession, HeaterTarget, ShiftableLoad};
use crate::entities::plan::Plan;
use crate::entities::planner_params::{PlannerObjective, PlannerParams};
use crate::entities::tariff_snapshot::TariffTimeSeries;

/// Fully-owned inputs for one planning-cycle solve. Built by the caller
/// (`PlanningService::solve_plan`) and moved whole into the solver, which may
/// run on a blocking thread.
pub struct SolveRequest {
    pub asset_contexts: Vec<Box<dyn AssetMilpContext>>,
    pub assets: SimSnapshot,
    pub tariffs: TariffTimeSeries,
    pub capacity: OadrCapacityState,
    /// WP3.1 (BL-04): active grid-alert windows — planner clamps the
    /// contractual import cap to 0 for slots overlapping any of these.
    pub alert_windows: Vec<AlertWindow>,
    /// WP3.2: active SIMPLE load-shed windows (levels 1–3) — planner clamps
    /// the per-slot import cap per level; see `SimpleWindow`.
    pub simple_windows: Vec<SimpleWindow>,
    pub planner: PlannerParams,
    pub grid_max_import_kw: f64,
    pub grid_max_export_kw: f64,
    pub asset_params: Vec<AssetParams>,
    pub now: DateTime<Utc>,
    pub trigger: PlanTrigger,
    pub ev_session: Option<EvSession>,
    pub heater_target: Option<HeaterTarget>,
    pub shiftable_loads: Vec<ShiftableLoad>,
    pub baseline_override: Option<BaselineOverride>,
    pub objective_override: Option<PlannerObjective>,
    pub pv_forecast_override: Option<f64>,
    /// WP5.2 (BL-14): learned per-asset heuristics, keyed by asset_id —
    /// resolved from `AppState` once per cycle, same as `ev_session`/
    /// `heater_target` above.
    pub asset_heuristics: HashMap<String, AssetHeuristics>,
    /// Weather-sourced PV forecast (R-50), already aligned to this cycle's
    /// slot grid. `None` when no weather feed is configured, no
    /// `weather_pv` profile section exists, or the cached forecast has
    /// gone stale — see `entities::solar::weather_pv_kw_for_slots` and
    /// `WeatherForecast::is_fresh`.
    pub weather_pv_kw: Option<Vec<f64>>,
}

/// Port to the MILP planning engine. Always infallible: implementations must
/// return a usable `Plan` even on internal solver failure (a fallback plan
/// with a warning), matching `milp_planner::run_planner`'s contract.
pub trait SolverPort: Send + Sync {
    fn solve(&self, req: SolveRequest) -> Plan;
}
