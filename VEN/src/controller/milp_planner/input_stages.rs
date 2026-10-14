//! The stages of `inputs::build_milp_inputs`, one per group of `MilpInputs` fields.
//!
//! Each stage computes its series from what it is handed and returns them; none reads another
//! stage's result except where the planner's rules demand it, and those edges are parameters:
//! `capacity_limits` takes the forecast load *before* the baseline override (SIMPLE level 2 caps at
//! the forecast, not at what the user added on top), and `ev_budget_warning` takes the import
//! rates. Order of the calls is `build_milp_inputs`'s business; this module must not assemble
//! `MilpInputs` itself, so the full field list stays in one place.

use chrono::{DateTime, Duration, Utc};

use super::inputs::{SiteInputs, StaleRateRefs};
use super::stale_rates::apply_stale_rate_policy;
use crate::controller::milp_planner::{AssetMilpContext, ForecastRole, Participation};
use crate::entities::capacity::tightest_capacity_limit;
use crate::entities::capacity_curve::CommitmentDirection;
use crate::entities::device_session::BaselineOverride;
use crate::entities::grid_signals::GridSignals;
use crate::entities::planner_params::PlannerParams;
use crate::entities::tariff_snapshot::TariffTimeSeries;
use crate::entities::time_grid::{slot_at, TimeGrid};
use lab_core::time_window::TimeWindow;

/// Start and end instant of one slot.
pub(super) type SlotBounds = (DateTime<Utc>, DateTime<Utc>);

pub(super) fn slot_bounds(grid: &TimeGrid, now: DateTime<Utc>) -> Vec<SlotBounds> {
    (0..grid.n)
        .map(|i| {
            (
                now + Duration::seconds(grid.cum_s[i]),
                now + Duration::seconds(grid.cum_s[i + 1]),
            )
        })
        .collect()
}

pub(super) struct TariffSeries {
    pub c_imp_eur_kwh: Vec<f64>,
    pub c_exp_eur_kwh: Vec<f64>,
    pub g_imp_kgco2_kwh: Vec<f64>,
    pub rate_stale: Vec<bool>,
    pub stale_rate_warning: Option<String>,
    pub co2_stale_rate_warning: Option<String>,
}

/// Import rate, export rate and CO2 intensity per slot.
pub(super) fn tariff_series(
    tariffs: &TariffTimeSeries,
    planner: &PlannerParams,
    bounds: &[SlotBounds],
    refs: &StaleRateRefs,
) -> TariffSeries {
    // WP4.4 (BL-07): import rates come through the stale-rate policy — covered
    // slots use the time-weighted mean over the slot (R-16), slots beyond
    // tariff coverage are filled per policy.
    let import = apply_stale_rate_policy(
        &planner.stale_rate_policy,
        planner.stale_rate_safe_pctl,
        &tariffs.import_eur_kwh,
        tariffs.import_coverage_end,
        bounds,
        0.25,
        "Tariff data",
        refs.import,
    );
    // BL-17 closeout: CO2 intensity gets the same staleness-policy parity as
    // the import tariff, instead of silently holding the last known value
    // forward forever. Default 300.0 g/kWh matches the pre-existing fallback.
    let co2 = apply_stale_rate_policy(
        &planner.stale_rate_policy,
        planner.stale_rate_safe_pctl,
        &tariffs.co2_g_kwh,
        tariffs.co2_coverage_end,
        bounds,
        300.0,
        "GHG data",
        refs.co2,
    );
    // R-16: time-weighted means so boundary-straddling slots blend rates;
    // falls back to the slot-start sample when the mean is undefined.
    let c_exp_eur_kwh = bounds
        .iter()
        .map(|&(slot_t, slot_end)| {
            tariffs
                .export_eur_kwh
                .time_weighted_mean(slot_t, slot_end)
                .or_else(|| tariffs.export_eur_kwh.interpolate_at(slot_t))
                .unwrap_or(0.08)
        })
        .collect();
    TariffSeries {
        c_imp_eur_kwh: import.values,
        c_exp_eur_kwh,
        // CO₂ stored as g/kWh → MILP uses kgCO₂/kWh.
        g_imp_kgco2_kwh: co2.values.iter().map(|v| v / 1000.0).collect(),
        rate_stale: import.rate_stale,
        stale_rate_warning: import.warning,
        co2_stale_rate_warning: co2.warning,
    }
}

/// The forecast participants' generation and load per slot [kW], each summed over the
/// participants of that role (PV, base load); a role no participant plays is 0 kW. A single
/// participant's forecast passes through unchanged.
pub(super) fn forecast_totals(
    asset_contexts: &[Box<dyn AssetMilpContext>],
    n: usize,
) -> (Vec<f64>, Vec<f64>) {
    let total = |role: ForecastRole| -> Vec<f64> {
        (0..n)
            .map(|t| {
                asset_contexts
                    .iter()
                    .filter_map(|ctx| match ctx.participation() {
                        Participation::Forecast {
                            role: r,
                            forecast_kw,
                        } if r == role => Some(forecast_kw[t]),
                        _ => None,
                    })
                    .reduce(|a, b| a + b)
                    .unwrap_or(0.0)
            })
            .collect()
    };
    (total(ForecastRole::Generation), total(ForecastRole::Load))
}

