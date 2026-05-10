#![allow(dead_code)] // types used by milp_planner submodules via use super::*
use chrono::{DateTime, Utc};

use crate::profile::{PlannerObjective, Profile};

// ── Internal MILP types ──────────────────────────────────────────────────────

/// Internal MILP load mode for an asset (EV / heater).
/// Derived from the presence of an active device session (EvSession / HeaterTarget).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MilpLoadMode {
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
pub(crate) struct Phase1Weights {
    /// Scales C_energy = Σ(c_imp·p_imp − c_exp·p_exp)·Δt
    pub(crate) w_energy: f64,
    /// Monetises GHG emissions [€/kgCO₂]
    pub(crate) w_ghg: f64,
    /// Penalty per kWh of total grid exchange (import + export)
    pub(crate) w_grid: f64,
    /// Penalty per kWh of grid import only (autarky objective)
    pub(crate) w_import: f64,
    /// Scales contractual violation penalties
    pub(crate) w_viol: f64,
    /// Battery cycling wear cost [€/kWh charged or discharged]
    pub(crate) c_bat_wear_eur_kwh: f64,
    /// Penalty per kWh of battery discharge co-occurring with EV charging when
    /// PV surplus ≥ ev_min_kw [€/kWh]. 0.0 = disabled.
    pub(crate) c_bat_ev_coexist_eur_kwh: f64,
    /// Scales service reward terms; always 1.0 until grid-services are modelled
    pub(crate) w_services: f64,
}

/// Phase 2 objective coefficients (operational friction). Used only when
/// `phase2_epsilon_eur > 0.0`. Phase 2 minimises startup/ramp/switching/tier
/// cost subject to a Phase 1 cost cap.
#[derive(Debug, Clone)]
pub(crate) struct Phase2Weights {
    /// Penalty per battery charge/discharge mode transition [€/transition]
    pub(crate) c_bat_startup_eur: f64,
    /// Penalty per kW of battery net-power change between consecutive slots [€/kW]
    pub(crate) c_bat_ramp_eur_kw: f64,
    /// Penalty per EV charging run startup [€/run]
    pub(crate) c_ev_startup_eur: f64,
    /// Penalty per kW of EV power change between consecutive slots [€/kW]
    pub(crate) c_ev_ramp_eur_kw: f64,
    /// Heater relay switching penalty [EUR/switch event]
    pub(crate) lambda_heat_sw_eur: f64,
    /// Soft penalty per slot for using the full power tier over mid tier [€/slot]
    pub(crate) w_tier_penalty_eur: f64,
}

/// Fully-resolved MILP input parameters for one planning cycle.
/// Created by `build_milp_inputs()`; consumed by `solve_milp_two_phase()` (Phase 3).
/// All per-step `Vec<f64>` fields have length `n`.
#[derive(Debug, Clone)]
pub(crate) struct MilpInputs {
    /// Number of planning steps.
    pub(crate) n: usize,
    /// Step size in hours (e.g. 300 s → 1/12 h).
    pub(crate) dt_h: f64,

    // ── Grid (per-step arrays, len = n) ──────────────────────────────────────
    /// Import tariff [€/kWh]
    pub(crate) c_imp_eur_kwh: Vec<f64>,
    /// Export tariff [€/kWh]
    pub(crate) c_exp_eur_kwh: Vec<f64>,
    /// Grid CO₂ intensity [kgCO₂/kWh] (÷1000 from stored g/kWh)
    pub(crate) g_imp_kgco2_kwh: Vec<f64>,
    /// PV generation forecast [kW]
    pub(crate) p_pv_kw: Vec<f64>,
    /// Non-controllable baseline load [kW]
    pub(crate) p_base_kw: Vec<f64>,
    /// Physical import limit at meter/breaker [kW]
    pub(crate) p_imp_max_phys_kw: Vec<f64>,
    /// Physical export limit [kW]
    pub(crate) p_exp_max_phys_kw: Vec<f64>,
    /// Contractual import limit [kW] (OpenADR event limit or physical when no event)
    pub(crate) p_imp_max_cont_kw: Vec<f64>,
    /// Contractual export limit [kW]
    pub(crate) p_exp_max_cont_kw: Vec<f64>,
    /// Per-kWh import violation penalty (scalar, from PlannerConfig)
    pub(crate) pen_imp_eur_kwh: f64,
    /// Per-kWh export violation penalty (scalar)
    pub(crate) pen_exp_eur_kwh: f64,

