//! The stages of `inputs::build_milp_inputs`, one per group of `MilpInputs` fields.
//!
//! Each stage computes its series from what it is handed and returns them; none reads another
//! stage's result except where the planner's rules demand it, and those edges are parameters:
//! `capacity_limits` takes the base load *before* the baseline override (SIMPLE level 2 caps at
//! the forecast, not at what the user added on top), and `ev_budget_warning` takes the import
//! rates. Order of the calls is `build_milp_inputs`'s business; this module must not assemble
//! `MilpInputs` itself, so the full field list stays in one place.

use chrono::{DateTime, Duration, Utc};

use super::asset_port::{AssetMilpParams, EvEnergySegment, EvObligation};
use super::inputs::{SiteInputs, StaleRateRefs};
use super::stale_rates::apply_stale_rate_policy;
use super::types::{MilpLoadMode, ShiftableLoadMilpContext};
use crate::controller::milp_planner::AssetMilpContext;
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

/// PV and base-load forecast per slot (the base load before any baseline override).
pub(super) fn forecast_series(bounds: &[SlotBounds], site: &SiteInputs) -> (Vec<f64>, Vec<f64>) {
    // Flat fallback (today's exact pre-WP5.2 behavior) used whenever no live
    // "base_load" asset exists this cycle.
    let flat_base_kw = site.base_load.map(|c| c.baseline_kw).unwrap_or(0.0);
    let mut p_pv = Vec::with_capacity(bounds.len());
    let mut p_base = Vec::with_capacity(bounds.len());
    for (i, &(slot_t, _)) in bounds.iter().enumerate() {
        // pv-competence-consolidation section 4: `pv_live_forecast_kw`
        // (resolved by the caller from a live `PvInverter` via
        // `uncurtailed_power_kw_at` — the same weather/decay-aware method
        // `max_effort_schedule` uses) is PV's own authority for
        // this slot. `pv_forecast_override` (the deterministic-testing pin)
        // always wins over it; `weather_pv_kw` (R-50) and the static `pv_cfg`
        // sin-model curve are the fallback chain for when no live "pv" asset
        // exists at all (site described, not simulated) — `pv_live_forecast_kw`
        // is `None` in exactly that case, since it's derived from the same
        // live snapshot.
        p_pv.push(
            site.pv_forecast_override
                .map(|kw| kw.max(0.0))
                .or_else(|| site.pv_live_forecast_kw.and_then(|v| v.get(i)).copied())
                .or_else(|| {
                    site.weather_pv_kw
                        .and_then(|v| v.get(i))
                        .map(|kw| kw.max(0.0))
                })
                .unwrap_or_else(|| site.pv_cfg.map(|c| c.forecast_kw(slot_t)).unwrap_or(0.0)),
        );
        // base-load-competence-consolidation: `base_load_live_forecast_kw`
        // (resolved by the caller from the live `BaseLoad`'s own
        // heuristic-aware `forecast_kw_at`) is the base load's own answer. `flat_base_kw` (the static
        // profile) is the fallback for when no live "base_load" asset exists.
        p_base.push(
            site.base_load_live_forecast_kw
                .and_then(|v| v.get(i))
                .copied()
                .unwrap_or(flat_base_kw),
        );
    }
    (p_pv, p_base)
}

pub(super) struct CapacityLimits {
    pub p_imp_max_phys_kw: Vec<f64>,
    pub p_exp_max_phys_kw: Vec<f64>,
    pub p_imp_max_cont_kw: Vec<f64>,
    pub p_exp_max_cont_kw: Vec<f64>,
}

