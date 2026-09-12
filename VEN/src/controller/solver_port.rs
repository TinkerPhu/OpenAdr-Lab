// SolverPort trait — the boundary between the planning task/service and the
// MILP solver. Mirrors SimulatorPort/VtnPort: the trait and its request type
// are domain-level; the implementation (`MilpSolver`) lives in
// `controller::milp_planner`, which remains reachable only through this port.
use chrono::{DateTime, Utc};

use crate::common::TimeSeries;
use crate::controller::asset_milp_port::AssetMilpContext;
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::AssetParams;
use crate::entities::capacity::{AlertWindow, OadrCapacityState, SimpleWindow};
use crate::entities::device_session::{BaselineOverride, EvSession, HeaterTarget, ShiftableLoad};
use crate::entities::plan::Plan;
use crate::entities::planner_params::{PlannerObjective, PlannerParams};
use crate::entities::tariff_snapshot::TariffTimeSeries;

/// Fully-owned inputs for one planning-cycle solve. Built by the caller
/// (`PlanningService::solve_plan`) and moved whole into the solver, which may
/// run on a blocking thread.
pub struct SolveRequest {
    pub asset_contexts: Vec<Box<dyn AssetMilpContext>>,
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
    /// `pv-competence-consolidation` section 4: live `PvInverter`-derived export
    /// ceiling per slot, resolved by the caller from a live `SimState` it still
    /// holds before flattening to `SimSnapshot` (`simulator::plan_context::
    /// resolve_pv_forecast_kw`). Takes precedence over `weather_pv_kw`/the
    /// static-curve fallback in `build_milp_inputs` (it already reflects
    /// weather internally via `PvInverter.weather_forecast`), but not over
    /// `pv_forecast_override`. `None` when no live `"pv"` asset exists.
    pub pv_live_forecast_kw: Option<Vec<f64>>,
    /// `base-load-competence-consolidation`: live `BaseLoad`-derived forecast
    /// per slot, resolved by the caller from the same live `SimState`
    /// (`simulator::plan_context::resolve_base_load_forecast_kw`). `None`
    /// when no live `"base_load"` asset exists.
    pub base_load_live_forecast_kw: Option<Vec<f64>>,
    /// Weather-sourced PV forecast (R-50), already aligned to this cycle's
    /// slot grid. `None` when no weather feed is configured, no
    /// `weather_pv` profile section exists, or the cached forecast has
    /// gone stale — see `entities::solar::weather_pv_kw_for_slots` and
    /// `WeatherForecast::is_fresh`.
    pub weather_pv_kw: Option<Vec<f64>>,
    /// GB-42: history-store-backed diurnal reference series for
    /// HEURISTIC_FORECAST's stale-slot fill, resolved once per cycle
    /// (`services::planning::resolve_diurnal_reference_for_cycle`). `None`
    /// when no `HistoryPort` is configured or it has no data yet.
    pub diurnal_import_eur_kwh: Option<TimeSeries>,
    pub diurnal_co2_g_kwh: Option<TimeSeries>,
}

/// Port to the MILP planning engine. Always infallible: implementations must
/// return a usable `Plan` even on internal solver failure (a fallback plan
/// with a warning), matching `milp_planner::run_planner`'s contract.
pub trait SolverPort: Send + Sync {
    fn solve(&self, req: SolveRequest) -> Plan;
}
