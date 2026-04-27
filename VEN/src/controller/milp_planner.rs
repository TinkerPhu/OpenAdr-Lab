//! MILP-based HEMS planner — entry point.
//!
//! Builds `MilpInputs` from live state, solves via HiGHS, and translates
//! the solution into a `Plan` with per-slot allocations.
//! See `docs/plans/milp_planner_transition.md` for the design.

use chrono::{DateTime, Duration, Utc};
use good_lp::solvers::highs::highs;
use good_lp::{
    constraint, variable, variables, Expression, Solution, SolverModel, Variable, WithInitialSolution,
    WithMipGap, WithTimeLimit,
};
use tracing::warn;
use uuid::Uuid;

use crate::assets::{AssetConfig, PvInverter};
use crate::assets::battery::BatteryMilpContext;
use crate::assets::ev::{EvMilpContext, EvMilpMode};
use crate::assets::heater::{HeaterMilpContext, HeaterMilpMode};
use crate::controller::milp_interactions::{
    build_interactions, GlobalMilpInputs, GridMilpVars, MilpVarPool, ShiftableLoadMilpVars,
};
use crate::entities::asset::PlanTrigger;
use crate::entities::capacity::OadrCapacityState;
use crate::entities::device_session::{BaselineOverride, ShiftableLoad};
use crate::entities::plan::{
    AssetAllocation, CostBreakdown, FlexibilityEnvelope, Plan, PlanSummary,
    PlanTimeSlot, PlanningHorizon, PlanWarning, WarningSeverity,
};
use crate::entities::tariff_snapshot::TariffTimeSeries;
use crate::profile::{PlannerObjective, Profile};
use crate::simulator::SimState;

// ── Internal MILP types ──────────────────────────────────────────────────────

/// Internal MILP load mode for an asset (EV / heater).
/// Derived from the presence of an active device session (EvSession / HeaterTarget).
#[derive(Debug, Clone, PartialEq)]
enum MilpLoadMode {
    /// Hard energy requirement — must be met within deadline. Used when
    /// EvSession.soft_deadline=false, or a HeaterTarget with remaining work is present.
    MustRun,
    /// Soft energy target — controlled by a reward term in the objective.
    /// Used when EvSession.soft_deadline=true.
    MayRun,
    /// Asset absent, currently unavailable, or no device session present.
    MustNotRun,
}

/// Phase 1 objective coefficients (economic cost). Derived from PlannerConfig / PlannerObjective.
#[derive(Debug, Clone)]
struct Phase1Weights {
    /// Scales C_energy = Σ(c_imp·p_imp − c_exp·p_exp)·Δt
    w_energy: f64,
    /// Monetises GHG emissions [€/kgCO₂]
    w_ghg: f64,
    /// Penalty per kWh of total grid exchange (import + export)
    w_grid: f64,
    /// Penalty per kWh of grid import only (autarky objective)
    w_import: f64,
    /// Scales contractual violation penalties
    w_viol: f64,
    /// Battery cycling wear cost [€/kWh charged or discharged]
    c_bat_wear_eur_kwh: f64,
    /// Penalty per kWh of battery discharge co-occurring with EV charging when
    /// PV surplus ≥ ev_min_kw [€/kWh]. 0.0 = disabled.
    c_bat_ev_coexist_eur_kwh: f64,
    /// Scales service reward terms; always 1.0 until grid-services are modelled
    w_services: f64,
}

/// Phase 2 objective coefficients (operational friction). Used only when
/// `phase2_epsilon_eur > 0.0`. Phase 2 minimises startup/ramp/switching/tier
/// cost subject to a Phase 1 cost cap.
#[derive(Debug, Clone)]
struct Phase2Weights {
    /// Penalty per battery charge/discharge mode transition [€/transition]
    c_bat_startup_eur: f64,
    /// Penalty per kW of battery net-power change between consecutive slots [€/kW]
    c_bat_ramp_eur_kw: f64,
    /// Penalty per EV charging run startup [€/run]
    c_ev_startup_eur: f64,
    /// Penalty per kW of EV power change between consecutive slots [€/kW]
    c_ev_ramp_eur_kw: f64,
    /// Heater relay switching penalty [EUR/switch event]
    lambda_heat_sw_eur: f64,
    /// Soft penalty per slot for using the full power tier over mid tier [€/slot]
    w_tier_penalty_eur: f64,
}

/// Fully-resolved MILP input parameters for one planning cycle.
/// Created by `build_milp_inputs()`; consumed by `solve_milp_two_phase()` (Phase 3).
/// All per-step `Vec<f64>` fields have length `n`.
#[derive(Debug, Clone)]
struct MilpInputs {
    /// Number of planning steps.
    n: usize,
    /// Step size in hours (e.g. 300 s → 1/12 h).
    dt_h: f64,

    // ── Grid (per-step arrays, len = n) ──────────────────────────────────────
    /// Import tariff [€/kWh]
    c_imp_eur_kwh: Vec<f64>,
    /// Export tariff [€/kWh]
    c_exp_eur_kwh: Vec<f64>,
    /// Grid CO₂ intensity [kgCO₂/kWh] (÷1000 from stored g/kWh)
    g_imp_kgco2_kwh: Vec<f64>,
    /// PV generation forecast [kW]
    p_pv_kw: Vec<f64>,
    /// Non-controllable baseline load [kW]
    p_base_kw: Vec<f64>,
    /// Physical import limit at meter/breaker [kW]
    p_imp_max_phys_kw: Vec<f64>,
    /// Physical export limit [kW]
    p_exp_max_phys_kw: Vec<f64>,
    /// Contractual import limit [kW] (OpenADR event limit or physical when no event)
    p_imp_max_cont_kw: Vec<f64>,
    /// Contractual export limit [kW]
    p_exp_max_cont_kw: Vec<f64>,
    /// Per-kWh import violation penalty (scalar, from PlannerConfig)
    pen_imp_eur_kwh: f64,
    /// Per-kWh export violation penalty (scalar)
    pen_exp_eur_kwh: f64,

    // ── Battery (None when no battery asset present in profile) ──────────────
    /// Nameplate capacity [kWh]
    e_bat_nom_kwh: Option<f64>,
    /// Initial SoC energy at call time [kWh]. Uses LIVE SimState SoC, NOT profile initial_soc.
    e_bat_init_kwh: Option<f64>,
    /// Operational lower bound = min_soc × capacity [kWh]
    e_bat_min_kwh: Option<f64>,
    /// Operational upper bound = capacity (no upper SoC cap today) [kWh]
    e_bat_max_kwh: Option<f64>,
    /// Max charge power [kW]
    p_bat_ch_max_kw: Option<f64>,
    /// Max discharge power [kW]
    p_bat_dis_max_kw: Option<f64>,
    /// One-way charge efficiency = √(round_trip_efficiency)
    eff_bat_ch: Option<f64>,
    /// One-way discharge efficiency = √(round_trip_efficiency)
    eff_bat_dis: Option<f64>,

    // ── EV (MustNotRun when EV absent or unplugged) ───────────────────────────
    /// Per-step plugged-in availability mask. False forces p_ev[t] = 0.
    a_ev: Vec<bool>,
    ev_mode: MilpLoadMode,
    /// Last step index that counts toward the EV energy sum.
    /// None = open horizon (plugged, no active packet with a deadline).
    t_ev_dead_step: Option<usize>,
    /// Max charge power [kW]; 0.0 when EV absent
    p_ev_max_kw: f64,
    /// Semi-continuous minimum charge power [kW] (EvConfig.min_charge_kw)
    p_ev_min_kw: f64,
    /// Core energy requirement [kWh] from active packet; 0.0 when absent
    e_ev_core_kwh: f64,
    /// Opportunistic headroom = battery_kwh × (1 − soc_target) [kWh]
    e_ev_extra_max_kwh: f64,
    /// Reward for meeting core target (MayRun only); hardcoded 0.0 until user-requests integration
    v_ev_core_eur: f64,
    /// Reward per kWh of extra opportunistic charging [€/kWh]
    v_ev_extra_eur_kwh: f64,

    // ── Heater (MustNotRun when heater absent) ────────────────────────────────
    heater_mode: MilpLoadMode,
    /// Deadline step index. None = no hard deadline (autonomous MayRun path).
    t_heat_dead_step: Option<usize>,
    /// Mid power level [kW] = mid_kw.unwrap_or(max_kw / 2.0)
    p_heat_mid_kw: f64,
    /// Full power level [kW] = max_kw
    p_heat_full_kw: f64,
    /// Initial tank energy above T_min [kWh]. May be negative when tank is below T_min.
    e_heat_init_kwh: f64,
    /// Maximum usable tank energy above T_min [kWh] = (T_max − T_min) × thermal_mass.
    e_heat_max_kwh: f64,
    /// Constant per-step thermal demand [kW]: draw_kw + k_loss × (T_mid − ambient).
    q_heat_dem_kw: f64,
    /// Target tank energy at deadline [kWh above T_min]. = e_heat_max_kwh in autonomous mode.
    e_heat_target_kwh: f64,
    /// Relay switching penalty [EUR/switch event].
    lambda_heat_sw_eur: f64,
    /// Soft penalty per slot for using the full power tier over mid tier [€/slot].
    w_tier_penalty_eur: f64,
    /// Initial heater mid-power binary (1.0 if heater was at mid power last tick).
    heat_initial_z_mid: f64,
    /// Initial heater full-power binary (1.0 if heater was at full power last tick).
    heat_initial_z_full: f64,

    // ── Shiftable loads (Phase B) ────────────────────────────────────────────
    /// MILP-ready shiftable load descriptors (one per ShiftableLoad that fits the horizon)
    shiftable_loads: Vec<ShiftableLoadMilp>,
}

/// Internal MILP descriptor for one shiftable load block.
#[derive(Debug, Clone)]
struct ShiftableLoadMilp {
    /// Label for allocations (e.g. "wm")
    asset_id: String,
    /// Fixed power level while running [kW]
    power_kw: f64,
    /// Duration in planning slots (ceil)
    duration_slots: usize,
    /// Valid start-slot indices within [0, n)
    valid_start_slots: Vec<usize>,
}

// ── Builder functions ────────────────────────────────────────────────────────

/// Build the MILP objective weights from the profile's planner configuration.
/// `objective` overrides `profile.planner.objective`; pass `profile.planner.objective`
/// to use the profile default.
fn build_phase1_weights(profile: &Profile, objective: PlannerObjective) -> Phase1Weights {
    let p = &profile.planner;
    match objective {
        PlannerObjective::MinCost => Phase1Weights {
            w_energy: 1.0,
            w_ghg: 0.20,
            w_grid: 0.02,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: 0.03,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            w_services: 1.0,
        },
        PlannerObjective::MinGhg => Phase1Weights {
            w_energy: 0.0,
            w_ghg: 10.0,
            w_grid: 0.0,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: 0.0,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            w_services: 1.0,
        },
        PlannerObjective::MinGrid => Phase1Weights {
            w_energy: 0.0,
            w_ghg: 0.0,
            w_grid: 1.0,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: 0.0,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            w_services: 1.0,
        },
        PlannerObjective::MinImport => Phase1Weights {
            w_energy: 0.0,
            w_ghg: 0.0,
            w_grid: 0.0,
            w_import: 1.0,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: 0.0,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            w_services: 1.0,
        },
        PlannerObjective::MaxRevenue => Phase1Weights {
            w_energy: 1.0,
            w_ghg: 0.0,
            w_grid: 0.0,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: 0.03,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            w_services: 1.0,
        },
        PlannerObjective::Custom => Phase1Weights {
            w_energy: p.w_energy,
            w_ghg: p.w_ghg,
            w_grid: p.w_grid,
            w_import: 0.0,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: p.c_bat_wear_eur_kwh,
            c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,
            w_services: 1.0,
        },
    }
}

fn build_phase2_weights(inputs: &MilpInputs, profile: &Profile) -> Phase2Weights {
    let p = &profile.planner;
    Phase2Weights {
        c_bat_startup_eur: p.c_bat_startup_eur,
        c_bat_ramp_eur_kw: p.c_bat_ramp_eur_kw,
        c_ev_startup_eur: p.c_ev_startup_eur,
        c_ev_ramp_eur_kw: p.c_ev_ramp_eur_kw,
        lambda_heat_sw_eur: inputs.lambda_heat_sw_eur,
        w_tier_penalty_eur: inputs.w_tier_penalty_eur,
    }
}

/// Convert a packet deadline to a planning step index, clamped to [0, n−1].
fn deadline_to_step(deadline: DateTime<Utc>, now: DateTime<Utc>, step_s: u64, n: usize) -> usize {
    let secs = (deadline - now).num_seconds();
    (secs / step_s as i64).clamp(0, (n.saturating_sub(1)) as i64) as usize
}