/// Physical and contractual import/export limits per slot. `p_base_kw` is the base-load forecast
/// before the baseline override: SIMPLE level 2 caps import at it.
pub(super) fn capacity_limits(
    grid: &GridSignals,
    planner: &PlannerParams,
    site: &SiteInputs,
    bounds: &[SlotBounds],
    p_base_kw: &[f64],
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
    for (&(slot_t, slot_end), &base_kw_t) in bounds.iter().zip(p_base_kw) {
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

/// What the asset contexts declare that the planner reads outside the solve: one EV and one
/// heater at most (a later context of a kind replaces an earlier one), and every shiftable load
/// in context order. Deliberately not a copy of the contexts: a parameter only the solve needs
/// (a power limit, an efficiency, a tank size) stays in the asset's own context, which is what
/// the solve reads.
#[derive(Debug, Clone)]
pub(crate) struct AssetScalars {
    /// Per-step plugged-in availability mask. False forces p_ev[t] = 0.
    pub a_ev: Vec<bool>,
    pub ev_mode: MilpLoadMode,
    /// Every charging obligation this EV must meet inside the horizon (R-92).
    /// Empty = open horizon (plugged, no departure to charge for).
    pub ev_obligations: Vec<EvObligation>,
    /// Usable pack size [kWh]; 0.0 when EV absent. Turns an obligation's target
    /// SoC into the energy the diagnostics and session_flexibility report.
    pub ev_battery_kwh: f64,
    /// `ev-comfort-piecewise-core`: the comfort curve as priced energy bands.
    pub ev_segments: Vec<EvEnergySegment>,
    /// Opportunistic headroom = battery_kwh × (1 − soc_target) [kWh]
    pub e_ev_extra_max_kwh: f64,
    pub ev_budget_eur: Option<f64>,
    /// Live SoC of the EV at plan-build time [0.0..1.0]; None when no EV is present.
    pub soc_ev_init: Option<f64>,
    /// MILP-ready shiftable load descriptors (one per ShiftableLoad that fits the horizon)
    pub shiftable_loads: Vec<ShiftableLoadMilpContext>,
    pub heater_mode: MilpLoadMode,
    /// Power per heater stage [kW] = max_kw / n_stages.
    pub p_heat_step_kw: f64,
    /// Number of heater stages (1 or 2).
    pub heat_n_stages: u8,
    /// Target tank energy at deadline [kWh above T_min].
    pub e_heat_target_kwh: f64,
    /// Stage index the heater was at on the last real tick.
    pub heat_initial_y: f64,
}

impl AssetScalars {
    /// A site with no battery, EV, heater or shiftable load.
    pub(crate) fn none(n: usize) -> Self {
        Self {
            a_ev: vec![false; n],
            ev_mode: MilpLoadMode::MustNotRun,
            ev_obligations: Vec::new(),
            ev_battery_kwh: 0.0,
            ev_segments: Vec::new(),
            e_ev_extra_max_kwh: 0.0,
            ev_budget_eur: None,
            soc_ev_init: None,
            shiftable_loads: Vec::new(),
            heater_mode: MilpLoadMode::MustNotRun,
            p_heat_step_kw: 0.0,
            heat_n_stages: 0,
            e_heat_target_kwh: 0.0,
            heat_initial_y: 0.0,
        }
    }
}

impl AssetScalars {
    /// The energy one obligation demands above the EV's live SoC [kWh]: an obligation states
    /// a target SoC (R-92). Never negative.
    pub(crate) fn ev_obligation_kwh(&self, obligation: &EvObligation) -> f64 {
        ((obligation.target_soc_frac - self.soc_ev_init.unwrap_or(0.0)) * self.ev_battery_kwh)
            .max(0.0)
    }

    /// The firm energy the EV must receive [kWh]: the largest demand among its obligations
    /// when more than one departure is queued, 0 without any.
    pub(crate) fn ev_firm_kwh(&self) -> f64 {
        self.ev_obligations
            .iter()
            .map(|o| self.ev_obligation_kwh(o))
            .fold(0.0_f64, f64::max)
    }
}

pub(crate) fn asset_scalars(
    asset_contexts: &[Box<dyn AssetMilpContext>],
    n: usize,
    now: DateTime<Utc>,
) -> AssetScalars {
    let mut s = AssetScalars::none(n);
    for ctx in asset_contexts {
        match ctx.milp_params(n, now) {
            // A battery declares nothing the planner reads outside the solve.
            AssetMilpParams::Battery(_) => {}
            AssetMilpParams::Ev(e) => {
                s.a_ev = e.a_ev;
                s.ev_mode = e.mode;
                s.ev_obligations = e.obligations;
                s.ev_battery_kwh = e.battery_kwh;
                s.ev_segments = e.segments;
                s.e_ev_extra_max_kwh = e.e_extra_max_kwh;
                s.ev_budget_eur = e.budget_eur;
                s.soc_ev_init = Some(e.soc_init_frac);
            }
            AssetMilpParams::Heater(h) => {
                s.heater_mode = h.mode;
                s.p_heat_step_kw = h.p_step_kw;
                s.heat_n_stages = h.n_stages;
                s.e_heat_target_kwh = h.e_target_kwh;
                s.heat_initial_y = h.initial_y;
            }
            // Each shiftable load declares itself through its own context
            // (shiftable-load-as-asset), not through a bolt-on parameter.
            AssetMilpParams::ShiftableLoad(l) => {
                s.shiftable_loads.push(ShiftableLoadMilpContext {
                    asset_id: ctx.asset_id().to_string(),
                    power_kw: l.power_kw,
                    duration_slots: l.duration_slots,
                    valid_start_slots: l.valid_start_slots,
                });
            }
            AssetMilpParams::Unknown => {}
        }
    }
    s
}

/// Baseline override: additive per-slot kW adjustments, in the order the user gave them. An
/// entry before `now` is skipped; one past the horizon lands on the last slot.
pub(super) fn apply_baseline_override(
    p_base_kw: &mut [f64],
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
        if let Some(kw) = p_base_kw.get_mut(slot_at(&grid.cum_s, grid.n, offset_s)) {
            *kw += slot.add_kw;
        }
    }
}

/// WP4.1-c: even at the cheapest slot rate the target energy (all "extra"
/// headroom in MAX_COST mode) exceeds the budget → charging will stop
/// early. Stable text — WP4.3's notification dedup keys on it.
pub(super) fn ev_budget_warning(
    c_imp_eur_kwh: &[f64],
    ev_budget_eur: Option<f64>,
    e_ev_extra_max_kwh: f64,
) -> Option<String> {
    let budget_eur = ev_budget_eur?;
    let min_rate = c_imp_eur_kwh.iter().cloned().fold(f64::INFINITY, f64::min);
    (e_ev_extra_max_kwh * min_rate > budget_eur).then(|| {
        "EV charging budget too low to reach the session target — \
         charging stops at the budget (MAX_COST)"
            .to_string()
    })
}