    // ── Battery (None when no battery asset present in profile) ──────────────
    /// Nameplate capacity [kWh]
    pub(crate) e_bat_nom_kwh: Option<f64>,
    /// Initial SoC energy at call time [kWh]. Uses LIVE SimState SoC, NOT profile initial_soc.
    pub(crate) e_bat_init_kwh: Option<f64>,
    /// Operational lower bound = min_soc × capacity [kWh]
    pub(crate) e_bat_min_kwh: Option<f64>,
    /// Operational upper bound = capacity (no upper SoC cap today) [kWh]
    pub(crate) e_bat_max_kwh: Option<f64>,
    /// Max charge power [kW]
    pub(crate) p_bat_ch_max_kw: Option<f64>,
    /// Max discharge power [kW]
    pub(crate) p_bat_dis_max_kw: Option<f64>,
    /// One-way charge efficiency = √(round_trip_efficiency)
    pub(crate) eff_bat_ch: Option<f64>,
    /// One-way discharge efficiency = √(round_trip_efficiency)
    pub(crate) eff_bat_dis: Option<f64>,

    // ── EV (MustNotRun when EV absent or unplugged) ───────────────────────────
    /// Per-step plugged-in availability mask. False forces p_ev[t] = 0.
    pub(crate) a_ev: Vec<bool>,
    pub(crate) ev_mode: MilpLoadMode,
    /// Last step index that counts toward the EV energy sum.
    /// None = open horizon (plugged, no active packet with a deadline).
    pub(crate) t_ev_dead_step: Option<usize>,
    /// Max charge power [kW]; 0.0 when EV absent
    pub(crate) p_ev_max_kw: f64,
    /// Semi-continuous minimum charge power [kW] (EvConfig.min_charge_kw)
    pub(crate) p_ev_min_kw: f64,
    /// Core energy requirement [kWh] from active packet; 0.0 when absent
    pub(crate) e_ev_core_kwh: f64,
    /// Opportunistic headroom = battery_kwh × (1 − soc_target) [kWh]
    pub(crate) e_ev_extra_max_kwh: f64,
    /// Reward for meeting core target (MayRun only); hardcoded 0.0 until user-requests integration
    pub(crate) v_ev_core_eur: f64,
    /// Reward per kWh of extra opportunistic charging [€/kWh]
    pub(crate) v_ev_extra_eur_kwh: f64,

    // ── Heater (MustNotRun when heater absent) ────────────────────────────────
    pub(crate) heater_mode: MilpLoadMode,
    /// Deadline step index. None = no hard deadline (autonomous MayRun path).
    pub(crate) t_heat_dead_step: Option<usize>,
    /// Mid power level [kW] = mid_kw.unwrap_or(max_kw / 2.0)
    pub(crate) p_heat_mid_kw: f64,
    /// Full power level [kW] = max_kw
    pub(crate) p_heat_full_kw: f64,
    /// Initial tank energy above T_min [kWh]. May be negative when tank is below T_min.
    pub(crate) e_heat_init_kwh: f64,
    /// Maximum usable tank energy above T_min [kWh] = (T_max − T_min) × thermal_mass.
    pub(crate) e_heat_max_kwh: f64,
    /// Constant per-step thermal demand [kW]: draw_kw + k_loss × (T_mid − ambient).
    pub(crate) q_heat_dem_kw: f64,
    /// Target tank energy at deadline [kWh above T_min]. = e_heat_max_kwh in autonomous mode.
    pub(crate) e_heat_target_kwh: f64,
    /// Relay switching penalty [EUR/switch event].
    pub(crate) lambda_heat_sw_eur: f64,
    /// Soft penalty per slot for using the full power tier over mid tier [€/slot].
    pub(crate) w_tier_penalty_eur: f64,
    /// Initial heater mid-power binary (1.0 if heater was at mid power last tick).
    pub(crate) heat_initial_z_mid: f64,
    /// Initial heater full-power binary (1.0 if heater was at full power last tick).
    pub(crate) heat_initial_z_full: f64,