/// Build the full MILP input parameter set from the current runtime state.
///
/// All transformations — CO₂ unit conversion (g→kg), √RTE efficiency split,
/// live battery SoC, EV horizon mask, and LoadMode translation — happen here.
/// The resulting `MilpInputs` is ready to pass directly to `solve_milp_two_phase()`.
fn build_milp_inputs(
    assets: &SimState,
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    baseline_override: Option<&BaselineOverride>,
) -> MilpInputs {
    let step_s = profile.planner.plan_step_s;
    let n = ((profile.planner.plan_horizon_h as f64 * 3600.0) / step_s as f64) as usize;
    let dt_h = step_s as f64 / 3600.0;

    // ── Per-step grid arrays ──────────────────────────────────────────────────
    let phys_imp = profile.grid.max_import_kw;
    let phys_exp = profile.grid.max_export_kw;
    let cont_imp = capacity.import_limit_kw.unwrap_or(phys_imp);
    let cont_exp = capacity.export_limit_kw.unwrap_or(phys_exp);
    let pv_cfg = profile.pv_config();
    let base_kw = profile.base_load_kw();

    let mut c_imp = Vec::with_capacity(n);
    let mut c_exp = Vec::with_capacity(n);
    let mut g_co2 = Vec::with_capacity(n);
    let mut p_pv = Vec::with_capacity(n);
    let mut p_base = Vec::with_capacity(n);
    let mut p_imp_phys = Vec::with_capacity(n);
    let mut p_exp_phys = Vec::with_capacity(n);
    let mut p_imp_cont = Vec::with_capacity(n);
    let mut p_exp_cont = Vec::with_capacity(n);

    for t in 0..n {
        let slot_t = now + Duration::seconds(t as i64 * step_s as i64);
        c_imp.push(
            tariffs
                .import_eur_kwh
                .interpolate_at(slot_t)
                .unwrap_or(0.25),
        );
        c_exp.push(
            tariffs
                .export_eur_kwh
                .interpolate_at(slot_t)
                .unwrap_or(0.08),
        );
        // CO₂ stored as g/kWh → MILP uses kgCO₂/kWh
        g_co2.push(
            tariffs
                .co2_g_kwh
                .interpolate_at(slot_t)
                .unwrap_or(300.0)
                / 1000.0,
        );
        // Use live PvInverter when available so that irradiance_offset (irradiance
        // slider) and pv_alpha (blend-back speed slider) both project into the
        // forecast. Falls back to the static sin model if no "pv" asset exists.
        let pv_kw = if let Some((_, cfg)) = assets.find_asset("pv") {
            if let AssetConfig::Pv(pv) = cfg {
                let natural = PvInverter::natural_irradiance_at(slot_t);
                // pv_alpha is "fraction removed per plan step (300 s)".
                // Exponent is the number of plan steps ahead, not raw seconds.
                let decayed_offset =
                    pv.irradiance_offset * (1.0 - pv.pv_alpha).powf(t as f64);
                (natural + decayed_offset).clamp(0.0, 1.0) * pv.rated_kw
            } else {
                pv_cfg.map(|c| c.forecast_kw(slot_t)).unwrap_or(0.0)
            }
        } else {
            pv_cfg.map(|c| c.forecast_kw(slot_t)).unwrap_or(0.0)
        };
        p_pv.push(pv_kw);
        p_base.push(base_kw);
        p_imp_phys.push(phys_imp);
        p_exp_phys.push(phys_exp);
        p_imp_cont.push(cont_imp);
        p_exp_cont.push(cont_exp);
    }

    // ── Battery ───────────────────────────────────────────────────────────────
    let (
        e_bat_nom,
        e_bat_init,
        e_bat_min,
        e_bat_max,
        p_bat_ch_max,
        p_bat_dis_max,
        eff_ch,
        eff_dis,
    ) = if let Some(bat_cfg) = profile.battery_config() {
        let ctx = match assets.find_asset("battery") {
            Some((entry, AssetConfig::Battery(b))) => BatteryMilpContext::from_state(&entry.state, b),
            _ => {
                // No live battery in sim (e.g. cleared in test): fall back to profile initial_soc
                let cap = bat_cfg.capacity_kwh;
                let eff = bat_cfg.round_trip_efficiency.sqrt();
                BatteryMilpContext {
                    e_nom_kwh: cap,
                    e_init_kwh: bat_cfg.initial_soc * cap,
                    e_min_kwh: bat_cfg.min_soc * cap,
                    e_max_kwh: cap,
                    p_ch_max_kw: bat_cfg.max_charge_kw,
                    p_dis_max_kw: bat_cfg.max_discharge_kw,
                    eff_ch: eff,
                    eff_dis: eff,
                }
            }
        };
        (
            Some(ctx.e_nom_kwh),
            Some(ctx.e_init_kwh),
            Some(ctx.e_min_kwh),
            Some(ctx.e_max_kwh),
            Some(ctx.p_ch_max_kw),
            Some(ctx.p_dis_max_kw),
            Some(ctx.eff_ch),
            Some(ctx.eff_dis),
        )
    } else {
        (None, None, None, None, None, None, None, None)
    };

    // ── EV ────────────────────────────────────────────────────────────────────
    let (a_ev, ev_mode, t_ev_dead, p_ev_max, p_ev_min, e_ev_core, e_ev_extra, v_ev_extra) =
        if let Some(ev_cfg) = profile.ev_config() {
            let ctx = match assets.find_asset("ev") {
                Some((entry, AssetConfig::Ev(e))) => EvMilpContext::from_state(
                    &entry.state, e, n, step_s, now, ev_session,
                    ev_cfg.min_charge_kw, profile.planner.v_ev_extra_eur_kwh,
                ),
                _ => EvMilpContext {
                    mode: EvMilpMode::MustNotRun,
                    a_ev: vec![false; n],
                    t_dead_step: None,
                    p_max_kw: ev_cfg.max_charge_kw,
                    p_min_kw: ev_cfg.min_charge_kw,
                    e_core_kwh: 0.0,
                    e_extra_max_kwh: ev_cfg.battery_kwh * (1.0 - ev_cfg.soc_target),
                    v_extra_eur_kwh: profile.planner.v_ev_extra_eur_kwh,
                },
            };
            let ev_mode = match ctx.mode {
                EvMilpMode::MustRun => MilpLoadMode::MustRun,
                EvMilpMode::MayRun => MilpLoadMode::MayRun,
                EvMilpMode::MustNotRun => MilpLoadMode::MustNotRun,
            };
            (ctx.a_ev, ev_mode, ctx.t_dead_step, ctx.p_max_kw, ctx.p_min_kw,
             ctx.e_core_kwh, ctx.e_extra_max_kwh, ctx.v_extra_eur_kwh)
        } else {
            // No EV asset in profile
            (vec![false; n], MilpLoadMode::MustNotRun, None, 0.0, 0.0, 0.0, 0.0, 0.0)
        };

    // ── Heater ────────────────────────────────────────────────────────────────
    let (heater_mode, t_heat_dead, p_mid, p_full, e_heat_init, e_heat_max, q_heat_dem, e_heat_target, lambda_sw, heat_iz_mid, heat_iz_full) =
        if let Some(heat_cfg) = profile.heater_config() {
            let lambda = heat_cfg.effective_switching_penalty();
            let ctx = match assets.find_asset("heater") {
                Some((entry, AssetConfig::Heater(h))) => HeaterMilpContext::from_state(
                    &entry.state, h, n, step_s, now, heater_target, lambda,
                ),
                _ => {
                    // No live heater in sim: reconstruct from profile config
                    let thermal_mass = heat_cfg.effective_thermal_mass();
                    let ambient = 10.0;
                    let live_t_min = heat_cfg.temp_min_c;
                    let live_t_max = heat_cfg.temp_max_c;
                    let live_max_kw = heat_cfg.max_kw;
                    let live_mid_kw = heat_cfg.mid_kw.unwrap_or(live_max_kw / 2.0);
                    let current_temp = heat_cfg.temp_initial_c;
                    let e_init = (current_temp - live_t_min) * thermal_mass;
                    let e_max = ((live_t_max - live_t_min) * thermal_mass).max(0.0);
                    let t_mid = (live_t_min + live_t_max) / 2.0;
                    let q_dem = (heat_cfg.effective_draw_kw()
                        + heat_cfg.effective_k_loss() * (t_mid - ambient))
                        .max(0.0);
                    if let Some(target) = heater_target {
                        let e_target = ((target.target_temp_c - live_t_min) * thermal_mass)
                            .clamp(0.0, e_max);
                        let secs = (target.ready_by - now).num_seconds();
                        let t_dead = (secs / step_s as i64)
                            .clamp(0, (n.saturating_sub(1)) as i64) as usize;
                        HeaterMilpContext {
                            mode: HeaterMilpMode::MustRun,
                            t_dead_step: Some(t_dead),
                            p_mid_kw: live_mid_kw,
                            p_full_kw: live_max_kw,
                            e_init_kwh: e_init,
                            e_max_kwh: e_max,
                            q_dem_kw: q_dem,
                            e_target_kwh: e_target,
                            lambda_sw_eur: lambda,
                            initial_z_mid: 0.0,
                            initial_z_full: 0.0,
                        }
                    } else {
                        HeaterMilpContext {
                            mode: HeaterMilpMode::MayRun,
                            t_dead_step: None,
                            p_mid_kw: live_mid_kw,
                            p_full_kw: live_max_kw,
                            e_init_kwh: e_init,
                            e_max_kwh: e_max,
                            q_dem_kw: q_dem,
                            e_target_kwh: e_max,
                            lambda_sw_eur: lambda,
                            initial_z_mid: 0.0,
                            initial_z_full: 0.0,
                        }
                    }
                }
            };
            let heater_mode = match ctx.mode {
                HeaterMilpMode::MustRun => MilpLoadMode::MustRun,
                HeaterMilpMode::MayRun => MilpLoadMode::MayRun,
                HeaterMilpMode::MustNotRun => MilpLoadMode::MustNotRun,
            };
            (heater_mode, ctx.t_dead_step, ctx.p_mid_kw, ctx.p_full_kw,
             ctx.e_init_kwh, ctx.e_max_kwh, ctx.q_dem_kw, ctx.e_target_kwh, ctx.lambda_sw_eur,
             ctx.initial_z_mid, ctx.initial_z_full)
        } else {
            (MilpLoadMode::MustNotRun, None, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
        };

    // ── Baseline override: additive per-slot kW adjustments ─────────────────
    if let Some(bo) = baseline_override {
        for slot in &bo.slots {
            let offset_s = (slot.slot_start - now).num_seconds();
            if offset_s < 0 { continue; }
            let idx = (offset_s as f64 / (dt_h * 3600.0)).floor() as usize;
            if idx < n {
                p_base[idx] += slot.add_kw;
            }
        }
    }

    // ── Shiftable loads → ShiftableLoadMilp ──────────────────────────────────
    let milp_loads: Vec<ShiftableLoadMilp> = shiftable_loads.iter().filter_map(|sl| {
        let dur_s = (sl.duration_min as f64) * 60.0;
        let duration_slots = (dur_s / (dt_h * 3600.0)).ceil() as usize;
        if duration_slots == 0 { return None; }

        let window_start_s = (sl.earliest_start - now).num_seconds().max(0) as f64;
        let window_end_s = (sl.latest_end - now).num_seconds().max(0) as f64;

        let first_slot = (window_start_s / (dt_h * 3600.0)).floor() as usize;
        // Last valid start: load must finish before latest_end
        let last_valid_s = window_end_s - dur_s;
        if last_valid_s < 0.0 {
            tracing::warn!(
                asset_id = %sl.asset_id,
                window_end_s,
                dur_s,
                "shiftable load window expired before planner ran — skipped"
            );
            return None;
        }
        let last_slot = ((last_valid_s / (dt_h * 3600.0)).floor() as usize).min(n.saturating_sub(duration_slots));

        let valid_start_slots: Vec<usize> = (first_slot..=last_slot).filter(|&s| s + duration_slots <= n).collect();
        if valid_start_slots.is_empty() {
            tracing::warn!(asset_id = %sl.asset_id, "shiftable load has no valid start slots in horizon — skipped");
            return None;
        }

        Some(ShiftableLoadMilp {
            asset_id: sl.asset_id.clone(),
            power_kw: sl.power_kw,
            duration_slots,
            valid_start_slots,
        })
    }).collect();

    MilpInputs {
        n,
        dt_h,
        c_imp_eur_kwh: c_imp,
        c_exp_eur_kwh: c_exp,
        g_imp_kgco2_kwh: g_co2,
        p_pv_kw: p_pv,
        p_base_kw: p_base,
        p_imp_max_phys_kw: p_imp_phys,
        p_exp_max_phys_kw: p_exp_phys,
        p_imp_max_cont_kw: p_imp_cont,
        p_exp_max_cont_kw: p_exp_cont,
        pen_imp_eur_kwh: profile.planner.pen_imp_eur_kwh,
        pen_exp_eur_kwh: profile.planner.pen_exp_eur_kwh,
        e_bat_nom_kwh: e_bat_nom,
        e_bat_init_kwh: e_bat_init,
        e_bat_min_kwh: e_bat_min,
        e_bat_max_kwh: e_bat_max,
        p_bat_ch_max_kw: p_bat_ch_max,
        p_bat_dis_max_kw: p_bat_dis_max,
        eff_bat_ch: eff_ch,
        eff_bat_dis: eff_dis,
        a_ev,
        ev_mode,
        t_ev_dead_step: t_ev_dead,
        p_ev_max_kw: p_ev_max,
        p_ev_min_kw: p_ev_min,
        e_ev_core_kwh: e_ev_core,
        e_ev_extra_max_kwh: e_ev_extra,
        v_ev_core_eur: 0.0,
        v_ev_extra_eur_kwh: v_ev_extra,
        heater_mode,
        t_heat_dead_step: t_heat_dead,
        p_heat_mid_kw: p_mid,
        p_heat_full_kw: p_full,
        e_heat_init_kwh: e_heat_init,
        e_heat_max_kwh: e_heat_max,
        q_heat_dem_kw: q_heat_dem,
        e_heat_target_kwh: e_heat_target,
        lambda_heat_sw_eur: lambda_sw,
        w_tier_penalty_eur: profile.planner.w_tier_penalty_eur,
        heat_initial_z_mid: heat_iz_mid,
        heat_initial_z_full: heat_iz_full,
        shiftable_loads: milp_loads,
    }
}

// ── MILP Solver ──────────────────────────────────────────────────────────────

/// Output from the MILP solver for one planning cycle.
/// All `Vec<f64>` fields have `len == n` except `e_bat_kwh` which has `len == n + 1`.
#[derive(Debug, Clone)]
struct SolveOutput {
    objective_eur: f64,
    /// Grid import power per step [kW]
    p_imp_kw: Vec<f64>,
    /// Grid export power per step [kW]
    p_exp_kw: Vec<f64>,
    /// Battery charge power per step [kW]; all 0.0 when no battery present
    p_bat_ch_kw: Vec<f64>,
    /// Battery discharge power per step [kW]; all 0.0 when no battery present
    p_bat_dis_kw: Vec<f64>,
    /// EV charge power per step [kW]; all 0.0 when EV absent/MustNotRun
    p_ev_kw: Vec<f64>,
    /// Heater mid-level binary (0/1) per step
    z_heat_mid: Vec<f64>,
    /// Heater full-level binary (0/1) per step
    z_heat_full: Vec<f64>,
    /// Battery SoC trajectory [kWh], len = n + 1; index 0 = initial SoC
    e_bat_kwh: Vec<f64>,
    /// Import contractual-limit violation slack [kW]
    s_imp_viol_kw: Vec<f64>,
    /// Export contractual-limit violation slack [kW]
    s_exp_viol_kw: Vec<f64>,
    /// EV on-flag binary per step (1 = charging, 0 = off)
    z_ev_on: Vec<f64>,
    /// Total extra EV energy above core requirement [kWh]
    e_ev_extra: f64,
    /// 1.0 when EV core target is met (MayRun only); 0.0 otherwise
    z_ev_core: f64,
    /// 1.0 when heater energy deadline is met (MayRun only); 0.0 otherwise
    z_heat_ready: f64,
    /// Tank energy above T_min [kWh] per slot; empty when heater absent
    e_heat_tank_kwh: Vec<f64>,
    /// Per-shiftable-load power schedule [kW]; outer len = num loads, inner len = n
    p_shiftable_kw: Vec<Vec<f64>>,
}

/// Run the MILP model and return the optimal schedule.
///
/// Uses per-asset context types from the assets module (`BatteryMilpContext`,
/// `EvMilpContext`, `HeaterMilpContext`) and the `MilpVarPool` / `AssetInteraction`
/// framework from `milp_interactions.rs`. The `MilpInputs` signature is preserved
/// so all existing unit tests continue to compile without modification.

const M_LOW_EUR_PER_KWH: f64 = 10.0;

/// Phase 1: minimise economic cost only. Battery and EV are declared without
/// startup/ramp aux vars (0.0 passed to `declare_vars`). Heater lambda_sw is 0.0.
fn solve_phase1(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
) -> Result<SolveOutput, Box<dyn std::error::Error>> {
    let n = inputs.n;
    let dt_h = inputs.dt_h;

    let bat_ctx: Option<BatteryMilpContext> = inputs.e_bat_nom_kwh.map(|e_nom| BatteryMilpContext {
        e_nom_kwh:    e_nom,
        e_init_kwh:   inputs.e_bat_init_kwh.unwrap_or(0.0),
        e_min_kwh:    inputs.e_bat_min_kwh.unwrap_or(0.0),
        e_max_kwh:    inputs.e_bat_max_kwh.unwrap_or(e_nom),
        p_ch_max_kw:  inputs.p_bat_ch_max_kw.unwrap_or(0.0),
        p_dis_max_kw: inputs.p_bat_dis_max_kw.unwrap_or(0.0),
        eff_ch:       inputs.eff_bat_ch.unwrap_or(1.0),
        eff_dis:      inputs.eff_bat_dis.unwrap_or(1.0),
    });

    let ev_ctx: Option<EvMilpContext> = if inputs.p_ev_max_kw > 0.0 {
        let ev_mode = match inputs.ev_mode {
            MilpLoadMode::MustRun => EvMilpMode::MustRun,
            MilpLoadMode::MayRun => EvMilpMode::MayRun,
            MilpLoadMode::MustNotRun => EvMilpMode::MustNotRun,
        };
        Some(EvMilpContext {
            mode: ev_mode,
            a_ev: inputs.a_ev.clone(),
            t_dead_step: inputs.t_ev_dead_step,
            p_max_kw: inputs.p_ev_max_kw,
            p_min_kw: inputs.p_ev_min_kw,
            e_core_kwh: inputs.e_ev_core_kwh,
            e_extra_max_kwh: inputs.e_ev_extra_max_kwh,
            v_extra_eur_kwh: inputs.v_ev_extra_eur_kwh,
        })
    } else {
        None
    };

    // Phase 1: lambda_sw_eur = 0.0 (switching penalty moves entirely to Phase 2).
    let heat_ctx: Option<HeaterMilpContext> = if inputs.p_heat_full_kw > 0.0 {
        let heat_mode = match inputs.heater_mode {
            MilpLoadMode::MustRun => HeaterMilpMode::MustRun,
            MilpLoadMode::MayRun => HeaterMilpMode::MayRun,
            MilpLoadMode::MustNotRun => HeaterMilpMode::MustNotRun,
        };
        Some(HeaterMilpContext {
            mode: heat_mode,
            t_dead_step: inputs.t_heat_dead_step,
            p_mid_kw: inputs.p_heat_mid_kw,
            p_full_kw: inputs.p_heat_full_kw,
            e_init_kwh: inputs.e_heat_init_kwh,
            e_max_kwh: inputs.e_heat_max_kwh,
            q_dem_kw: inputs.q_heat_dem_kw,
            e_target_kwh: inputs.e_heat_target_kwh,
            lambda_sw_eur: 0.0,
            initial_z_mid: inputs.heat_initial_z_mid,
            initial_z_full: inputs.heat_initial_z_full,
        })
    } else {
        None
    };

    let global = GlobalMilpInputs {
        n,
        dt_h,
        c_imp_eur_kwh: inputs.c_imp_eur_kwh.clone(),
        c_exp_eur_kwh: inputs.c_exp_eur_kwh.clone(),
        g_imp_kgco2_kwh: inputs.g_imp_kgco2_kwh.clone(),
        p_pv_kw: inputs.p_pv_kw.clone(),
        p_base_kw: inputs.p_base_kw.clone(),
        p_imp_max_phys_kw: inputs.p_imp_max_phys_kw.clone(),
        p_exp_max_phys_kw: inputs.p_exp_max_phys_kw.clone(),
        p_imp_max_cont_kw: inputs.p_imp_max_cont_kw.clone(),
        p_exp_max_cont_kw: inputs.p_exp_max_cont_kw.clone(),
        pen_imp_eur_kwh: inputs.pen_imp_eur_kwh,
        pen_exp_eur_kwh: inputs.pen_exp_eur_kwh,
    };

    let mut vars = variables!();

    let p_imp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let p_exp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let u_grid: Vec<Variable> = (0..n).map(|_| vars.add(variable().binary())).collect();
    let s_imp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let s_exp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let grid_vars = GridMilpVars {
        p_imp: p_imp.clone(),
        p_exp: p_exp.clone(),
        u_grid: u_grid.clone(),
        s_imp_viol: s_imp_viol.clone(),
        s_exp_viol: s_exp_viol.clone(),
    };

    // Phase 1: no startup/ramp aux vars.
    let bat_vars = bat_ctx.as_ref().map(|ctx| ctx.declare_vars(n, 0.0, 0.0, &mut vars));
    let ev_vars  = ev_ctx.as_ref().map(|ctx| ctx.declare_vars(n, 0.0, 0.0, &mut vars));
    let heat_vars = heat_ctx.as_ref().map(|ctx| ctx.declare_vars(n, &mut vars));

    let shift_vars: Vec<ShiftableLoadMilpVars> = inputs.shiftable_loads.iter().map(|sl| {
        let y_shift = sl.valid_start_slots.iter().map(|_| vars.add(variable().binary())).collect();
        ShiftableLoadMilpVars {
            asset_id: sl.asset_id.clone(),
            power_kw: sl.power_kw,
            duration_slots: sl.duration_slots,
            valid_start_slots: sl.valid_start_slots.clone(),
            y_shift,
        }
    }).collect();

    let pool = MilpVarPool { grid: grid_vars, bat: bat_vars, ev: ev_vars, heater: heat_vars, shiftable: shift_vars };

    let interactions = build_interactions(p1w.c_bat_ev_coexist_eur_kwh);
    let mut active_interactions: Vec<&Box<dyn crate::controller::milp_interactions::AssetInteraction>> = Vec::new();
    let mut iv_list: Vec<crate::controller::milp_interactions::InteractionVars> = Vec::new();
    for interaction in &interactions {
        if interaction.applicable(&pool) {
            let iv = interaction.declare_vars(&pool, &global, &mut vars);
            active_interactions.push(interaction);
            iv_list.push(iv);
        }
    }

    // Phase 1 objective: economic + m_low; no startup/ramp/switching/tier friction.
    let mut objective = Expression::from(0.0);
    for t in 0..n {
        objective += (p1w.w_energy * dt_h * inputs.c_imp_eur_kwh[t]) * p_imp[t];
        objective += -(p1w.w_energy * dt_h * inputs.c_exp_eur_kwh[t]) * p_exp[t];
        objective += (p1w.w_ghg * dt_h * inputs.g_imp_kgco2_kwh[t]) * p_imp[t];
        objective += (p1w.w_grid * dt_h) * p_imp[t];
        objective += (p1w.w_grid * dt_h) * p_exp[t];
        objective += (p1w.w_import * dt_h) * p_imp[t];
        objective += (p1w.w_viol * inputs.pen_imp_eur_kwh * dt_h) * s_imp_viol[t];
        objective += (p1w.w_viol * inputs.pen_exp_eur_kwh * dt_h) * s_exp_viol[t];
    }
    if let Some(v) = &pool.bat {
        objective += BatteryMilpContext::objective(v, p1w.c_bat_wear_eur_kwh, 0.0, 0.0, n, dt_h);
    }
    if let (Some(ctx), Some(v)) = (&ev_ctx, &pool.ev) {
        objective += ctx.objective(v, 0.0, 0.0, p1w.w_services, n);
    }
    if let (Some(ctx), Some(v)) = (&heat_ctx, &pool.heater) {
        // Phase 1 heater: m_low penalty only (tier=0, lambda_sw=0 on ctx).
        objective += ctx.objective(v, 0.0, M_LOW_EUR_PER_KWH, n);
    }
    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        objective += interaction.objective(iv, dt_h);
    }

    let mut model = vars.minimise(&objective).using(highs);
    model = add_model_constraints(model, inputs, &pool, &heat_ctx, &bat_ctx, &ev_ctx,
        &p_imp, &p_exp, &u_grid, &s_imp_viol, &s_exp_viol, &active_interactions, &iv_list, &global, n);
    model = model.with_time_limit(60.0);
    model = model.with_mip_gap(0.02)?;
    let solution = model.solve()?;

    Ok(read_solve_output(&solution, &objective, &pool, inputs, n))
}

