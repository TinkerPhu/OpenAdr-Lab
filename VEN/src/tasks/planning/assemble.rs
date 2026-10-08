//! Gathering everything one solve needs, as one named step.
//!
//! This was ~75 lines in the middle of `cycle.rs`: two more `AppState` reads,
//! four calls into `services::planning` and `simulator::plan_context`, and
//! eleven intermediate locals threaded between them — all sitting between
//! "spawn the progress ticker" and "run the solve". The cycle was doing two
//! jobs at once, and only one of them was orchestration.
//!
//! It stays in `tasks/` rather than moving into `services/`, deliberately.
//! Assembly needs concrete `SimState` (`build_asset_contexts`,
//! `resolve_pv_forecast_kw`, `resolve_base_load_forecast_kw`), and both
//! `.claude/CLAUDE.md`'s ring map and `simulator/plan_context.rs`'s own module
//! doc say `services/` reaches the simulator only through `SimulatorPort`.
//! The adapter ring may touch infra; the application ring may not. So this
//! separates two jobs inside the correct ring — it is not a layering fix,
//! because under that rule the layering here was already right.
//!
//! (Two services do hold concrete `SimState` today —
//! `services::forecast` and `services::user_request` — which contradicts that
//! rule. `scripts/audit_ven_architecture.py` now reports them instead of
//! leaving the gap in prose. Moving assembly inward would have added a third
//! rather than settled the question.)

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::controller::{HistoryPort, SolveRequest, WeatherForecastPort};
use crate::entities::asset::PlanTrigger;
use crate::entities::asset_params::{AssetParams, PvForecastParams};
use crate::entities::grid_signals::GridSignals;
use crate::entities::plan::Plan;
use crate::entities::planner_params::PlannerParams;
use crate::services::planning::PlanCycleInputs;
use crate::simulator::plan_context::{
    build_asset_contexts, resolve_base_load_forecast_kw, resolve_pv_forecast_kw,
};
use crate::simulator::SimState;
use crate::state::AppState;

use super::cycle_state::CycleState;

/// The cycle's own context, as against the per-cycle state in `CycleState`:
/// the ports and the values fixed for this process's lifetime.
///
/// A struct rather than a parameter list because `assemble_solve_request`
/// would otherwise take twelve, which is the shape this whole refactor is
/// about (see `SolveRequest`'s own history in `entities::grid_signals`).
pub(super) struct SolveAssembly<'a> {
    pub state: &'a AppState,
    /// Already cloned out from under the simulator's Mutex by the caller, and
    /// already patched with any pending PV inject — assembly must not block
    /// sim ticks for the duration of a solve.
    pub sim_snap: &'a SimState,
    pub planner: &'a PlannerParams,
    pub asset_params: &'a [AssetParams],
    pub grid_max_import_kw: f64,
    pub grid_max_export_kw: f64,
    pub weather: &'a Arc<dyn WeatherForecastPort>,
    pub weather_pv_params: Option<&'a PvForecastParams>,
    pub history: Option<&'a Arc<dyn HistoryPort>>,
    /// Step-aligned cycle instant — the slot grid's origin.
    pub now: DateTime<Utc>,
    /// True wall clock, for anything measuring real elapsed time.
    pub wall_now: DateTime<Utc>,
    pub trigger: PlanTrigger,
    /// The plan in force, which the heater anchor pins against. Read by the
    /// caller because it needs the same value again after the solve, for the
    /// adoption gate's "previous plan".
    pub current_plan: Option<&'a Plan>,
}

/// Everything one solve is built from, gathered from the ports.
///
/// Consumes `st`: the EV session, heater target, shiftable loads and baseline
/// override move into the request rather than being cloned. The caller takes
/// the two `Copy` fields it still needs (`obj`) before handing it over.
pub(super) async fn assemble_solve_request(a: SolveAssembly<'_>, st: CycleState) -> SolveRequest {
    // Read before the blocking solve so heater tiers pin to the last adopted plan.
    let anchor_until = a.state.anchor_until().await;
    let PlanCycleInputs {
        tariff_ts,
        n_slots,
        cum_s,
        lambda_sw,
        c_terminal_eur_kwh_by_asset,
        heater_anchor,
    } = crate::services::planning::build_plan_cycle_inputs(
        &st.rates,
        a.planner,
        a.asset_params,
        a.current_plan,
        anchor_until,
        a.now,
    );

    // Assembled after `build_plan_cycle_inputs`, not at the state reads,
    // because `tariffs` is the stale-rate-processed series it just returned.
    let grid = GridSignals {
        tariffs: tariff_ts,
        capacity: a.state.capacity_state().await,
        capacity_schedule: a.state.planned_capacity_limits().await,
        alert_windows: a.state.alert_windows().await,
        simple_windows: a.state.simple_windows().await,
    };

    // Per-asset MILP contexts from live simulator state. Before the blocking
    // solve, so asset states are captured at this instant.
    let asset_contexts = build_asset_contexts(
        a.sim_snap,
        n_slots,
        &cum_s,
        a.now,
        st.ev_sessions.as_slice(), // the EV decides what it can serve
        st.heat_tgt.as_ref(),
        a.asset_params,
        a.planner,
        lambda_sw,
        &c_terminal_eur_kwh_by_asset,
        &heater_anchor,
        &a.state.comfort_overrides_map().await,
    );

    // Each asset's own forecast, from the asset rather than reconstructed by a
    // site-level caller: `None` when that asset is not live this cycle.
    let pv_live_forecast_kw = resolve_pv_forecast_kw(a.sim_snap, n_slots, &cum_s, a.now);
    let base_load_live_forecast_kw =
        resolve_base_load_forecast_kw(a.sim_snap, n_slots, &cum_s, a.now);

    // R-50: resolves the weather-sourced PV forecast internally.
    crate::services::planning::build_solve_request(
        asset_contexts,
        grid,
        a.planner.clone(),
        a.grid_max_import_kw,
        a.grid_max_export_kw,
        a.asset_params.to_vec(),
        a.now,
        a.trigger,
        st.ev_sessions.upcoming(a.now).cloned(), // next commitment, not just an open window
        st.heat_tgt,
        st.shift_loads,
        st.bl_override,
        Some(st.obj),
        st.pv_forecast_override,
        pv_live_forecast_kw,
        base_load_live_forecast_kw,
        a.weather,
        a.weather_pv_params,
        a.wall_now,
        &cum_s,
        n_slots,
        a.history,
        a.state.pv_snow_state().await,
    )
    .await
}