    // ── Shiftable loads (Phase B) ────────────────────────────────────────────
    /// MILP-ready shiftable load descriptors (one per ShiftableLoad that fits the horizon)
    pub(crate) shiftable_loads: Vec<ShiftableLoadMilp>,

    /// Live SoC of the EV at plan-build time [0.0..1.0].
    /// Used to integrate the planned EV charge power into a SoC trajectory for
    /// the timeline API. None when no EV asset is present or EV state is unavailable.
    pub(crate) soc_ev_init: Option<f64>,
}

/// Internal MILP descriptor for one shiftable load block.
#[derive(Debug, Clone)]
pub(crate) struct ShiftableLoadMilp {
    /// Label for allocations (e.g. "wm")
    pub(crate) asset_id: String,
    /// Fixed power level while running [kW]
    pub(crate) power_kw: f64,
    /// Duration in planning slots (ceil)
    pub(crate) duration_slots: usize,
    /// Valid start-slot indices within [0, n)
    pub(crate) valid_start_slots: Vec<usize>,
}

// ── Builder functions ────────────────────────────────────────────────────────

/// Build the MILP objective weights from the profile's planner configuration.
/// `objective` overrides `profile.planner.objective`; pass `profile.planner.objective`
/// to use the profile default.
pub(crate) fn build_phase1_weights(profile: &Profile, objective: PlannerObjective) -> Phase1Weights {
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

pub(crate) fn build_phase2_weights(inputs: &MilpInputs, profile: &Profile) -> Phase2Weights {
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
pub(crate) fn deadline_to_step(deadline: DateTime<Utc>, now: DateTime<Utc>, step_s: u64, n: usize) -> usize {
    let secs = (deadline - now).num_seconds();
    (secs / step_s as i64).clamp(0, (n.saturating_sub(1)) as i64) as usize
}

/// Output from the MILP solver for one planning cycle.
/// All `Vec<f64>` fields have `len == n` except `e_bat_kwh` which has `len == n + 1`.
#[derive(Debug, Clone)]
pub(crate) struct SolveOutput {
    pub(crate) objective_eur: f64,
    /// Grid import power per step [kW]
    pub(crate) p_imp_kw: Vec<f64>,
    /// Grid export power per step [kW]
    pub(crate) p_exp_kw: Vec<f64>,
    /// Battery charge power per step [kW]; all 0.0 when no battery present
    pub(crate) p_bat_ch_kw: Vec<f64>,
    /// Battery discharge power per step [kW]; all 0.0 when no battery present
    pub(crate) p_bat_dis_kw: Vec<f64>,
    /// EV charge power per step [kW]; all 0.0 when EV absent/MustNotRun
    pub(crate) p_ev_kw: Vec<f64>,
    /// Heater mid-level binary (0/1) per step
    pub(crate) z_heat_mid: Vec<f64>,
    /// Heater full-level binary (0/1) per step
    pub(crate) z_heat_full: Vec<f64>,
    /// Battery SoC trajectory [kWh], len = n + 1; index 0 = initial SoC
    pub(crate) e_bat_kwh: Vec<f64>,
    /// Import contractual-limit violation slack [kW]
    pub(crate) s_imp_viol_kw: Vec<f64>,
    /// Export contractual-limit violation slack [kW]
    pub(crate) s_exp_viol_kw: Vec<f64>,
    /// EV on-flag binary per step (1 = charging, 0 = off)
    pub(crate) z_ev_on: Vec<f64>,
    /// Total extra EV energy above core requirement [kWh]
    pub(crate) e_ev_extra: f64,
    /// 1.0 when EV core target is met (MayRun only); 0.0 otherwise
    pub(crate) z_ev_core: f64,
    /// 1.0 when heater energy deadline is met (MayRun only); 0.0 otherwise
    pub(crate) z_heat_ready: f64,
    /// Tank energy above T_min [kWh] per slot; empty when heater absent
    pub(crate) e_heat_tank_kwh: Vec<f64>,
    /// Per-shiftable-load power schedule [kW]; outer len = num loads, inner len = n
    pub(crate) p_shiftable_kw: Vec<Vec<f64>>,
}