/// Phase 2: minimise operational friction subject to phase1_cost(p2_vars) ≤ c_star + epsilon.
/// All variables are declared fresh. Battery/EV get startup/ramp aux vars.
/// Warm-start vector: Phase 1 solution values provided as initial MIP incumbent for Phase 2.
/// This ensures HiGHS immediately has a feasible integer point (the Phase 1 solution satisfies
/// all Phase 2 constraints), avoiding the NoSolutionFound timeout on Pi4 ARM.
fn build_phase2_warm_start(
    inputs: &MilpInputs,
    p1: &SolveOutput,
    p_imp: &[Variable],
    p_exp: &[Variable],
    u_grid: &[Variable],
    s_imp_viol: &[Variable],
    s_exp_viol: &[Variable],
    pool: &MilpVarPool,
    n: usize,
) -> Vec<(Variable, f64)> {
    let mut iv: Vec<(Variable, f64)> = Vec::with_capacity(n * 12);
    for t in 0..n {
        iv.push((p_imp[t], p1.p_imp_kw[t].max(0.0)));
        iv.push((p_exp[t], p1.p_exp_kw[t].max(0.0)));
        iv.push((u_grid[t], if p1.p_imp_kw[t] > 1e-6 { 1.0 } else { 0.0 }));
        iv.push((s_imp_viol[t], p1.s_imp_viol_kw[t].max(0.0)));
        iv.push((s_exp_viol[t], p1.s_exp_viol_kw[t].max(0.0)));
    }
    if let Some(v) = &pool.heater {
        let iz_mid = inputs.heat_initial_z_mid;
        let iz_full = inputs.heat_initial_z_full;
        for t in 0..n {
            let zm = p1.z_heat_mid[t];
            let zf = p1.z_heat_full[t];
            iv.push((v.z_heat_mid[t], zm));
            iv.push((v.z_heat_full[t], zf));
            let e = p1.e_heat_tank_kwh.get(t).copied().unwrap_or(0.0);
            iv.push((v.e_tank[t], e));
            iv.push((v.s_low[t], (-e).max(0.0)));
            let zm_prev = if t == 0 { iz_mid } else { p1.z_heat_mid[t - 1] };
            let zf_prev = if t == 0 { iz_full } else { p1.z_heat_full[t - 1] };
            let sw = (zm - zm_prev).abs().max((zf - zf_prev).abs());
            iv.push((v.sw[t], sw));
        }
        iv.push((v.z_heat_ready, p1.z_heat_ready));
    }
    if let Some(v) = &pool.bat {
        for t in 0..=n {
            if let (Some(&e_var), Some(&e_val)) = (v.e_bat.get(t), p1.e_bat_kwh.get(t)) {
                iv.push((e_var, e_val));
            }
        }
        for t in 0..n {
            iv.push((v.p_ch[t], p1.p_bat_ch_kw[t].max(0.0)));
            iv.push((v.p_dis[t], p1.p_bat_dis_kw[t].max(0.0)));
            let active = if p1.p_bat_ch_kw[t] + p1.p_bat_dis_kw[t] > 1e-6 { 1.0 } else { 0.0 };
            iv.push((v.u_bat[t], active));
            if let Some(&za) = v.z_active.get(t) { iv.push((za, active)); }
        }
        for i in 0..v.delta_active.len() {
            let t = i + 1;
            let z_prev = if p1.p_bat_ch_kw[i] + p1.p_bat_dis_kw[i] > 1e-6 { 1.0_f64 } else { 0.0 };
            let z_curr = if p1.p_bat_ch_kw[t] + p1.p_bat_dis_kw[t] > 1e-6 { 1.0_f64 } else { 0.0 };
            iv.push((v.delta_active[i], (z_curr - z_prev).max(0.0)));
        }
        for i in 0..v.delta_ramp.len() {
            let t = i + 1;
            let net_prev = p1.p_bat_ch_kw[i] - p1.p_bat_dis_kw[i];
            let net_curr = p1.p_bat_ch_kw[t] - p1.p_bat_dis_kw[t];
            iv.push((v.delta_ramp[i], (net_curr - net_prev).abs()));
        }
    }
    if let Some(v) = &pool.ev {
        for t in 0..n {
            iv.push((v.p_ev[t], p1.p_ev_kw[t].max(0.0)));
            iv.push((v.z_ev_on[t], p1.z_ev_on[t]));
        }
        for i in 0..v.delta_ev.len() {
            let t = i + 1;
            let delta = (p1.z_ev_on[t] - p1.z_ev_on[i]).max(0.0);
            iv.push((v.delta_ev[i], delta));
        }
        for i in 0..v.delta_ev_ramp.len() {
            let t = i + 1;
            iv.push((v.delta_ev_ramp[i], (p1.p_ev_kw[t] - p1.p_ev_kw[i]).abs()));
        }
        iv.push((v.e_ev_extra, p1.e_ev_extra.max(0.0)));
        iv.push((v.z_ev_core, p1.z_ev_core));
    }
    iv
}

fn solve_phase2(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    p2w: &Phase2Weights,
    c_star: f64,
    epsilon: f64,
    phase1_sol: &SolveOutput,
) -> Result<(SolveOutput, f64), Box<dyn std::error::Error>> {
    let n = inputs.n;
    let dt_h = inputs.dt_h;

    let bat_ctx: Option<BatteryMilpContext> = inputs.e_bat_nom_kwh.map(|e_nom| BatteryMilpContext {
        e_nom_kwh:    e_nom,
        e_init_kwh:   inputs.e_bat_init_kwh.unwrap_or(0.0),
        e_min_kwh:    inputs.e_bat_min_kwh.unwrap_or(0.0),
        e_max_kwh:    inputs.e_bat_max_kwh.unwrap_or(e_nom),
        p_ch_max_kw:  inputs.p_bat_ch_max_kw.unwrap_or(0.0),
        p_dis_max_kw: inputs.p_bat_dis_max_kw.unwrap_or(0.0),
        eff_ch:       inputs.eff_bat_ch.unwrap_or(1.0),
        eff_dis:      inputs.eff_bat_dis.unwrap_or(1.0),
    });

    let ev_ctx: Option<EvMilpContext> = if inputs.p_ev_max_kw > 0.0 {
        let ev_mode = match inputs.ev_mode {
            MilpLoadMode::MustRun => EvMilpMode::MustRun,
            MilpLoadMode::MayRun => EvMilpMode::MayRun,
            MilpLoadMode::MustNotRun => EvMilpMode::MustNotRun,
        };
        Some(EvMilpContext {
            mode: ev_mode,
            a_ev: inputs.a_ev.clone(),
            t_dead_step: inputs.t_ev_dead_step,
            p_max_kw: inputs.p_ev_max_kw,
            p_min_kw: inputs.p_ev_min_kw,
            e_core_kwh: inputs.e_ev_core_kwh,
            e_extra_max_kwh: inputs.e_ev_extra_max_kwh,
            v_extra_eur_kwh: inputs.v_ev_extra_eur_kwh,
        })
    } else {
        None
    };

    // Phase 2: heater carries switching penalty and tier penalty.
    let heat_ctx: Option<HeaterMilpContext> = if inputs.p_heat_full_kw > 0.0 {
        let heat_mode = match inputs.heater_mode {
            MilpLoadMode::MustRun => HeaterMilpMode::MustRun,
            MilpLoadMode::MayRun => HeaterMilpMode::MayRun,
            MilpLoadMode::MustNotRun => HeaterMilpMode::MustNotRun,
        };
        Some(HeaterMilpContext {
            mode: heat_mode,
            t_dead_step: inputs.t_heat_dead_step,
            p_mid_kw: inputs.p_heat_mid_kw,
            p_full_kw: inputs.p_heat_full_kw,
            e_init_kwh: inputs.e_heat_init_kwh,
            e_max_kwh: inputs.e_heat_max_kwh,
            q_dem_kw: inputs.q_heat_dem_kw,
            e_target_kwh: inputs.e_heat_target_kwh,
            lambda_sw_eur: p2w.lambda_heat_sw_eur,
            initial_z_mid: inputs.heat_initial_z_mid,
            initial_z_full: inputs.heat_initial_z_full,
        })
    } else {
        None
    };

    let global = GlobalMilpInputs {
        n,
        dt_h,
        c_imp_eur_kwh: inputs.c_imp_eur_kwh.clone(),
        c_exp_eur_kwh: inputs.c_exp_eur_kwh.clone(),
        g_imp_kgco2_kwh: inputs.g_imp_kgco2_kwh.clone(),
        p_pv_kw: inputs.p_pv_kw.clone(),
        p_base_kw: inputs.p_base_kw.clone(),
        p_imp_max_phys_kw: inputs.p_imp_max_phys_kw.clone(),
        p_exp_max_phys_kw: inputs.p_exp_max_phys_kw.clone(),
        p_imp_max_cont_kw: inputs.p_imp_max_cont_kw.clone(),
        p_exp_max_cont_kw: inputs.p_exp_max_cont_kw.clone(),
        pen_imp_eur_kwh: inputs.pen_imp_eur_kwh,
        pen_exp_eur_kwh: inputs.pen_exp_eur_kwh,
    };

    let mut vars = variables!();

    let p_imp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let p_exp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let u_grid: Vec<Variable> = (0..n).map(|_| vars.add(variable().binary())).collect();
    let s_imp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let s_exp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let grid_vars = GridMilpVars {
        p_imp: p_imp.clone(),
        p_exp: p_exp.clone(),
        u_grid: u_grid.clone(),
        s_imp_viol: s_imp_viol.clone(),
        s_exp_viol: s_exp_viol.clone(),
    };

    // Phase 2: startup/ramp aux vars are declared with real cost values.
    let bat_vars  = bat_ctx.as_ref().map(|ctx| ctx.declare_vars(n, p2w.c_bat_startup_eur, p2w.c_bat_ramp_eur_kw, &mut vars));
    let ev_vars   = ev_ctx.as_ref().map(|ctx| ctx.declare_vars(n, p2w.c_ev_startup_eur, p2w.c_ev_ramp_eur_kw, &mut vars));
    let heat_vars = heat_ctx.as_ref().map(|ctx| ctx.declare_vars(n, &mut vars));

    let shift_vars: Vec<ShiftableLoadMilpVars> = inputs.shiftable_loads.iter().map(|sl| {
        let y_shift = sl.valid_start_slots.iter().map(|_| vars.add(variable().binary())).collect();
        ShiftableLoadMilpVars {
            asset_id: sl.asset_id.clone(),
            power_kw: sl.power_kw,
            duration_slots: sl.duration_slots,
            valid_start_slots: sl.valid_start_slots.clone(),
            y_shift,
        }
    }).collect();

    let pool = MilpVarPool { grid: grid_vars, bat: bat_vars, ev: ev_vars, heater: heat_vars, shiftable: shift_vars };

    let interactions = build_interactions(p1w.c_bat_ev_coexist_eur_kwh);
    let mut active_interactions: Vec<&Box<dyn crate::controller::milp_interactions::AssetInteraction>> = Vec::new();
    let mut iv_list: Vec<crate::controller::milp_interactions::InteractionVars> = Vec::new();
    for interaction in &interactions {
        if interaction.applicable(&pool) {
            let iv = interaction.declare_vars(&pool, &global, &mut vars);
            active_interactions.push(interaction);
            iv_list.push(iv);
        }
    }

    // Phase 1 cost cap expression, rebuilt using Phase 2 variables.
    let mut phase1_cap_expr = Expression::from(0.0);
    for t in 0..n {
        phase1_cap_expr += (p1w.w_energy * dt_h * inputs.c_imp_eur_kwh[t]) * p_imp[t];
        phase1_cap_expr += -(p1w.w_energy * dt_h * inputs.c_exp_eur_kwh[t]) * p_exp[t];
        phase1_cap_expr += (p1w.w_ghg * dt_h * inputs.g_imp_kgco2_kwh[t]) * p_imp[t];
        phase1_cap_expr += (p1w.w_grid * dt_h) * p_imp[t];
        phase1_cap_expr += (p1w.w_grid * dt_h) * p_exp[t];
        phase1_cap_expr += (p1w.w_import * dt_h) * p_imp[t];
        phase1_cap_expr += (p1w.w_viol * inputs.pen_imp_eur_kwh * dt_h) * s_imp_viol[t];
        phase1_cap_expr += (p1w.w_viol * inputs.pen_exp_eur_kwh * dt_h) * s_exp_viol[t];
    }
    if let Some(v) = &pool.bat {
        // Battery wear only (0.0 startup/ramp in cost cap expression).
        phase1_cap_expr += BatteryMilpContext::objective(v, p1w.c_bat_wear_eur_kwh, 0.0, 0.0, n, dt_h);
    }
    if let (Some(ctx), Some(v)) = (&ev_ctx, &pool.ev) {
        // EV reward only (0.0 startup/ramp in cost cap expression).
        phase1_cap_expr += ctx.objective(v, 0.0, 0.0, p1w.w_services, n);
    }
    if let Some(v) = &pool.heater {
        // m_low heater term manually (bypass ctx.objective to exclude tier/switching).
        for t in 0..n {
            phase1_cap_expr += M_LOW_EUR_PER_KWH * v.s_low[t];
        }
    }
    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        phase1_cap_expr += interaction.objective(iv, dt_h);
    }

    // Phase 2 friction objective: startup/ramp/switching/tier; no economic terms.
    let mut friction_obj = Expression::from(0.0);
    if let Some(v) = &pool.bat {
        friction_obj += BatteryMilpContext::objective(v, 0.0, p2w.c_bat_startup_eur, p2w.c_bat_ramp_eur_kw, n, dt_h);
    }
    if let (Some(ctx), Some(v)) = (&ev_ctx, &pool.ev) {
        friction_obj += ctx.objective(v, p2w.c_ev_startup_eur, p2w.c_ev_ramp_eur_kw, 0.0, n);
    }
    if let (Some(ctx), Some(v)) = (&heat_ctx, &pool.heater) {
        // Tier + switching (lambda_sw_eur on ctx), no m_low.
        friction_obj += ctx.objective(v, p2w.w_tier_penalty_eur, 0.0, n);
    }

    let warm_start = build_phase2_warm_start(
        inputs, phase1_sol, &p_imp, &p_exp, &u_grid, &s_imp_viol, &s_exp_viol, &pool, n,
    );

    let mut model = vars.minimise(&friction_obj).using(highs);
    model = model.with_initial_solution(warm_start);
    model = model.with(constraint!(phase1_cap_expr <= c_star + epsilon));
    model = add_model_constraints(model, inputs, &pool, &heat_ctx, &bat_ctx, &ev_ctx,
        &p_imp, &p_exp, &u_grid, &s_imp_viol, &s_exp_viol, &active_interactions, &iv_list, &global, n);
    model = model.with_time_limit(60.0);
    model = model.with_mip_gap(0.02)?;
    let solution = model.solve()?;

    let friction_value = solution.eval(&friction_obj);
    let out = read_solve_output(&solution, &friction_obj, &pool, inputs, n);
    Ok((out, friction_value))
}

