//! Builds `MilpInputs`, the planner's complete input parameter set, from the grid signals and
//! the forecasts the forecast participants (PV, base load) declare in their contexts.
//!
//! Owns the order of the stages (`input_stages`) and the one full spelling of `MilpInputs`'s
//! fields, so a new field is a compile error here and nowhere else. It decides nothing itself:
//! every rule (stale rates, the forecast totals, SIMPLE and alert caps, the baseline override)
//! lives in its stage. It must stay infallible and free of logging; warnings travel as
//! fields of the result.

use chrono::{DateTime, Utc};

use super::input_stages::{
    apply_baseline_override, capacity_limits, forecast_totals, slot_bounds, tariff_series,
};
use super::types::*;
use crate::controller::milp_planner::AssetMilpContext;
use crate::entities::device_session::BaselineOverride;
use crate::entities::grid_signals::GridSignals;
use crate::entities::planner_params::PlannerParams;
use crate::entities::time_grid::TimeGrid;
use lab_core::time_series::TimeSeries;

/// What the site contributes besides its assets: the grid connection's physical limits and the
/// user's baseline override.
pub(crate) struct SiteInputs<'a> {
    pub grid_max_import_kw: f64,
    pub grid_max_export_kw: f64,
    pub baseline_override: Option<&'a BaselineOverride>,
}

/// GB-42: history-store-backed diurnal reference series for HEURISTIC_FORECAST's
/// 168h-back (day-type-mismatch) lookback, resolved once per cycle by the caller
/// (services::planning::build_solve_request). `None` when history is disabled or
/// has no data yet for a series.
pub(crate) struct StaleRateRefs<'a> {
    pub import: Option<&'a TimeSeries>,
    pub co2: Option<&'a TimeSeries>,
}

/// Build the planner's site- and grid-level inputs from the current runtime state: prices and
/// limits from `grid`, the site's forecast generation and load summed from the forecast
/// participants among `asset_contexts`, the baseline override from `site`. Everything else about
/// an asset stays in its own context, which the solve reads directly.
pub(crate) fn build_milp_inputs(
    grid: &GridSignals,
    planner: &PlannerParams,
    site: &SiteInputs,
    refs: &StaleRateRefs,
    now: DateTime<Utc>,
    asset_contexts: &[Box<dyn AssetMilpContext>],
) -> MilpInputs {
    let time = TimeGrid::from_zones(&planner.plan_zones);
    let bounds = slot_bounds(&time, now);
    let tariffs = tariff_series(&grid.tariffs, planner, &bounds, refs);
    let (generation_forecast_kw, mut load_forecast_kw) = forecast_totals(asset_contexts, time.n);
    // Before the override: SIMPLE level 2 caps import at the forecast load.
    let limits = capacity_limits(grid, planner, site, &bounds, &load_forecast_kw);
    apply_baseline_override(&mut load_forecast_kw, &time, now, site.baseline_override);
    let mut load_adjust_kw = vec![0.0; time.n];
    apply_baseline_override(&mut load_adjust_kw, &time, now, site.baseline_override);

    MilpInputs {
        n: time.n,
        dt_h: time.dt_h,
        cum_s: time.cum_s,
        c_imp_eur_kwh: tariffs.c_imp_eur_kwh,
        rate_stale: tariffs.rate_stale,
        stale_rate_warning: tariffs.stale_rate_warning,
        co2_stale_rate_warning: tariffs.co2_stale_rate_warning,
        c_exp_eur_kwh: tariffs.c_exp_eur_kwh,
        g_imp_kgco2_kwh: tariffs.g_imp_kgco2_kwh,
        generation_forecast_kw,
        load_forecast_kw,
        load_adjust_kw,
        p_imp_max_phys_kw: limits.p_imp_max_phys_kw,
        p_exp_max_phys_kw: limits.p_exp_max_phys_kw,
        p_imp_max_cont_kw: limits.p_imp_max_cont_kw,
        p_exp_max_cont_kw: limits.p_exp_max_cont_kw,
        pen_imp_eur_kwh: planner.pen_imp_eur_kwh,
        pen_exp_eur_kwh: planner.pen_exp_eur_kwh,
        mip_gap_target: planner.mip_gap_target,
        penalty_rules: planner.penalty_rules.clone(),
    }
}
