//! Builds `MilpInputs`, the planner's complete input parameter set, from the asset contexts,
//! the grid signals and the site's own forecasts.
//!
//! Owns the order of the stages (`input_stages`) and the one full spelling of `MilpInputs`'s
//! fields, so a new field is a compile error here and nowhere else. It decides nothing itself:
//! every rule (stale rates, the PV precedence chain, SIMPLE and alert caps, the baseline
//! override) lives in its stage. It must stay infallible and free of logging; warnings travel as
//! fields of the result.

use chrono::{DateTime, Utc};

use super::input_stages::{
    apply_baseline_override, asset_scalars, capacity_limits, ev_budget_warning, forecast_series,
    slot_bounds, tariff_series,
};
use super::types::*;
use crate::controller::milp_planner::AssetMilpContext;
use crate::entities::asset_params::{BaseLoadParams, PvParams};
use crate::entities::device_session::BaselineOverride;
use crate::entities::grid_signals::GridSignals;
use crate::entities::planner_params::PlannerParams;
use crate::entities::time_grid::TimeGrid;
use lab_core::time_series::TimeSeries;

/// What the site contributes besides its schedulable assets: the grid connection's physical
/// limits and where the PV and base-load forecasts come from.
pub(crate) struct SiteInputs<'a> {
    pub grid_max_import_kw: f64,
    pub grid_max_export_kw: f64,
    pub pv_cfg: Option<&'a PvParams>,
    pub base_load: Option<&'a BaseLoadParams>,
    pub baseline_override: Option<&'a BaselineOverride>,
    /// Deterministic-testing pin: always wins over every other PV source.
    pub pv_forecast_override: Option<f64>,
    /// pv-competence-consolidation section 4: live PvInverter-derived export
    /// ceiling per slot (simulator::plan_context::resolve_pv_forecast_kw).
    /// None when no live "pv" asset exists this cycle.
    pub pv_live_forecast_kw: Option<&'a [f64]>,
    /// base-load-competence-consolidation: live BaseLoad-derived forecast per
    /// slot (simulator::plan_context::resolve_base_load_forecast_kw). None
    /// when no live "base_load" asset exists this cycle.
    pub base_load_live_forecast_kw: Option<&'a [f64]>,
    /// Weather-sourced PV forecast (R-50), pre-aligned to this call's own
    /// slot grid by the caller (entities::solar::weather_pv_kw_for_slots).
    /// Only consulted when pv_live_forecast_kw is None (no live "pv" asset
    /// this cycle) — when a live asset exists, its own weather_forecast field
    /// already factors weather in, so this and pv_live_forecast_kw are never
    /// both meaningfully in play for the same slot. Falls back further to the
    /// static pv_cfg sin-model curve when this is also None.
    pub weather_pv_kw: Option<&'a [f64]>,
}

/// GB-42: history-store-backed diurnal reference series for HEURISTIC_FORECAST's
/// 168h-back (day-type-mismatch) lookback, resolved once per cycle by the caller
/// (services::planning::build_solve_request). `None` when history is disabled or
/// has no data yet for a series.
pub(crate) struct StaleRateRefs<'a> {
    pub import: Option<&'a TimeSeries>,
    pub co2: Option<&'a TimeSeries>,
}

/// Build the full MILP input parameter set from asset contexts and current runtime state.
///
/// Asset-specific parameters (battery, EV, heater, shiftable loads) come through the
/// `AssetMilpContext` trait; prices and limits from `grid`; PV and base load from `site`.
pub(crate) fn build_milp_inputs(
    asset_contexts: &[Box<dyn AssetMilpContext>],
    grid: &GridSignals,
    planner: &PlannerParams,
    site: &SiteInputs,
    refs: &StaleRateRefs,
    now: DateTime<Utc>,
) -> MilpInputs {
    let time = TimeGrid::from_zones(&planner.plan_zones);
    let bounds = slot_bounds(&time, now);
    let tariffs = tariff_series(&grid.tariffs, planner, &bounds, refs);
    let (p_pv_kw, mut p_base_kw) = forecast_series(&bounds, site);
    // Before the override: SIMPLE level 2 caps import at the forecast base load.
    let limits = capacity_limits(grid, planner, site, &bounds, &p_base_kw);
    apply_baseline_override(&mut p_base_kw, &time, now, site.baseline_override);
    let assets = asset_scalars(asset_contexts, time.n, now);
    let budget_warning = ev_budget_warning(
        &tariffs.c_imp_eur_kwh,
        assets.ev_budget_eur,
        assets.e_ev_extra_max_kwh,
    );

    MilpInputs {
        n: time.n,
        dt_h: time.dt_h,
        cum_s: time.cum_s,
        c_imp_eur_kwh: tariffs.c_imp_eur_kwh,
        rate_stale: tariffs.rate_stale,
        stale_rate_warning: tariffs.stale_rate_warning,
        co2_stale_rate_warning: tariffs.co2_stale_rate_warning,
        budget_warning,
        c_exp_eur_kwh: tariffs.c_exp_eur_kwh,
        g_imp_kgco2_kwh: tariffs.g_imp_kgco2_kwh,
        p_pv_kw,
        p_base_kw,
        p_imp_max_phys_kw: limits.p_imp_max_phys_kw,
        p_exp_max_phys_kw: limits.p_exp_max_phys_kw,
        p_imp_max_cont_kw: limits.p_imp_max_cont_kw,
        p_exp_max_cont_kw: limits.p_exp_max_cont_kw,
        pen_imp_eur_kwh: planner.pen_imp_eur_kwh,
        pen_exp_eur_kwh: planner.pen_exp_eur_kwh,
        mip_gap_target: planner.mip_gap_target,
        penalty_rules: planner.penalty_rules.clone(),
        e_bat_nom_kwh: assets.e_bat_nom_kwh,
        e_bat_init_kwh: assets.e_bat_init_kwh,
        e_bat_min_kwh: assets.e_bat_min_kwh,
        e_bat_max_kwh: assets.e_bat_max_kwh,
        p_bat_ch_max_kw: assets.p_bat_ch_max_kw,
        p_bat_dis_max_kw: assets.p_bat_dis_max_kw,
        eff_bat_ch: assets.eff_bat_ch,
        eff_bat_dis: assets.eff_bat_dis,
        a_ev: assets.a_ev,
        ev_mode: assets.ev_mode,
        ev_obligations: assets.ev_obligations,
        ev_battery_kwh: assets.ev_battery_kwh,
        p_ev_max_kw: assets.p_ev_max_kw,
        p_ev_min_kw: assets.p_ev_min_kw,
        ev_segments: assets.ev_segments,
        e_ev_extra_max_kwh: assets.e_ev_extra_max_kwh,
        v_ev_extra_eur_kwh: assets.v_ev_extra_eur_kwh,
        heater_mode: assets.heater_mode,
        t_heat_dead_step: assets.t_heat_dead_step,
        p_heat_step_kw: assets.p_heat_step_kw,
        heat_n_stages: assets.heat_n_stages,
        e_heat_init_kwh: assets.e_heat_init_kwh,
        e_heat_max_kwh: assets.e_heat_max_kwh,
        q_heat_dem_kw: assets.q_heat_dem_kw,
        e_heat_target_kwh: assets.e_heat_target_kwh,
        lambda_heat_sw_eur: assets.lambda_heat_sw_eur,
        w_tier_penalty_eur: planner.w_tier_penalty_eur,
        heat_initial_y: assets.heat_initial_y,
        shiftable_loads: assets.shiftable_loads,
        soc_ev_init: assets.soc_ev_init,
        ev_soc_drops: assets.ev_soc_drops,
    }
}