/// Lexicographic two-phase wrapper. Phase 1 always runs.
/// Phase 2 runs when `epsilon > 0`; on Phase 2 failure, Phase 1 solution is returned.
/// Returns `(solution, phase1_cost_eur, friction_eur)`.
fn solve_milp_two_phase(
    inputs: &MilpInputs,
    p1w: &Phase1Weights,
    p2w: &Phase2Weights,
    epsilon: f64,
) -> Result<(SolveOutput, f64, f64), Box<dyn std::error::Error>> {
    let phase1_sol = solve_phase1(inputs, p1w)?;
    let c_star = phase1_sol.objective_eur;
    if epsilon == 0.0 {
        return Ok((phase1_sol, c_star, 0.0));
    }
    tracing::debug!(c_star, epsilon, "Phase 2 starting");
    match solve_phase2(inputs, p1w, p2w, c_star, epsilon, &phase1_sol) {
        Ok((sol, friction_eur)) => Ok((sol, c_star, friction_eur)),
        Err(e) => {
            tracing::warn!(c_star, epsilon, "Phase 2 failed (warm-start provided), using Phase 1: {e}");
            Ok((phase1_sol, c_star, 0.0))
        }
    }
}

/// Helper: add power-balance and per-asset constraints to the model.
#[allow(clippy::too_many_arguments)]
fn add_model_constraints<S: SolverModel>(
    mut model: S,
    inputs: &MilpInputs,
    pool: &MilpVarPool,
    heat_ctx: &Option<HeaterMilpContext>,
    bat_ctx: &Option<BatteryMilpContext>,
    ev_ctx: &Option<EvMilpContext>,
    p_imp: &[Variable],
    p_exp: &[Variable],
    u_grid: &[Variable],
    s_imp_viol: &[Variable],
    s_exp_viol: &[Variable],
    active_interactions: &[&Box<dyn crate::controller::milp_interactions::AssetInteraction>],
    iv_list: &[crate::controller::milp_interactions::InteractionVars],
    global: &GlobalMilpInputs,
    n: usize,
) -> S {
    for t in 0..n {
        let mut shift_kw = Expression::from(0.0);
        for sv in &pool.shiftable {
            for (ji, &j) in sv.valid_start_slots.iter().enumerate() {
                if t >= j && t < j + sv.duration_slots {
                    shift_kw += sv.power_kw * sv.y_shift[ji];
                }
            }
        }

        let bat_dis: Expression = pool.bat.as_ref()
            .map(|v| Expression::from(v.p_dis[t]))
            .unwrap_or_else(|| Expression::from(0.0));
        let bat_ch: Expression = pool.bat.as_ref()
            .map(|v| Expression::from(v.p_ch[t]))
            .unwrap_or_else(|| Expression::from(0.0));
        let ev_kw: Expression = pool.ev.as_ref()
            .map(|v| Expression::from(v.p_ev[t]))
            .unwrap_or_else(|| Expression::from(0.0));
        let heat_kw: Expression = heat_ctx.as_ref().zip(pool.heater.as_ref())
            .map(|(ctx, v)| ctx.power_expr(v, t))
            .unwrap_or_else(|| Expression::from(0.0));

        model = model.with(constraint!(
            p_imp[t] + inputs.p_pv_kw[t] + bat_dis
                == inputs.p_base_kw[t] + ev_kw + heat_kw + shift_kw + bat_ch + p_exp[t]
        ));
        model = model.with(constraint!(p_imp[t] <= inputs.p_imp_max_phys_kw[t] * u_grid[t]));
        model = model.with(constraint!(p_exp[t] <= inputs.p_exp_max_phys_kw[t] * (1.0 - u_grid[t])));
        model = model.with(constraint!(p_imp[t] <= inputs.p_imp_max_cont_kw[t] + s_imp_viol[t]));
        model = model.with(constraint!(p_exp[t] <= inputs.p_exp_max_cont_kw[t] + s_exp_viol[t]));
    }

    if let (Some(ctx), Some(v)) = (bat_ctx, &pool.bat) {
        for c in ctx.constraints(v, n, global.dt_h) { model = model.with(c); }
    }
    if let (Some(ctx), Some(v)) = (ev_ctx, &pool.ev) {
        for c in ctx.constraints(v, n, global.dt_h) { model = model.with(c); }
    }
    if let (Some(ctx), Some(v)) = (heat_ctx, &pool.heater) {
        for c in ctx.constraints(v, n, global.dt_h) { model = model.with(c); }
    }

    for sv in &pool.shiftable {
        let mut sum_y = Expression::from(0.0);
        for &y in &sv.y_shift { sum_y += y; }
        model = model.with(constraint!(sum_y == 1.0));
    }

    for (interaction, iv) in active_interactions.iter().zip(iv_list.iter()) {
        for c in interaction.constraints(pool, iv, global) { model = model.with(c); }
    }
    model
}

/// Extract a `SolveOutput` from a solved `good_lp::Solution`.
fn read_solve_output<S: Solution>(
    solution: &S,
    objective: &Expression,
    pool: &MilpVarPool,
    inputs: &MilpInputs,
    n: usize,
) -> SolveOutput {
    let p_imp_ref = &pool.grid.p_imp;
    let p_exp_ref = &pool.grid.p_exp;
    let s_imp_ref = &pool.grid.s_imp_viol;
    let s_exp_ref = &pool.grid.s_exp_viol;

    let (bat_ch_kw, bat_dis_kw, e_bat_kwh) = if let Some(v) = &pool.bat {
        let sol = BatteryMilpContext::read_solution(solution, v, n);
        (sol.p_ch_kw, sol.p_dis_kw, sol.e_kwh)
    } else {
        (vec![0.0; n], vec![0.0; n], vec![0.0; n + 1])
    };

    let (ev_kw_out, z_ev_on_out, e_ev_extra_out, z_ev_core_out) =
        if let Some(v) = &pool.ev {
            let sol = EvMilpContext::read_solution(solution, v, n);
            (sol.p_ev_kw, sol.z_ev_on, sol.e_ev_extra_kwh, sol.z_ev_core)
        } else {
            (vec![0.0; n], vec![0.0; n], 0.0, 0.0)
        };

    let (z_heat_mid_out, z_heat_full_out, z_heat_ready_out, e_heat_tank_out) =
        if let Some(v) = &pool.heater {
            let sol = HeaterMilpContext::read_solution(solution, v, n);
            (sol.z_heat_mid, sol.z_heat_full, sol.z_heat_ready, sol.e_tank_kwh)
        } else {
            (vec![0.0; n], vec![0.0; n], 0.0, vec![])
        };

    let mut p_shiftable_kw = vec![vec![0.0; n]; inputs.shiftable_loads.len()];
    for (s, sv) in pool.shiftable.iter().enumerate() {
        for t in 0..n {
            for (ji, &j) in sv.valid_start_slots.iter().enumerate() {
                if t >= j && t < j + sv.duration_slots {
                    p_shiftable_kw[s][t] += sv.power_kw * solution.value(sv.y_shift[ji]);
                }
            }
        }
    }

    SolveOutput {
        objective_eur: solution.eval(objective),
        p_imp_kw: (0..n).map(|t| solution.value(p_imp_ref[t])).collect(),
        p_exp_kw: (0..n).map(|t| solution.value(p_exp_ref[t])).collect(),
        p_bat_ch_kw: bat_ch_kw,
        p_bat_dis_kw: bat_dis_kw,
        p_ev_kw: ev_kw_out,
        z_heat_mid: z_heat_mid_out,
        z_heat_full: z_heat_full_out,
        e_bat_kwh,
        s_imp_viol_kw: (0..n).map(|t| solution.value(s_imp_ref[t])).collect(),
        s_exp_viol_kw: (0..n).map(|t| solution.value(s_exp_ref[t])).collect(),
        z_ev_on: z_ev_on_out,
        e_ev_extra: e_ev_extra_out,
        z_ev_core: z_ev_core_out,
        z_heat_ready: z_heat_ready_out,
        e_heat_tank_kwh: e_heat_tank_out,
        p_shiftable_kw,
    }
}

// ── Output translator ───────────────────────────────────────────────────────

/// Build per-device schedulability metadata for all active device sessions.
fn build_plan_envelopes(
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    inputs: &MilpInputs,
    profile: &Profile,
    now: DateTime<Utc>,
) -> Vec<FlexibilityEnvelope> {
    let step_s = profile.planner.plan_step_s as i64;
    let n = inputs.n;
    let mut envelopes = Vec::new();

    // EV envelope
    if let Some(session) = ev_session {
        if let Some(ev_cfg) = profile.ev_config() {
            // Remaining energy to charge
            let energy_needed_kwh = inputs.e_ev_core_kwh;
            if energy_needed_kwh > 0.0 {
                let window_start = now;
                let window_end = session.departure_time;
                let slots_available = ((window_end - window_start).num_seconds() / step_s).max(0) as usize;
                let t_start = 0usize;
                let t_end = ((window_end - now).num_seconds() / step_s).min(n as i64) as usize;
                let eligible = t_start..t_end;
                let count = eligible.len().max(1) as f64;
                let avg_tariff = (t_start..t_end).map(|t| inputs.c_imp_eur_kwh[t]).sum::<f64>() / count;
                let avg_co2 = (t_start..t_end).map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0).sum::<f64>() / count;
                envelopes.push(FlexibilityEnvelope {
                    asset_id: ev_cfg.id.clone(),
                    energy_needed_kwh,
                    power_min_kw: ev_cfg.min_charge_kw,
                    power_max_kw: ev_cfg.max_charge_kw,
                    window_start,
                    window_end,
                    slots_available,
                    max_acceptable_rate: 0.35,
                    min_acceptable_rate: 0.05,
                    budget_remaining_eur: 1.0e9,
                    estimated_cost_eur: energy_needed_kwh * avg_tariff,
                    estimated_co2_g: energy_needed_kwh * avg_co2,
                });
            }
        }
    }

    // Heater envelope
    if let Some(target) = heater_target {
        if let Some(heat_cfg) = profile.heater_config() {
            let energy_needed_kwh = inputs.e_heat_target_kwh;
            if energy_needed_kwh > 0.0 {
                let window_start = now;
                let window_end = target.ready_by;
                let slots_available = ((window_end - window_start).num_seconds() / step_s).max(0) as usize;
                let t_end = ((window_end - now).num_seconds() / step_s).min(n as i64) as usize;
                let count = t_end.max(1) as f64;
                let avg_tariff = (0..t_end).map(|t| inputs.c_imp_eur_kwh[t]).sum::<f64>() / count;
                let avg_co2 = (0..t_end).map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0).sum::<f64>() / count;
                envelopes.push(FlexibilityEnvelope {
                    asset_id: heat_cfg.id.clone(),
                    energy_needed_kwh,
                    power_min_kw: 0.0,
                    power_max_kw: inputs.p_heat_full_kw,
                    window_start,
                    window_end,
                    slots_available,
                    max_acceptable_rate: 0.35,
                    min_acceptable_rate: 0.05,
                    budget_remaining_eur: 1.0e9,
                    estimated_cost_eur: energy_needed_kwh * avg_tariff,
                    estimated_co2_g: energy_needed_kwh * avg_co2,
                });
            }
        }
    }

    // Shiftable load envelopes
    for (s, sl) in shiftable_loads.iter().enumerate() {
        if s >= inputs.shiftable_loads.len() { break; }
        let milp_sl = &inputs.shiftable_loads[s];
        let energy_needed_kwh = sl.power_kw * sl.duration_min as f64 / 60.0;
        let window_start = sl.earliest_start.max(now);
        let window_end = sl.latest_end;
        let slots_available = ((window_end - window_start).num_seconds() / step_s).max(0) as usize;
        let t_start = ((window_start - now).num_seconds() / step_s).max(0) as usize;
        let t_end = ((window_end - now).num_seconds() / step_s).min(n as i64) as usize;
        let count = (t_end.saturating_sub(t_start)).max(1) as f64;
        let avg_tariff = (t_start..t_end).map(|t| inputs.c_imp_eur_kwh[t]).sum::<f64>() / count;
        let avg_co2 = (t_start..t_end).map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0).sum::<f64>() / count;
        let _ = milp_sl;
        envelopes.push(FlexibilityEnvelope {
            asset_id: sl.asset_id.clone(),
            energy_needed_kwh,
            power_min_kw: sl.power_kw,
            power_max_kw: sl.power_kw,
            window_start,
            window_end,
            slots_available,
            max_acceptable_rate: 0.35,
            min_acceptable_rate: 0.05,
            budget_remaining_eur: 1.0e9,
            estimated_cost_eur: energy_needed_kwh * avg_tariff,
            estimated_co2_g: energy_needed_kwh * avg_co2,
        });
    }

    envelopes
}