pub(super) struct CapacityLimits {
    pub p_imp_max_phys_kw: Vec<f64>,
    pub p_exp_max_phys_kw: Vec<f64>,
    pub p_imp_max_cont_kw: Vec<f64>,
    pub p_exp_max_cont_kw: Vec<f64>,
}

/// Physical and contractual import/export limits per slot. `load_kw` is the forecast load before
/// the baseline override: SIMPLE level 2 caps import at it.
pub(super) fn capacity_limits(
    grid: &GridSignals,
    planner: &PlannerParams,
    site: &SiteInputs,
    bounds: &[SlotBounds],
    load_kw: &[f64],
) -> CapacityLimits {
    let (phys_imp, phys_exp) = (site.grid_max_import_kw, site.grid_max_export_kw);
    // WP3.3 (§8.10): subscription + reservation form one contracted allowance
    // — the rule itself lives on `OadrCapacityState`, since the reservation
    // request the VEN reports back asks the same question (R-76).
    let imp_allowance = grid.capacity.import_allowance_kw();
    let exp_allowance = grid.capacity.export_allowance_kw();
    // GB-48: a slot's contractual cap is the tightest scheduled limit
    // overlapping it (never planning through the capped part of a coarse
    // slot, like alerts/SIMPLE), else the physical bound, and never above the
    // allowance. The folded `capacity.import/export_limit_kw` ("in force now")
    // is not a planner input.
    let slot_cap = |direction: CommitmentDirection, from, to, phys: f64, allowance: f64| {
        tightest_capacity_limit(&grid.capacity_schedule, direction, from, to)
            .map_or(phys, |l| l.limit_kw)
            .min(allowance)
    };
    let mut p_imp_max_cont_kw = Vec::with_capacity(bounds.len());
    let mut p_exp_max_cont_kw = Vec::with_capacity(bounds.len());
    for (&(slot_t, slot_end), &base_kw_t) in bounds.iter().zip(load_kw) {
        let cont_imp = slot_cap(
            CommitmentDirection::Import,
            slot_t,
            slot_end,
            phys_imp,
            imp_allowance,
        );
        // WP3.2: SIMPLE levels clamp the import cap per slot — level 1 to a
        // configurable fraction of the contractual limit, level 2 to the
        // baseline forecast (defer all flexible draw), level 3 to 0. Highest
        // overlapping level wins; combined with the contractual cap via min.
        let simple_level = grid
            .simple_windows
            .iter()
            .filter(|w| w.overlaps(slot_t, slot_end))
            .map(|w| w.level)
            .max();
        let simple_cap = match simple_level {
            Some(1) => cont_imp * planner.simple_level1_import_cap_pct,
            Some(2) => base_kw_t.max(0.0),
            Some(l) if l >= 3 => 0.0,
            _ => cont_imp,
        };
        // WP3.1 (BL-04): slots overlapping an active grid-alert window get an
        // import cap of 0 ("minimize electricity use", both alert types). The
        // cap is soft in the solver (slack + violation penalty), so unavoidable
        // base load yields a warned violation, never infeasibility. Export is
        // left untouched — the spec prescribes nothing for it. Alerts override
        // any SIMPLE level.
        let in_alert = grid
            .alert_windows
            .iter()
            .any(|a| a.overlaps(slot_t, slot_end));
        p_imp_max_cont_kw.push(if in_alert {
            0.0
        } else {
            cont_imp.min(simple_cap)
        });
        p_exp_max_cont_kw.push(slot_cap(
            CommitmentDirection::Export,
            slot_t,
            slot_end,
            phys_exp,
            exp_allowance,
        ));
    }
    CapacityLimits {
        p_imp_max_phys_kw: vec![phys_imp; bounds.len()],
        p_exp_max_phys_kw: vec![phys_exp; bounds.len()],
        p_imp_max_cont_kw,
        p_exp_max_cont_kw,
    }
}

/// Baseline override: additive per-slot kW adjustments, in the order the user gave them. An
/// entry before `now` is skipped; one past the horizon lands on the last slot.
pub(super) fn apply_baseline_override(
    load_kw: &mut [f64],
    grid: &TimeGrid,
    now: DateTime<Utc>,
    baseline_override: Option<&BaselineOverride>,
) {
    let Some(bo) = baseline_override else { return };
    for slot in &bo.slots {
        let offset_s = (slot.slot_start - now).num_seconds();
        if offset_s < 0 {
            continue;
        }
        if let Some(kw) = load_kw.get_mut(slot_at(&grid.cum_s, grid.n, offset_s)) {
            *kw += slot.add_kw;
        }
    }
}