/// Fallback plan returned when the MILP solver fails.
/// When `inputs` is `Some`, emits populated slots with zero allocations
/// so tests asserting on per-slot fields still find data.
fn fallback_plan(
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    inputs: Option<&MilpInputs>,
    reason: String,
    objective: PlannerObjective,
) -> Plan {
    let step_s = profile.planner.plan_step_s;
    let horizon_h = profile.planner.plan_horizon_h;
    let horizon_end = now + Duration::seconds((horizon_h as f64 * 3600.0) as i64);
    let total_steps = ((horizon_h as f64 * 3600.0) / step_s as f64) as usize;

    let horizon = PlanningHorizon {
        start_time: now,
        end_time: horizon_end,
        step_size_s: step_s,
        num_steps: total_steps,
        far_horizon: horizon_end,
    };
    let warning = PlanWarning {
        severity: WarningSeverity::Critical,
        message: reason,
        suggested_action: None,
    };
    let slots: Vec<PlanTimeSlot> = match inputs {
        Some(inp) => (0..inp.n)
            .map(|t| {
                let step_s_i64 = step_s as i64;
                PlanTimeSlot {
                    slot_index: t,
                    start: now + Duration::seconds(t as i64 * step_s_i64),
                    end: now + Duration::seconds((t as i64 + 1) * step_s_i64),
                    import_tariff_eur_kwh: inp.c_imp_eur_kwh[t],
                    export_tariff_eur_kwh: inp.c_exp_eur_kwh[t],
                    co2_g_kwh: inp.g_imp_kgco2_kwh[t] * 1000.0,
                    grid_effective_cost: inp.c_imp_eur_kwh[t],
                    rate_estimated: false,
                    import_cap_kw: inp.p_imp_max_cont_kw[t],
                    export_cap_kw: inp.p_exp_max_cont_kw[t],
                    baseline_kw: inp.p_base_kw[t],
                    pv_forecast_kw: inp.p_pv_kw[t],
                    surplus_available_kw: (inp.p_pv_kw[t] - inp.p_base_kw[t]).max(0.0),
                    allocations: vec![],
                    net_import_kw: 0.0,
                    net_export_kw: 0.0,
                    import_flexibility_kw: 0.0,
                    export_flexibility_kw: 0.0,
                    bat_charge_kw: 0.0,
                    bat_discharge_kw: 0.0,
                    planned_kw_by_asset: std::collections::HashMap::new(),
                }
            })
            .collect(),
        None => vec![],
    };
    let envelopes = match inputs {
        Some(inp) => build_plan_envelopes(ev_session, heater_target, shiftable_loads, inp, profile, now),
        None => vec![],
    };
    let plan = Plan {
        id: Uuid::new_v4(),
        created_at: now,
        trigger,
        horizon,
        slots,
        summary: PlanSummary::default(),
        envelopes,
        warnings: vec![warning],
        objective,
        soc_trajectory_kwh: vec![],
        objective_eur: 0.0,
        friction_eur: 0.0,
        cost_breakdown: CostBreakdown::default(),
    };
    plan
}

/// Translate a MILP solution into a `Plan` with per-slot allocations.
fn translate_to_plan(
    sol: &SolveOutput,
    inputs: &MilpInputs,
    weights: &Phase1Weights,
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    objective: PlannerObjective,
    phase1_cost_eur: f64,
    friction_eur: f64,
) -> Plan {
    let step_s = profile.planner.plan_step_s;
    let n = inputs.n;
    let dt_h = inputs.dt_h;
    let horizon_end = now + Duration::seconds((n as i64) * step_s as i64);
    let horizon = PlanningHorizon {
        start_time: now,
        end_time: horizon_end,
        step_size_s: step_s,
        num_steps: n,
        far_horizon: horizon_end,
    };

    let ev_id = profile.ev_config().map(|c| c.id.clone());
    let heater_id = profile.heater_config().map(|c| c.id.clone());
    let bat_id = profile.battery_config().map(|c| c.id.clone());

    let mut slots = Vec::with_capacity(n);
    let mut violation_count: usize = 0;

    for t in 0..n {
        let slot_start = now + Duration::seconds((t as i64) * step_s as i64);
        let slot_end = now + Duration::seconds(((t + 1) as i64) * step_s as i64);
        let surplus_available_kw = (inputs.p_pv_kw[t] - inputs.p_base_kw[t]).max(0.0);
        let mut surplus_remaining_kw = surplus_available_kw;

        let mut allocations = Vec::new();

        // ── EV allocation ───────────────────────────────────────────────
        if inputs.ev_mode != MilpLoadMode::MustNotRun && sol.p_ev_kw[t] > 0.01 {
            if let Some(ref eid) = ev_id {
                let power_kw = sol.p_ev_kw[t];
                let surplus_power_kw = surplus_remaining_kw.min(power_kw);
                let grid_power_kw = power_kw - surplus_power_kw;
                surplus_remaining_kw -= surplus_power_kw;
                allocations.push(AssetAllocation {
                    asset_id: eid.clone(),
                    power_kw,
                    surplus_power_kw,
                    grid_power_kw,
                    marginal_value: inputs.c_imp_eur_kwh[t],
                    cost_eur: grid_power_kw * inputs.c_imp_eur_kwh[t] * dt_h
                        - surplus_power_kw * inputs.c_exp_eur_kwh[t] * dt_h,
                    co2_g: grid_power_kw * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h,
                });
            }
        }

        // ── Heater allocation ───────────────────────────────────────────
        if inputs.heater_mode != MilpLoadMode::MustNotRun {
            let heat_kw =
                sol.z_heat_mid[t] * inputs.p_heat_mid_kw + sol.z_heat_full[t] * inputs.p_heat_full_kw;
            if heat_kw > 0.01 {
                if let Some(ref hid) = heater_id {
                    let surplus_power_kw = surplus_remaining_kw.min(heat_kw);
                    let grid_power_kw = heat_kw - surplus_power_kw;
                    surplus_remaining_kw -= surplus_power_kw;
                    allocations.push(AssetAllocation {
                        asset_id: hid.clone(),
                        power_kw: heat_kw,
                        surplus_power_kw,
                        grid_power_kw,
                        marginal_value: inputs.c_imp_eur_kwh[t],
                        cost_eur: grid_power_kw * inputs.c_imp_eur_kwh[t] * dt_h
                            - surplus_power_kw * inputs.c_exp_eur_kwh[t] * dt_h,
                        co2_g: grid_power_kw * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h,
                    });
                }
            }
        }

        // ── Shiftable load allocations ──────────────────────────────────
        for (s, sl) in inputs.shiftable_loads.iter().enumerate() {
            let power = sol.p_shiftable_kw[s][t];
            if power > 0.01 {
                let surplus_power_kw = surplus_remaining_kw.min(power);
                let grid_power_kw = power - surplus_power_kw;
                surplus_remaining_kw -= surplus_power_kw;
                allocations.push(AssetAllocation {
                    asset_id: sl.asset_id.clone(),
                    power_kw: power,
                    surplus_power_kw,
                    grid_power_kw,
                    marginal_value: inputs.c_imp_eur_kwh[t],
                    cost_eur: grid_power_kw * inputs.c_imp_eur_kwh[t] * dt_h
                        - surplus_power_kw * inputs.c_exp_eur_kwh[t] * dt_h,
                    co2_g: grid_power_kw * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h,
                });
            }
        }

        // ── Battery allocation ────────────────────────────────────────────
        if let Some(ref bid) = bat_id {
            let bat_net_kw = sol.p_bat_ch_kw[t] - sol.p_bat_dis_kw[t];
            if bat_net_kw.abs() > 0.01 {
                let (surplus_power_kw, grid_power_kw, cost_eur, co2_g) = if bat_net_kw > 0.0 {
                    // Charging: consume PV surplus first, then grid
                    let sp = surplus_remaining_kw.min(bat_net_kw);
                    let gp = bat_net_kw - sp;
                    (sp, gp,
                     gp * inputs.c_imp_eur_kwh[t] * dt_h - sp * inputs.c_exp_eur_kwh[t] * dt_h,
                     gp * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h)
                } else {
                    // Discharging: negative power_kw = net injection; revenue = negative cost
                    let dis_kw = sol.p_bat_dis_kw[t];
                    (0.0, bat_net_kw,
                     -(dis_kw * inputs.c_exp_eur_kwh[t] * dt_h),
                     -(dis_kw * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h))
                };
                allocations.push(AssetAllocation {
                    asset_id: bid.clone(),
                    power_kw: bat_net_kw,
                    surplus_power_kw,
                    grid_power_kw,
                    marginal_value: inputs.c_exp_eur_kwh[t],
                    cost_eur,
                    co2_g,
                });
            }
        }

        // ── Track violations ────────────────────────────────────────────
        if sol.s_imp_viol_kw[t] > 0.01 || sol.s_exp_viol_kw[t] > 0.01 {
            violation_count += 1;
        }

        // ── Assemble slot ───────────────────────────────────────────────
        slots.push(PlanTimeSlot {
            slot_index: t,
            start: slot_start,
            end: slot_end,
            import_tariff_eur_kwh: inputs.c_imp_eur_kwh[t],
            export_tariff_eur_kwh: inputs.c_exp_eur_kwh[t],
            co2_g_kwh: inputs.g_imp_kgco2_kwh[t] * 1000.0,
            grid_effective_cost: inputs.c_imp_eur_kwh[t],
            rate_estimated: false,
            import_cap_kw: inputs.p_imp_max_cont_kw[t],
            export_cap_kw: inputs.p_exp_max_cont_kw[t],
            baseline_kw: inputs.p_base_kw[t],
            pv_forecast_kw: inputs.p_pv_kw[t],
            surplus_available_kw,
            planned_kw_by_asset: allocations.iter()
                .map(|a| (a.asset_id.clone(), a.power_kw))
                .collect(),
            allocations,
            net_import_kw: sol.p_imp_kw[t],
            net_export_kw: sol.p_exp_kw[t],
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            bat_charge_kw: sol.p_bat_ch_kw[t],
            bat_discharge_kw: sol.p_bat_dis_kw[t],
        });
    }

    // ── SoC trajectory ──────────────────────────────────────────────────
    let soc_trajectory_kwh = sol.e_bat_kwh.clone();

    // ── Summary (raw energy economics, no weights) ──────────────────────
    let summary = PlanSummary {
        total_cost_eur: (0..n)
            .map(|t| {
                (inputs.c_imp_eur_kwh[t] * sol.p_imp_kw[t]
                    - inputs.c_exp_eur_kwh[t] * sol.p_exp_kw[t])
                    * dt_h
            })
            .sum(),
        total_co2_g: (0..n)
            .map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0 * sol.p_imp_kw[t] * dt_h)
            .sum(),
        total_import_kwh: sol.p_imp_kw.iter().sum::<f64>() * dt_h,
        total_export_kwh: sol.p_exp_kw.iter().sum::<f64>() * dt_h,
    };

    // ── Cost breakdown (post-hoc from solution × weights) ───────────────
    let cost_breakdown = CostBreakdown {
        c_energy_eur: (0..n)
            .map(|t| {
                weights.w_energy
                    * (inputs.c_imp_eur_kwh[t] * sol.p_imp_kw[t]
                        - inputs.c_exp_eur_kwh[t] * sol.p_exp_kw[t])
                    * dt_h
            })
            .sum(),
        c_ghg_eur: (0..n)
            .map(|t| weights.w_ghg * inputs.g_imp_kgco2_kwh[t] * sol.p_imp_kw[t] * dt_h)
            .sum(),
        c_grid_eur: (0..n)
            .map(|t| weights.w_grid * (sol.p_imp_kw[t] + sol.p_exp_kw[t]) * dt_h)
            .sum(),
        c_wear_eur: (0..n)
            .map(|t| {
                weights.c_bat_wear_eur_kwh
                    * (sol.p_bat_ch_kw[t] + sol.p_bat_dis_kw[t])
                    * dt_h
            })
            .sum(),
        c_violations_eur: (0..n)
            .map(|t| {
                weights.w_viol
                    * (inputs.pen_imp_eur_kwh * sol.s_imp_viol_kw[t]
                        + inputs.pen_exp_eur_kwh * sol.s_exp_viol_kw[t])
                    * dt_h
            })
            .sum(),
        v_services_eur: 0.0,
    };

    // ── Warnings ────────────────────────────────────────────────────────
    let mut warnings = Vec::new();
    if violation_count > 0 {
        warnings.push(PlanWarning {
            severity: WarningSeverity::Warning,
            message: format!(
                "Grid capacity violation in {violation_count} slot(s) — solver used slack"
            ),
            suggested_action: None,
        });
    }

    // ── Assemble plan ───────────────────────────────────────────────────
    let plan = Plan {
        id: Uuid::new_v4(),
        created_at: now,
        trigger,
        horizon,
        slots,
        summary,
        envelopes: build_plan_envelopes(ev_session, heater_target, shiftable_loads, inputs, profile, now),
        warnings,
        objective,
        soc_trajectory_kwh,
        objective_eur: phase1_cost_eur,
        friction_eur,
        cost_breakdown,
    };
    plan
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Run the MILP planner: build inputs from live state, solve via HiGHS,
/// and translate the solution into a Plan.
/// `objective_override` — when `Some`, overrides the profile's default objective.
pub fn run_planner(
    assets: &SimState,
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    baseline_override: Option<&BaselineOverride>,
    objective_override: Option<PlannerObjective>,
) -> Plan {
    let objective = objective_override.unwrap_or(profile.planner.objective);
    let inputs = build_milp_inputs(assets, tariffs, capacity, profile, now, ev_session, heater_target, shiftable_loads, baseline_override);
    let p1w = build_phase1_weights(profile, objective);
    let p2w = build_phase2_weights(&inputs, profile);
    match solve_milp_two_phase(&inputs, &p1w, &p2w, profile.planner.phase2_epsilon_eur) {
        Ok((sol, phase1_cost_eur, friction_eur)) => translate_to_plan(&sol, &inputs, &p1w, profile, now, trigger, ev_session, heater_target, shiftable_loads, objective, phase1_cost_eur, friction_eur),
        Err(e) => {
            warn!("MILP solver failed: {e}");
            fallback_plan(
                profile,
                now,
                trigger,
                ev_session,
                heater_target,
                shiftable_loads,
                Some(&inputs),
                format!("MILP solver failed: {e}"),
                objective,
            )
        }
    }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use crate::assets::AssetState;
    use crate::entities::device_session::HeaterTarget;
    use crate::entities::tariff_snapshot::TariffSnapshot;
    use crate::profile::{
        AssetProfile, BaseLoadConfig, BatteryConfig, EvConfig, GridConfig, HeaterConfig,
        PlannerConfig, PlannerObjective, PvConfig, SimulatorConfig,
    };
    use crate::simulator::SimState;

    // ── Test helpers ─────────────────────────────────────────────────────────

    fn fixed_now() -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 4, 11, 6, 0, 0).unwrap()
    }

    /// Build a TariffTimeSeries with a single constant interval covering the full horizon.
    fn make_tariffs(imp: f64, exp: f64, co2_g: f64) -> TariffTimeSeries {
        let now = fixed_now();
        let snap = TariffSnapshot {
            interval_start: now - Duration::hours(1),
            interval_end: now + Duration::hours(25),
            import_tariff_eur_kwh: Some(imp),
            export_tariff_eur_kwh: Some(exp),
            co2_g_kwh: Some(co2_g),
        };
        TariffTimeSeries::from_snapshots(&[snap])
    }

    fn no_capacity() -> OadrCapacityState {
        OadrCapacityState {
            import_limit_kw: None,
            export_limit_kw: None,
            import_subscription_kw: None,
            import_reservation_kw: None,
            import_limit_event_id: None,
            export_limit_event_id: None,
            last_updated: None,
        }
    }

    /// Build a Profile with battery + EV + heater + PV + base_load.
    fn make_profile() -> Profile {
        Profile {
            devices: Default::default(),
            assets: vec![
                AssetProfile::Battery(BatteryConfig {
                    id: "battery".into(),
                    capacity_kwh: 10.0,
                    max_charge_kw: 5.0,
                    max_discharge_kw: 5.0,
                    initial_soc: 0.5,
                    round_trip_efficiency: 0.9,
                    min_soc: 0.1,
                }),
                AssetProfile::Ev(EvConfig {
                    id: "ev".into(),
                    max_charge_kw: 7.4,
                    max_discharge_kw: 0.0,
                    initial_soc: 0.2,
                    battery_kwh: 60.0,
                    soc_target: 0.8,
                    default_charge_kw: 0.0,
                    min_charge_kw: 1.4,
                }),
                AssetProfile::Heater(HeaterConfig {
                    id: "heater".into(),
                    max_kw: 3.0,
                    temp_initial_c: 20.0,
                    temp_min_c: 18.0,
                    temp_max_c: 23.0,
                    mid_kw: None,
                    volume_l: None,
                    thermal_mass_kwh_per_c: None,
                    k_loss_kw_per_c: None,
                    draw_kw: None,
                    switching_penalty_eur: None,
                }),
                AssetProfile::Pv(PvConfig {
                    id: "pv".into(),
                    rated_kw: 5.0,
                }),
                AssetProfile::BaseLoad(BaseLoadConfig {
                    id: "base_load".into(),
                    baseline_kw: 0.5,
                }),
            ],
            simulator: SimulatorConfig::default(),
            planner: PlannerConfig {
                plan_step_s: 300,   // 5 min steps
                plan_horizon_h: 2,  // 2-hour horizon → 24 steps
                ..PlannerConfig::default()
            },
            grid: GridConfig {
                max_import_kw: 25.0,
                max_export_kw: 10.0,
            },
            packets: vec![],
        }
    }

    /// Set plugged state on the EV in an existing SimState.
    fn set_ev_plugged(sim: &mut SimState, plugged: bool) {
        for entry in &mut sim.assets {
            if let AssetState::Ev(ref mut ev) = entry.state {
                ev.plugged = plugged;
            }
        }
    }

    /// Set battery SoC on the battery in an existing SimState.
    fn set_battery_soc(sim: &mut SimState, soc: f64) {
        for entry in &mut sim.assets {
            if let AssetState::Battery(ref mut bat) = entry.state {
                bat.soc = soc;
            }
        }
    }

    /// Set heater temperature on the heater in an existing SimState.
    fn set_heater_temp(sim: &mut SimState, temp_c: f64) {
        for entry in &mut sim.assets {
            if let AssetState::Heater(ref mut h) = entry.state {
                h.temperature_c = temp_c;
            }
        }
    }

    /// Build a Profile with only a heater (no battery, EV, PV, base_load).
    fn make_heater_only_profile(
        volume_l: Option<f64>,
        temp_min_c: f64,
        temp_max_c: f64,
        temp_initial_c: f64,
    ) -> Profile {
        Profile {
            devices: Default::default(),
            assets: vec![AssetProfile::Heater(HeaterConfig {
                id: "heater".into(),
                max_kw: 3.0,
                temp_initial_c,
                temp_min_c,
                temp_max_c,
                mid_kw: None,
                volume_l,
                thermal_mass_kwh_per_c: None,
                k_loss_kw_per_c: None,
                draw_kw: None,
                switching_penalty_eur: None,
            })],
            simulator: SimulatorConfig::default(),
            planner: PlannerConfig {
                plan_step_s: 300,
                plan_horizon_h: 2,
                ..PlannerConfig::default()
            },
            grid: GridConfig {
                max_import_kw: 25.0,
                max_export_kw: 10.0,
            },
            packets: vec![],
        }
    }

    // ── Tests ────────────────────────────────────────────────────────────────

    #[test]
    fn co2_g_kwh_divided_by_1000() {
        // CO₂ stored in tariffs as g/kWh; MILP needs kgCO₂/kWh
        let now = fixed_now();
        let profile = make_profile();
        let tariffs = make_tariffs(0.25, 0.08, 450.0); // 450 g/kWh
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &tariffs, &no_capacity(), &profile, now, None, None, &[], None);
        // All slots should have 0.45 kgCO₂/kWh
        assert!(inp.g_imp_kgco2_kwh.iter().all(|&v| (v - 0.45).abs() < 1e-9));
    }

    #[test]
    fn battery_eff_is_sqrt_rte() {
        // Each direction gets √(round_trip_efficiency), not the full RTE
        let now = fixed_now();
        let profile = make_profile(); // battery RTE = 0.9
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        let expected = 0.9_f64.sqrt();
        assert!((inp.eff_bat_ch.unwrap() - expected).abs() < 1e-9);
        assert!((inp.eff_bat_dis.unwrap() - expected).abs() < 1e-9);
        // Symmetry: both directions use the same value
        assert!((inp.eff_bat_ch.unwrap() - inp.eff_bat_dis.unwrap()).abs() < 1e-9);
    }

    #[test]
    fn battery_init_soc_uses_live_state() {
        // When SimState has battery with SoC=0.3, build_milp_inputs should use 0.3×capacity,
        // not the profile's initial_soc=0.5.
        let now = fixed_now();
        let profile = make_profile(); // initial_soc=0.5, capacity=10.0
        let mut sim = SimState::from_profile(&profile);
        set_battery_soc(&mut sim, 0.3); // override to 0.3
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert!((inp.e_bat_init_kwh.unwrap() - 3.0).abs() < 1e-9); // 0.3 × 10.0 = 3.0
    }

    #[test]
    fn battery_init_soc_falls_back_to_profile() {
        // When SimState has no battery asset, fall back to profile.battery_config().initial_soc
        let now = fixed_now();
        let profile = make_profile(); // initial_soc=0.5, capacity=10.0
        let mut sim = SimState::from_profile(&profile);
        sim.assets.clear(); // remove all assets → battery_state() returns None
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert!((inp.e_bat_init_kwh.unwrap() - 5.0).abs() < 1e-9); // 0.5 × 10.0 = 5.0
    }

    #[test]
    fn ev_mask_plugged_no_session_all_true() {
        // Plugged EV with no session → all slots available (mask true), mode MustNotRun
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert!(inp.a_ev.iter().all(|&v| v));
        assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun); // no session → MustNotRun (but mask is true)
        assert_eq!(inp.t_ev_dead_step, None);
    }

    #[test]
    fn ev_mask_plugged_with_session_deadline() {
        // Plugged EV with EvSession deadline at 1h → first 12 slots true (step=300s, 12×300=3600s)
        let now = fixed_now();
        let profile = make_profile(); // plan_step_s=300, plan_horizon_h=2 → 24 steps
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let session = crate::entities::device_session::EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.9,
            departure_time: now + Duration::hours(1),
            soft_deadline: false,
            created_at: now,
            updated_at: now,
        };
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, Some(&session), None, &[], None);
        // deadline = 3600s, step_s=300 → deadline_step = 12
        let d = inp.t_ev_dead_step.unwrap();
        assert_eq!(d, 12);
        // Slots 0..=12 true, slots 13..23 false
        for t in 0..inp.n {
            assert_eq!(inp.a_ev[t], t <= d, "slot {t} mask mismatch");
        }
    }

    #[test]
    fn ev_mask_unplugged_all_false() {
        // Unplugged EV → all slots false regardless of session
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, false);
        let session = crate::entities::device_session::EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.9,
            departure_time: now + Duration::hours(1),
            soft_deadline: false,
            created_at: now,
            updated_at: now,
        };
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, Some(&session), None, &[], None);
        assert!(inp.a_ev.iter().all(|&v| !v));
        assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun);
    }

    #[test]
    fn ev_mode_must_run_for_firm_deadline_session() {
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let session = crate::entities::device_session::EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.9,
            departure_time: now + Duration::hours(2),
            soft_deadline: false,
            created_at: now,
            updated_at: now,
        };
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, Some(&session), None, &[], None);
        assert_eq!(inp.ev_mode, MilpLoadMode::MustRun);
    }

    #[test]
    fn ev_mode_may_run_for_soft_deadline_session() {
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let session = crate::entities::device_session::EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.9,
            departure_time: now + Duration::hours(2),
            soft_deadline: true,
            created_at: now,
            updated_at: now,
        };
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, Some(&session), None, &[], None);
        assert_eq!(inp.ev_mode, MilpLoadMode::MayRun);
    }

    #[test]
    fn ev_mode_must_not_run_for_no_session() {
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        // No session at all
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun);
    }

    #[test]
    fn tariff_fallback_when_series_empty() {
        // Empty TariffTimeSeries → defaults: imp=0.25, exp=0.08, co2=300g→0.30kg
        let now = fixed_now();
        let profile = make_profile();
        let sim = SimState::from_profile(&profile);
        let empty_tariffs = TariffTimeSeries::from_snapshots(&[]);
        let inp = build_milp_inputs(&sim, &empty_tariffs, &no_capacity(), &profile, now, None, None, &[], None);
        assert!(inp.c_imp_eur_kwh.iter().all(|&v| (v - 0.25).abs() < 1e-9));
        assert!(inp.c_exp_eur_kwh.iter().all(|&v| (v - 0.08).abs() < 1e-9));
        assert!(inp.g_imp_kgco2_kwh.iter().all(|&v| (v - 0.30).abs() < 1e-9));
    }

    #[test]
    fn heater_mid_kw_defaults_to_half_max() {
        // HeaterConfig.mid_kw = None → p_heat_mid_kw = max_kw / 2.0
        let now = fixed_now();
        let profile = make_profile(); // heater max_kw=3.0, mid_kw=None
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true); // avoid EV noise
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert!((inp.p_heat_mid_kw - 1.5).abs() < 1e-9); // 3.0 / 2.0
        assert!((inp.p_heat_full_kw - 3.0).abs() < 1e-9);
    }

    #[test]
    fn heater_mid_kw_uses_explicit_value() {
        // HeaterConfig.mid_kw = Some(2.0) → p_heat_mid_kw = 2.0
        let now = fixed_now();
        let mut profile = make_profile();
        // Replace heater with one that has explicit mid_kw
        profile.assets = profile
            .assets
            .into_iter()
            .map(|a| match a {
                AssetProfile::Heater(mut h) => {
                    h.mid_kw = Some(2.0);
                    AssetProfile::Heater(h)
                }
                other => other,
            })
            .collect();
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert!((inp.p_heat_mid_kw - 2.0).abs() < 1e-9);
    }

    #[test]
    fn weights_preset_min_cost() {
        let mut profile = make_profile();
        profile.planner.w_energy = 99.0; // should be overridden by preset
        profile.planner.w_ghg = 99.0;
        let w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        assert!((w.w_energy - 1.0).abs() < 1e-9);
        assert!((w.w_ghg - 0.20).abs() < 1e-9);
        assert!((w.w_grid - 0.02).abs() < 1e-9);
        assert!((w.c_bat_wear_eur_kwh - 0.03).abs() < 1e-9);
    }

    #[test]
    fn weights_preset_min_ghg() {
        let profile = make_profile();
        let w = build_phase1_weights(&profile, PlannerObjective::MinGhg);
        assert!((w.w_energy - 0.0).abs() < 1e-9);
        assert!((w.w_ghg - 10.0).abs() < 1e-9);
        assert!((w.c_bat_wear_eur_kwh - 0.0).abs() < 1e-9);
    }

    #[test]
    fn weights_preset_custom_uses_fields() {
        let mut profile = make_profile();
        profile.planner.w_energy = 0.5;
        profile.planner.w_ghg = 0.001;
        profile.planner.w_grid = 0.1;
        profile.planner.c_bat_wear_eur_kwh = 0.02;
        let w = build_phase1_weights(&profile, PlannerObjective::Custom);
        assert!((w.w_energy - 0.5).abs() < 1e-9);
        assert!((w.w_ghg - 0.001).abs() < 1e-9);
        assert!((w.w_grid - 0.1).abs() < 1e-9);
        assert!((w.c_bat_wear_eur_kwh - 0.02).abs() < 1e-9);
    }

    #[test]
    fn capacity_event_overrides_grid_limit() {
        // When OadrCapacityState has an active limit, p_imp_max_cont_kw should use it
        let now = fixed_now();
        let profile = make_profile(); // grid.max_import_kw = 25.0
        let sim = SimState::from_profile(&profile);
        let capacity = OadrCapacityState {
            import_limit_kw: Some(5.0), // OpenADR event limit
            export_limit_kw: None,
            import_subscription_kw: None,
            import_reservation_kw: None,
            import_limit_event_id: None,
            export_limit_event_id: None,
            last_updated: None,
        };
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &capacity, &profile, now, None, None, &[], None);
        // Physical limit unchanged
        assert!(inp.p_imp_max_phys_kw.iter().all(|&v| (v - 25.0).abs() < 1e-9));
        // Contractual limit uses the event value
        assert!(inp.p_imp_max_cont_kw.iter().all(|&v| (v - 5.0).abs() < 1e-9));
    }

    // ── Solver tests (run actual HiGHS on synthetic inputs) ──────────────────

    /// Build a minimal MilpInputs with no optional assets.
    fn make_solver_inputs(n: usize, base_kw: f64) -> MilpInputs {
        MilpInputs {
            n,
            dt_h: 1.0,
            c_imp_eur_kwh: vec![0.25; n],
            c_exp_eur_kwh: vec![0.08; n],
            g_imp_kgco2_kwh: vec![0.30; n],
            p_pv_kw: vec![0.0; n],
            p_base_kw: vec![base_kw; n],
            p_imp_max_phys_kw: vec![25.0; n],
            p_exp_max_phys_kw: vec![10.0; n],
            p_imp_max_cont_kw: vec![25.0; n],
            p_exp_max_cont_kw: vec![10.0; n],
            pen_imp_eur_kwh: 0.0,
            pen_exp_eur_kwh: 0.0,
            e_bat_nom_kwh: None,
            e_bat_init_kwh: None,
            e_bat_min_kwh: None,
            e_bat_max_kwh: None,
            p_bat_ch_max_kw: None,
            p_bat_dis_max_kw: None,
            eff_bat_ch: None,
            eff_bat_dis: None,
            a_ev: vec![false; n],
            ev_mode: MilpLoadMode::MustNotRun,
            t_ev_dead_step: None,
            p_ev_max_kw: 0.0,
            p_ev_min_kw: 0.0,
            e_ev_core_kwh: 0.0,
            e_ev_extra_max_kwh: 0.0,
            v_ev_core_eur: 0.0,
            v_ev_extra_eur_kwh: 0.0,
            heater_mode: MilpLoadMode::MustNotRun,
            t_heat_dead_step: None,
            p_heat_mid_kw: 0.0,
            p_heat_full_kw: 0.0,
            e_heat_init_kwh: 0.0,
            e_heat_max_kwh: 0.0,
            q_heat_dem_kw: 0.0,
            e_heat_target_kwh: 0.0,
            lambda_heat_sw_eur: 0.0,
            w_tier_penalty_eur: 0.0,
            heat_initial_z_mid: 0.0,
            heat_initial_z_full: 0.0,
            shiftable_loads: vec![],
        }
    }

    fn make_phase1_weights() -> Phase1Weights {
        Phase1Weights {
            w_energy: 1.0,
            w_ghg: 0.0,
            w_grid: 0.0,
            w_import: 0.0,
            w_viol: 1.0,
            c_bat_wear_eur_kwh: 0.0,
            c_bat_ev_coexist_eur_kwh: 0.0,
            w_services: 1.0,
        }
    }

    fn make_phase2_weights() -> Phase2Weights {
        Phase2Weights {
            c_bat_startup_eur: 0.0,
            c_bat_ramp_eur_kw: 0.0,
            c_ev_startup_eur: 0.0,
            c_ev_ramp_eur_kw: 0.0,
            lambda_heat_sw_eur: 0.0,
            w_tier_penalty_eur: 0.0,
        }
    }

    #[test]
    fn solve_feasible_no_optional_assets() {
        // Minimal case: no battery, no EV, no heater. Import exactly covers base load.
        let inputs = make_solver_inputs(4, 0.5); // base = 0.5 kW
        let result = solve_phase1(&inputs, &make_phase1_weights());
        assert!(result.is_ok(), "solver failed: {:?}", result.err());
        let out = result.unwrap();
        for t in 0..4 {
            assert!(
                (out.p_imp_kw[t] - 0.5).abs() < 1e-3,
                "p_imp[{t}] = {:.4} should be ≈ 0.5",
                out.p_imp_kw[t]
            );
        }
        assert!(out.s_imp_viol_kw.iter().all(|&v| v < 1e-6), "unexpected violations");
    }

    #[test]
    fn solve_ev_must_run_meets_core() {
        // EV MustRun: optimizer must deliver exactly e_ev_core_kwh within deadline.
        let mut inputs = make_solver_inputs(4, 0.0); // no base load
        inputs.a_ev = vec![true; 4];
        inputs.ev_mode = MilpLoadMode::MustRun;
        inputs.t_ev_dead_step = Some(3);
        inputs.p_ev_max_kw = 7.4;
        inputs.p_ev_min_kw = 0.0; // no semi-continuous (cleaner test)
        inputs.e_ev_core_kwh = 4.0;
        inputs.e_ev_extra_max_kwh = 20.0;

        let result = solve_phase1(&inputs, &make_phase1_weights());
        assert!(result.is_ok(), "solver failed: {:?}", result.err());
        let out = result.unwrap();

        let ev_energy: f64 = out.p_ev_kw.iter().sum::<f64>() * inputs.dt_h;
        assert!(
            (ev_energy - 4.0).abs() < 1e-2,
            "EV energy {ev_energy:.4} kWh should be ≈ 4.0 kWh"
        );
    }

    #[test]
    fn solve_battery_arbitrage() {
        // Battery should charge at cheap tariff (t=0,1) and discharge at expensive (t=2,3).
        let mut inputs = make_solver_inputs(4, 1.0); // base = 1.0 kW
        // Cheap then expensive tariff
        inputs.c_imp_eur_kwh = vec![0.10, 0.10, 0.30, 0.30];
        // Add battery: init=0, can hold 5 kWh, eff=1
        inputs.e_bat_nom_kwh = Some(5.0);
        inputs.e_bat_init_kwh = Some(0.0);
        inputs.e_bat_min_kwh = Some(0.0);
        inputs.e_bat_max_kwh = Some(5.0);
        inputs.p_bat_ch_max_kw = Some(5.0);
        inputs.p_bat_dis_max_kw = Some(5.0);
        inputs.eff_bat_ch = Some(1.0);
        inputs.eff_bat_dis = Some(1.0);

        let result = solve_phase1(&inputs, &make_phase1_weights());
        assert!(result.is_ok(), "solver failed: {:?}", result.err());
        let out = result.unwrap();

        // Both charge patterns are degenerate-optimalat 0.40 EUR. Verify objective value only.
        let dt_h = inputs.dt_h;
        let obj: f64 = (0..4)
            .map(|t| {
                inputs.c_imp_eur_kwh[t] * out.p_imp_kw[t] * dt_h
                    - inputs.c_exp_eur_kwh[t] * out.p_exp_kw[t] * dt_h
            })
            .sum();
        assert!(
            (obj - 0.40).abs() < 1e-2,
            "arbitrage objective {obj:.4} EUR should be ≈ 0.40 EUR (charge cheap, discharge expensive)"
        );
        // Battery must discharge in expensive window (at least 1 kWh)
        let dis_in_expensive = out.p_bat_dis_kw[2] + out.p_bat_dis_kw[3];
        assert!(
            dis_in_expensive > 0.5,
            "battery should discharge in expensive period, got {dis_in_expensive:.4} kWh"
        );
    }

    #[test]
    fn ev_startup_penalty_produces_contiguous_block() {
        // Flat tariff across 6 slots → degenerate without penalty.
        // With a high startup cost the solver must consolidate EV charging into one run.
        // p_ev_min_kw > 0 makes the semi-continuous constraint bind: z_ev_on=1 forces p_ev >= min,
        // so the solver cannot trivially keep z_ev_on=1 everywhere at zero charging cost.
        let n = 6;
        let mut inputs = make_solver_inputs(n, 0.0);
        inputs.a_ev = vec![true; n];
        inputs.ev_mode = MilpLoadMode::MustRun;
        inputs.t_ev_dead_step = Some(n - 1);
        inputs.p_ev_max_kw = 7.4;
        inputs.p_ev_min_kw = 1.4; // semi-continuous: z_ev_on=1 forces p_ev >= 1.4
        inputs.e_ev_core_kwh = 3.0 * 7.4; // needs 3 full slots at 1 h each

        let mut weights = make_phase2_weights();
        weights.c_ev_startup_eur = 0.5; // high penalty — one startup costs 0.5 EUR

        let out = solve_milp_two_phase(&inputs, &make_phase1_weights(), &weights, 1.0)
            .expect("solver failed").0;

        // Identify active slots (z_ev_on > 0.5 means EV charging committed)
        let active: Vec<bool> = out.z_ev_on.iter().map(|&v| v > 0.5).collect();
        // Count off→on switches; with startup penalty, expect at most 1 (contiguous block).
        // Starting at slot 0 (0 startups) is also a valid contiguous block.
        let startups = active.windows(2).filter(|w| !w[0] && w[1]).count();
        assert!(startups <= 1, "expected at most 1 EV startup (contiguous block), got {startups}; active={active:?}");
    }

    #[test]
    fn battery_startup_penalty_minimises_active_restarts() {
        // 6 slots with cheap→expensive tariff pattern.
        // Without penalty: solver may fragment battery into scattered charge/discharge bursts.
        // With high startup penalty: battery should activate in contiguous blocks (≤2 startups:
        // one for charging, one for discharging).
        let n = 6;
        let mut inputs = make_solver_inputs(n, 0.0);
        inputs.c_imp_eur_kwh = vec![0.10, 0.10, 0.10, 0.30, 0.30, 0.30];
        inputs.e_bat_nom_kwh = Some(6.0);
        inputs.e_bat_init_kwh = Some(3.0);
        inputs.e_bat_min_kwh = Some(0.6);
        inputs.e_bat_max_kwh = Some(6.0);
        inputs.p_bat_ch_max_kw = Some(2.0);
        inputs.p_bat_dis_max_kw = Some(2.0);
        inputs.eff_bat_ch = Some(1.0);
        inputs.eff_bat_dis = Some(1.0);

        let mut weights = make_phase2_weights();
        weights.c_bat_startup_eur = 0.5; // high penalty

        let out = solve_milp_two_phase(&inputs, &make_phase1_weights(), &weights, 1.0)
            .expect("solver failed").0;

        // Count idle→active transitions (mirrors EV startup test logic)
        let active: Vec<bool> = (0..n)
            .map(|t| out.p_bat_ch_kw[t] > 1e-3 || out.p_bat_dis_kw[t] > 1e-3)
            .collect();
        let startups = active.windows(2).filter(|w| !w[0] && w[1]).count()
            + if active[0] { 1 } else { 0 }; // count slot-0 active as a startup
        assert!(
            startups <= 2,
            "expected ≤2 battery startups (charge block + discharge block), got {startups}; active={active:?} ch={:?} dis={:?}",
            out.p_bat_ch_kw, out.p_bat_dis_kw,
        );
    }

    #[test]
    fn solve_power_balance_holds() {
        // For every step the power balance constraint must hold in the solution.
        let mut inputs = make_solver_inputs(4, 1.5);
        inputs.p_pv_kw = vec![2.0; 4]; // PV exceeds base, forces export
        // Add battery so there are non-trivial flows to check
        inputs.e_bat_nom_kwh = Some(5.0);
        inputs.e_bat_init_kwh = Some(2.5);
        inputs.e_bat_min_kwh = Some(0.5);
        inputs.e_bat_max_kwh = Some(5.0);
        inputs.p_bat_ch_max_kw = Some(3.0);
        inputs.p_bat_dis_max_kw = Some(3.0);
        inputs.eff_bat_ch = Some(1.0);
        inputs.eff_bat_dis = Some(1.0);

        let out = solve_phase1(&inputs, &make_phase1_weights()).expect("solver failed");

        for t in 0..4 {
            // p_imp + p_pv + p_bat_dis = p_base + p_bat_ch + p_exp (EV=0, heater=0)
            let residual = out.p_imp_kw[t] + inputs.p_pv_kw[t] + out.p_bat_dis_kw[t]
                - inputs.p_base_kw[t]
                - out.p_bat_ch_kw[t]
                - out.p_exp_kw[t];
            assert!(
                residual.abs() < 1e-4,
                "power balance violated at t={t}: residual={residual:.6}"
            );
        }
    }

    #[test]
    fn ev_ramp_penalty_produces_flat_charging_power() {
        // 6 slots, flat tariff → solver is indifferent between e.g. [7.4,1.4,7.4,…] and [4.0,4.0,…].
        // High ramp penalty forces the solver to keep p_ev constant across slots.
        let n = 6;
        let mut inputs = make_solver_inputs(n, 0.0);
        inputs.a_ev = vec![true; n];
        inputs.ev_mode = MilpLoadMode::MustRun;
        inputs.t_ev_dead_step = Some(n - 1);
        inputs.p_ev_max_kw = 7.4;
        inputs.p_ev_min_kw = 1.4;
        inputs.e_ev_core_kwh = 3.0 * 7.4; // needs ~3 full slots at max

        let mut weights = make_phase2_weights();
        weights.c_ev_startup_eur = 0.5; // also penalise startups so EV is one block
        weights.c_ev_ramp_eur_kw = 1.0; // 1 EUR per kW change — very high

        let out = solve_milp_two_phase(&inputs, &make_phase1_weights(), &weights, 1.0)
            .expect("solver failed").0;

        let active_power: Vec<f64> = out.p_ev_kw.iter().copied().filter(|&v| v > 0.05).collect();
        // All active slots must have the same power (within 0.1 kW rounding)
        if active_power.len() > 1 {
            let first = active_power[0];
            for &p in &active_power[1..] {
                assert!(
                    (p - first).abs() < 0.1,
                    "EV power varies across active slots: {active_power:?}"
                );
            }
        }
    }

    #[test]
    fn battery_ramp_penalty_produces_smooth_power() {
        // 6 slots, cheap→expensive tariff. Battery should charge in cheap slots, discharge
        // in expensive slots. With high ramp penalty the solver should keep charge and
        // discharge power levels constant across their respective blocks.
        let n = 6;
        let mut inputs = make_solver_inputs(n, 1.0); // 1 kW base load
        inputs.c_imp_eur_kwh = vec![0.08, 0.08, 0.08, 0.30, 0.30, 0.30];
        inputs.e_bat_nom_kwh = Some(6.0);
        inputs.e_bat_init_kwh = Some(3.0);
        inputs.e_bat_min_kwh = Some(0.6);
        inputs.e_bat_max_kwh = Some(6.0);
        inputs.p_bat_ch_max_kw = Some(3.0);
        inputs.p_bat_dis_max_kw = Some(3.0);
        inputs.eff_bat_ch = Some(1.0);
        inputs.eff_bat_dis = Some(1.0);

        let mut weights = make_phase2_weights();
        weights.c_bat_startup_eur = 0.5;   // keep blocks contiguous
        weights.c_bat_ramp_eur_kw = 1.0;   // very high — force flat power

        let out = solve_milp_two_phase(&inputs, &make_phase1_weights(), &weights, 1.0)
            .expect("solver failed").0;

        // Check charging slots are at uniform power
        let ch_power: Vec<f64> = out.p_bat_ch_kw.iter().copied().filter(|&v| v > 0.05).collect();
        if ch_power.len() > 1 {
            let first = ch_power[0];
            for &p in &ch_power[1..] {
                assert!(
                    (p - first).abs() < 0.15,
                    "battery charge power varies across active slots: {ch_power:?}"
                );
            }
        }
        // Check discharging slots are at uniform power
        let dis_power: Vec<f64> = out.p_bat_dis_kw.iter().copied().filter(|&v| v > 0.05).collect();
        if dis_power.len() > 1 {
            let first = dis_power[0];
            for &p in &dis_power[1..] {
                assert!(
                    (p - first).abs() < 0.15,
                    "battery discharge power varies across active slots: {dis_power:?}"
                );
            }
        }
    }

    #[test]
    fn battery_does_not_discharge_during_ev_charging_with_pv_surplus() {
        // 4 slots, flat tariff, PV surplus exceeds ev_min in every slot.
        // Battery has stored energy. EV must charge.
        // High c_bat_ev_coexist → battery should not discharge during EV-on slots.
        let n = 4;
        let mut inputs = make_solver_inputs(n, 0.5);
        inputs.p_pv_kw = vec![5.0; n]; // surplus = 5.0 - 0.5 = 4.5 kW ≥ ev_min

        inputs.e_bat_nom_kwh = Some(10.0);
        inputs.e_bat_init_kwh = Some(8.0);
        inputs.e_bat_min_kwh = Some(0.0);
        inputs.e_bat_max_kwh = Some(10.0);
        inputs.p_bat_ch_max_kw = Some(3.0);
        inputs.p_bat_dis_max_kw = Some(3.0);
        inputs.eff_bat_ch = Some(1.0);
        inputs.eff_bat_dis = Some(1.0);

        inputs.ev_mode = MilpLoadMode::MustRun;
        inputs.a_ev = vec![true; n];
        inputs.t_ev_dead_step = Some(n - 1);
        inputs.p_ev_max_kw = 7.4;
        inputs.p_ev_min_kw = 1.4;
        inputs.e_ev_core_kwh = 4.0 * 1.4; // 5.6 kWh — easily met by PV alone

        let out = solve_phase1(&inputs, &Phase1Weights { c_bat_ev_coexist_eur_kwh: 10.0, ..make_phase1_weights() }).expect("solver failed");

        for t in 0..n {
            if out.z_ev_on[t] > 0.5 {
                assert!(
                    out.p_bat_dis_kw[t] < 0.1,
                    "slot {t}: battery discharged {:.3} kW while EV charging with PV surplus",
                    out.p_bat_dis_kw[t]
                );
            }
        }
    }

    // ── PV forecast reflects live irradiance_offset and pv_alpha ─────────────

    /// Return midnight so natural_irradiance_at() = 0, isolating the offset term.
    fn fixed_midnight() -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 4, 12, 0, 0, 0).unwrap()
    }

    /// Set irradiance_offset and pv_alpha on the PV asset in an existing SimState.
    fn set_pv_inject(sim: &mut SimState, offset: f64, alpha: f64) {
        if let Some((_, cfg)) = sim.find_asset_mut("pv") {
            if let AssetConfig::Pv(pv) = cfg {
                pv.irradiance_offset = offset;
                pv.pv_alpha = alpha;
            } else {
                panic!("pv asset has unexpected config type");
            }
        } else {
            panic!("no pv asset in sim");
        }
    }

    #[test]
    fn pv_irradiance_offset_in_forecast() {
        // Regression: irradiance_offset must project into p_pv_kw.
        // At midnight, natural irradiance = 0. With offset=0.5 and very slow
        // alpha (≈no decay over the horizon), slot 0 must be ≈ 0.5 × rated_kw.
        let now = fixed_midnight();
        let profile = make_profile(); // rated_kw=5.0
        let mut sim = SimState::from_profile(&profile);
        set_pv_inject(&mut sim, 0.5, 0.001); // slow alpha → offset barely decays

        let inp = build_milp_inputs(
            &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(),
            &profile, now, None, None, &[], None,
        );

        // slot 0: seconds_ahead=0 → decayed_offset = 0.5×(0.999)^0 = 0.5
        // p_pv[0] = (0.0 + 0.5).clamp(0,1) × 5.0 = 2.5 kW
        assert!(
            inp.p_pv_kw[0] > 1.0,
            "p_pv_kw[0] should reflect irradiance_offset (got {:.4})",
            inp.p_pv_kw[0]
        );
    }

    #[test]
    fn pv_irradiance_offset_decays_per_step_not_per_second() {
        // Regression guard: with alpha=0.1 (typical), the decay exponent must be
        // the plan-step count (t), NOT raw seconds (t * 300).
        // Buggy formula: 0.9^(1×300) ≈ 5e-14  → slot 1 ≈ 0 kW  (WRONG)
        // Correct formula: 0.9^1 = 0.9         → slot 1 ≈ 2.25 kW (RIGHT)
        let now = fixed_midnight(); // natural=0, isolates offset
        let profile = make_profile(); // rated_kw=5.0, step_s=300
        let mut sim = SimState::from_profile(&profile);
        set_pv_inject(&mut sim, 0.5, 0.1); // typical alpha=0.1

        let inp = build_milp_inputs(
            &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(),
            &profile, now, None, None, &[], None,
        );

        // slot 0: 0.5 × 0.9^0 × 5.0 = 2.5 kW
        // slot 1: 0.5 × 0.9^1 × 5.0 = 2.25 kW (must be clearly non-zero)
        assert!(
            inp.p_pv_kw[1] > 1.0,
            "slot 1 must retain offset with alpha=0.1 (decay per step, not per second), got {:.6}",
            inp.p_pv_kw[1]
        );
        // slot 5: 0.5 × 0.9^5 × 5.0 ≈ 1.476 kW
        assert!(
            inp.p_pv_kw[5] > 0.5,
            "slot 5 must still show partial offset, got {:.6}",
            inp.p_pv_kw[5]
        );
        // Decay is monotonically decreasing (offset fades over horizon)
        assert!(
            inp.p_pv_kw[1] < inp.p_pv_kw[0],
            "slot 1 must be less than slot 0 (offset decaying)"
        );
    }

    #[test]
    fn pv_alpha_faster_decay_in_forecast() {
        // Regression: higher pv_alpha (blend-back speed) must produce lower p_pv_kw
        // at later forecast slots because the offset decays faster.
        // At midnight natural=0, so all forecast power comes from the decaying offset.
        let now = fixed_midnight();
        let profile = make_profile(); // rated_kw=5.0, step_s=300s, 24 slots

        let mut sim_slow = SimState::from_profile(&profile);
        set_pv_inject(&mut sim_slow, 0.5, 0.001); // slow: 0.1 % per second

        let mut sim_fast = SimState::from_profile(&profile);
        set_pv_inject(&mut sim_fast, 0.5, 0.05); // fast: 5 % per second

        let inp_slow = build_milp_inputs(
            &sim_slow, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(),
            &profile, now, None, None, &[], None,
        );
        let inp_fast = build_milp_inputs(
            &sim_fast, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(),
            &profile, now, None, None, &[], None,
        );

        // At slot 3 (900 s ahead at midnight, natural=0):
        //   slow: 0.5 × (0.999)^900 ≈ 0.5 × 0.41 ≈ 2.0 kW
        //   fast: 0.5 × (0.95)^900  ≈ 0.5 × ~0   ≈ 0.0 kW
        let t = 3;
        assert!(
            inp_fast.p_pv_kw[t] < inp_slow.p_pv_kw[t],
            "higher alpha should produce lower p_pv_kw at later slots: \
             fast={:.4} >= slow={:.4}",
            inp_fast.p_pv_kw[t],
            inp_slow.p_pv_kw[t]
        );
    }

    #[test]
    fn pv_zero_offset_matches_sin_model() {
        // Backward compat: when irradiance_offset=0, p_pv_kw must equal the
        // profile's pure sin model (PvConfig::forecast_kw).
        let now = fixed_now(); // 06:00 → natural = 0 at slot 0
        let profile = make_profile(); // rated_kw=5.0, step_s=300s

        // from_profile initialises irradiance_offset=0, pv_alpha=0.1
        let sim = SimState::from_profile(&profile);

        let inp = build_milp_inputs(
            &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(),
            &profile, now, None, None, &[], None,
        );

        // Compare every slot against the profile's sin model
        let pv_cfg = profile.pv_config().unwrap();
        for t in 0..inp.n {
            let slot_t = now + Duration::seconds(t as i64 * 300);
            let expected = pv_cfg.forecast_kw(slot_t);
            assert!(
                (inp.p_pv_kw[t] - expected).abs() < 1e-9,
                "slot {t}: zero-offset p_pv_kw should match sin model \
                 (got {:.6}, expected {:.6})",
                inp.p_pv_kw[t],
                expected
            );
        }
    }

    // ── run_planner() regression-guard tests ─────────────────────────────────
    // These call only the public run_planner() entry point so they remain valid
    // through any internal refactor. Use 1800s steps (4 slots, 2h horizon) for
    // fast solver runs (~<100ms each).

    fn make_profile_1800s() -> Profile {
        let mut p = make_profile();
        p.planner.plan_step_s = 1800;
        p
    }

    fn make_two_zone_tariffs(imp_cheap: f64, imp_exp: f64) -> TariffTimeSeries {
        let now = fixed_now();
        TariffTimeSeries::from_snapshots(&[
            TariffSnapshot {
                interval_start: now - Duration::hours(1),
                interval_end: now + Duration::hours(1),
                import_tariff_eur_kwh: Some(imp_cheap),
                export_tariff_eur_kwh: Some(0.08),
                co2_g_kwh: Some(300.0),
            },
            TariffSnapshot {
                interval_start: now + Duration::hours(1),
                interval_end: now + Duration::hours(3),
                import_tariff_eur_kwh: Some(imp_exp),
                export_tariff_eur_kwh: Some(0.08),
                co2_g_kwh: Some(300.0),
            },
        ])
    }

    #[test]
    fn run_planner_no_assets_covers_base_load() {
        let now = fixed_now();
        let profile = Profile {
            devices: Default::default(),
            assets: vec![AssetProfile::BaseLoad(BaseLoadConfig {
                id: "base_load".into(),
                baseline_kw: 1.0,
            })],
            simulator: SimulatorConfig::default(),
            planner: PlannerConfig { plan_step_s: 1800, plan_horizon_h: 2, ..PlannerConfig::default() },
            grid: crate::profile::GridConfig { max_import_kw: 25.0, max_export_kw: 10.0 },
            packets: vec![],
        };
        let sim = SimState::from_profile(&profile);
        let plan = run_planner(
            &sim, &make_tariffs(0.25, 0.08, 300.0), &no_capacity(), &profile, now,
            crate::entities::asset::PlanTrigger::Periodic, None, None, &[], None, None,
        );
        assert_eq!(plan.slots.len(), 4);
        for slot in &plan.slots {
            assert!((slot.net_import_kw - 1.0).abs() < 0.05,
                "expected net_import ≈ 1.0 kW, got {:.4}", slot.net_import_kw);
            assert!(slot.bat_charge_kw < 1e-3);
            assert!(slot.bat_discharge_kw < 1e-3);
            assert!(!slot.allocations.iter().any(|a| a.asset_id == "ev"));
        }
    }

    #[test]
    fn run_planner_battery_absent_no_bat_allocation() {
        let now = fixed_now();
        let mut profile = make_profile_1800s();
        profile.assets.retain(|a| !matches!(a, AssetProfile::Battery(_)));
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let session = crate::entities::device_session::EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.8,
            departure_time: now + Duration::hours(2),
            soft_deadline: false,
            created_at: now,
            updated_at: now,
        };
        let plan = run_planner(
            &sim, &make_tariffs(0.25, 0.08, 300.0), &no_capacity(), &profile, now,
            crate::entities::asset::PlanTrigger::Periodic, Some(&session), None, &[], None, None,
        );
        for slot in &plan.slots {
            assert!(slot.bat_charge_kw < 1e-3,
                "battery absent → bat_charge_kw=0, got {:.4}", slot.bat_charge_kw);
            assert!(slot.bat_discharge_kw < 1e-3,
                "battery absent → bat_discharge_kw=0, got {:.4}", slot.bat_discharge_kw);
        }
        assert!(plan.soc_trajectory_kwh.is_empty() || plan.soc_trajectory_kwh.iter().all(|&v| v < 1e-3),
            "no battery → soc_trajectory_kwh empty or all-zero");
    }

    #[test]
    fn run_planner_ev_absent_no_ev_allocation() {
        let now = fixed_now();
        let mut profile = make_profile_1800s();
        profile.assets.retain(|a| !matches!(a, AssetProfile::Ev(_)));
        let sim = SimState::from_profile(&profile);
        let plan = run_planner(
            &sim, &make_tariffs(0.25, 0.08, 300.0), &no_capacity(), &profile, now,
            crate::entities::asset::PlanTrigger::Periodic, None, None, &[], None, None,
        );
        for slot in &plan.slots {
            assert!(!slot.allocations.iter().any(|a| a.asset_id == "ev"),
                "EV absent → no EV allocation");
        }
    }

    #[test]
    fn run_planner_battery_charges_on_cheap_tariff() {
        let now = fixed_now();
        let mut profile = make_profile_1800s();
        profile.assets.retain(|a| matches!(a, AssetProfile::Battery(_) | AssetProfile::BaseLoad(_)));
        let mut sim = SimState::from_profile(&profile);
        set_battery_soc(&mut sim, 0.1); // low SoC → wants to charge
        let plan = run_planner(
            &sim, &make_two_zone_tariffs(0.05, 0.40), &no_capacity(), &profile, now,
            crate::entities::asset::PlanTrigger::Periodic, None, None, &[], None, None,
        );
        let cheap_charge: f64 = plan.slots[0..2].iter().map(|s| s.bat_charge_kw).sum();
        let exp_dis: f64 = plan.slots[2..4].iter().map(|s| s.bat_discharge_kw).sum();
        assert!(
            cheap_charge > 0.1 || exp_dis > 0.1,
            "expected charging in cheap slots or discharging in expensive slots; \
             cheap_charge={:.3}, exp_dis={:.3}", cheap_charge, exp_dis
        );
    }

    #[test]
    fn run_planner_ev_must_run_energy_met() {
        let now = fixed_now();
        let mut profile = make_profile_1800s();
        profile.assets.retain(|a| !matches!(a, AssetProfile::Heater(_) | AssetProfile::Pv(_)));
        // Shrink EV battery so 7 kWh is feasible in 2h at 7.4 kW
        profile.assets = profile.assets.into_iter().map(|a| match a {
            AssetProfile::Ev(mut ev) => { ev.battery_kwh = 10.0; AssetProfile::Ev(ev) }
            other => other,
        }).collect();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        for entry in &mut sim.assets {
            if let AssetState::Ev(ref mut ev) = entry.state { ev.soc = 0.1; }
        }
        let e_core_kwh = (0.8 - 0.1) * 10.0; // 7.0 kWh
        let session = crate::entities::device_session::EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.8,
            departure_time: now + Duration::hours(2),
            soft_deadline: false,
            created_at: now,
            updated_at: now,
        };
        let plan = run_planner(
            &sim, &make_tariffs(0.25, 0.08, 300.0), &no_capacity(), &profile, now,
            crate::entities::asset::PlanTrigger::Periodic, Some(&session), None, &[], None, None,
        );
        let dt_h = 1800.0 / 3600.0;
        let ev_energy: f64 = plan.slots.iter()
            .map(|s| s.planned_kw_by_asset.get("ev").copied().unwrap_or(0.0) * dt_h)
            .sum();
        assert!(ev_energy >= e_core_kwh - 0.1,
            "MustRun EV should meet {:.1} kWh core, got {:.4}", e_core_kwh, ev_energy);
    }

    #[test]
    fn run_planner_power_balance_invariant() {
        // No EV session — avoids infeasibility from large battery/target gap in make_profile.
        // Battery + heater + PV (MayRun) is sufficient to exercise the balance.
        let now = fixed_now();
        let profile = make_profile_1800s();
        let mut sim = SimState::from_profile(&profile);
        set_battery_soc(&mut sim, 0.5);
        let plan = run_planner(
            &sim, &make_two_zone_tariffs(0.05, 0.40), &no_capacity(), &profile, now,
            crate::entities::asset::PlanTrigger::Periodic, None, None, &[], None, None,
        );
        for (t, slot) in plan.slots.iter().enumerate() {
            let ev_kw = slot.planned_kw_by_asset.get("ev").copied().unwrap_or(0.0);
            let heat_kw = slot.planned_kw_by_asset.get("heater").copied().unwrap_or(0.0);
            // p_imp + p_pv + p_dis = p_base + p_ev + p_heat + p_ch + p_exp
            let lhs = slot.net_import_kw + slot.pv_forecast_kw + slot.bat_discharge_kw;
            let rhs = slot.baseline_kw + ev_kw + heat_kw + slot.bat_charge_kw + slot.net_export_kw;
            assert!((lhs - rhs).abs() < 0.1,
                "power balance violated at slot {t}: lhs={:.4} rhs={:.4}", lhs, rhs);
        }
    }

    #[test]
    fn run_planner_absent_battery_no_panic() {
        let now = fixed_now();
        let mut profile = make_profile_1800s();
        profile.assets.retain(|a| !matches!(a, AssetProfile::Battery(_)));
        let sim = SimState::from_profile(&profile);
        let plan = run_planner(
            &sim, &make_tariffs(0.25, 0.08, 300.0), &no_capacity(), &profile, now,
            crate::entities::asset::PlanTrigger::Periodic, None, None, &[], None, None,
        );
        assert_eq!(plan.slots.len(), 4, "plan must have 4 slots");
        assert!(
            plan.soc_trajectory_kwh.is_empty() || plan.soc_trajectory_kwh.iter().all(|&v| v < 1e-3),
            "no battery → soc_trajectory_kwh empty or all-zero"
        );
    }

    // ── Heater trajectory model unit tests ────────────────────────────────────

    #[test]
    fn heater_inputs_e_init_positive_above_min() {
        // volume_l=200 → thermal_mass = 200×4.186/3600 ≈ 0.23256 kWh/°C
        // T_current=60, T_min=40 → e_init = (60−40) × 0.23256 ≈ 4.65 kWh
        let now = fixed_now();
        let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        let expected = 20.0 * 200.0 * 4.186 / 3600.0;
        assert!((inp.e_heat_init_kwh - expected).abs() < 0.01,
            "e_init={:.4} expected≈{:.4}", inp.e_heat_init_kwh, expected);
    }

    #[test]
    fn heater_inputs_e_init_negative_below_min() {
        // T_current=35 < T_min=40 → e_init < 0
        let now = fixed_now();
        let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 40.0);
        let mut sim = SimState::from_profile(&profile);
        set_heater_temp(&mut sim, 35.0);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert!(inp.e_heat_init_kwh < 0.0,
            "e_init {} should be negative when temp < T_min", inp.e_heat_init_kwh);
    }

    #[test]
    fn heater_inputs_e_max_formula() {
        // e_max = (T_max − T_min) × thermal_mass = (80−40) × 200×4.186/3600 ≈ 9.30 kWh
        let now = fixed_now();
        let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 40.0);
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        let expected = 40.0 * 200.0 * 4.186 / 3600.0;
        assert!((inp.e_heat_max_kwh - expected).abs() < 0.01,
            "e_max={:.4} expected≈{:.4}", inp.e_heat_max_kwh, expected);
    }

    #[test]
    fn heater_inputs_q_dem_scalar() {
        // q_dem = draw_kw + k_loss × ((T_min+T_max)/2 − ambient)
        // With defaults: draw=0, k_loss=0.1, t_mid=(40+80)/2=60, ambient=10
        // → q_dem = 0 + 0.1 × (60−10) = 5.0 kW
        let now = fixed_now();
        let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert!((inp.q_heat_dem_kw - 5.0).abs() < 0.01,
            "q_dem={:.4} expected 5.0", inp.q_heat_dem_kw);
    }

    #[test]
    fn heater_inputs_e_target_from_heater_target() {
        // e_target = (target_temp_c − T_min) × thermal_mass, clamped to [0, e_max]
        // target=70, T_min=40 → (70−40) × 200×4.186/3600 ≈ 6.98 kWh
        let now = fixed_now();
        let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
        let sim = SimState::from_profile(&profile);
        let target = HeaterTarget {
            id: uuid::Uuid::new_v4(),
            target_temp_c: 70.0,
            ready_by: now + Duration::hours(1),
            created_at: now,
            updated_at: now,
        };
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, Some(&target), &[], None);
        let expected = 30.0 * 200.0 * 4.186 / 3600.0;
        assert!((inp.e_heat_target_kwh - expected).abs() < 0.01,
            "e_target={:.4} expected≈{:.4}", inp.e_heat_target_kwh, expected);
    }

    #[test]
    fn heater_inputs_autonomous_e_target_is_e_max() {
        // Without HeaterTarget, e_heat_target_kwh == e_heat_max_kwh
        let now = fixed_now();
        let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert!((inp.e_heat_target_kwh - inp.e_heat_max_kwh).abs() < 1e-9,
            "autonomous: e_target {} should equal e_max {}", inp.e_heat_target_kwh, inp.e_heat_max_kwh);
    }

    #[test]
    fn heater_inputs_autonomous_mode_is_may_run() {
        // Without HeaterTarget, heater_mode == MilpLoadMode::MayRun
        let now = fixed_now();
        let profile = make_heater_only_profile(Some(200.0), 40.0, 80.0, 60.0);
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert_eq!(inp.heater_mode, MilpLoadMode::MayRun);
    }

    #[test]
    fn heater_inputs_switching_penalty_defaults() {
        // HeaterConfig with no switching_penalty_eur → lambda_heat_sw_eur == 0.01
        let now = fixed_now();
        let profile = make_profile(); // heater has no switching_penalty_eur set
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), &profile, now, None, None, &[], None);
        assert!((inp.lambda_heat_sw_eur - 0.01).abs() < 1e-9,
            "lambda_sw={} expected 0.01", inp.lambda_heat_sw_eur);
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_dynamics_respected() {
        // After solve: e_tank[t+1] ≈ e_tank[t] + (p_heat[t] − q_dem) × dt_h ± 1e-3
        todo!("implement after solve_milp() heater trajectory integration")
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_must_run_meets_e_target() {
        // MustRun with deadline: e_tank[t_dead] ≥ e_target − 1e-3
        todo!("implement after solve_milp() heater trajectory integration")
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_soft_low_positive_when_below_min() {
        // e_init < 0: s_low[0] > 0 in solution
        todo!("implement after solve_milp() heater trajectory integration")
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_switching_reduces_with_penalty() {
        // High lambda_sw → fewer mode changes than lambda_sw = 0
        todo!("implement after solve_milp() heater trajectory integration")
    }

    #[test]
    #[ignore = "implemented in Step 5"]
    fn solve_heater_upper_bound_not_exceeded() {
        // e_tank[t] ≤ e_max + 1e-6 for all t in solution
        todo!("implement after solve_milp() heater trajectory integration")
    }
}
