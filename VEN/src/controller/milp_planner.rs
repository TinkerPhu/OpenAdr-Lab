//! MILP-based HEMS planner — entry point.
//!
//! This module will contain the full Mixed-Integer Linear Program formulation
//! for the HEMS energy scheduler.  See `docs/plans/milp_planner_transition.md`
//! for the design.
//!
//! Currently a stub: returns an empty plan so the rest of the controller
//! (dispatcher, monitor, timeline) compiles and runs while the MILP
//! implementation is being built.

use chrono::{DateTime, Duration, Utc};
use good_lp::{constraint, variable, variables, Expression, Variable};
use good_lp::solvers::highs::highs;
use uuid::Uuid;

use crate::entities::asset::{PlanTrigger, UserRequestMode};
use crate::entities::capacity::OadrCapacityState;
use crate::entities::energy_packet::{EnergyPacket, PacketStatus};
use crate::entities::plan::{
    CostBreakdown, Plan, PlanStep, PlanSummary, PlanningHorizon, PlanWarning,
};
use crate::entities::tariff_snapshot::TariffTimeSeries;
use crate::profile::{PlannerObjective, Profile};
use crate::simulator::SimState;

// ── Internal MILP types ──────────────────────────────────────────────────────

/// Internal MILP load mode for an asset (EV / heater).
/// Derived from the active EnergyPacket state + UserRequestMode.
#[derive(Debug, Clone, PartialEq)]
enum MilpLoadMode {
    /// Hard energy requirement — must be met within deadline. Translates from
    /// UserRequestMode::Asap or ByDeadline on an active/pending/scheduled packet.
    MustRun,
    /// Soft energy target — controlled by a reward term in the objective.
    /// Translates from Free/MaxCost/Opportunistic modes, or asset present but no packet.
    MayRun,
    /// Asset absent, currently unavailable, or packet is terminal
    /// (Abandoned / Completed / Failed / PartialCompleted).
    MustNotRun,
}

/// Objective function coefficients, derived from PlannerConfig / PlannerObjective preset.
#[derive(Debug, Clone)]
struct MilpWeights {
    /// Scales C_energy = Σ(c_imp·p_imp − c_exp·p_exp)·Δt
    w_energy: f64,
    /// Monetises GHG emissions [€/kgCO₂]
    w_ghg: f64,
    /// Penalty per kWh of total grid exchange (import + export)
    w_grid: f64,
    /// Scales contractual violation penalties
    w_viol: f64,
    /// Battery cycling wear cost [€/kWh charged or discharged]
    c_bat_wear_eur_kwh: f64,
    /// Scales service reward terms; always 1.0 until grid-services are modelled
    w_services: f64,
}

/// Fully-resolved MILP input parameters for one planning cycle.
/// Created by `build_milp_inputs()`; consumed by `solve_milp()` (Phase 3).
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
    /// Last step index that counts toward the heater energy sum. None = open horizon.
    t_heat_dead_step: Option<usize>,
    /// Mid power level [kW] = mid_kw.unwrap_or(max_kw / 2.0)
    p_heat_mid_kw: f64,
    /// Full power level [kW] = max_kw
    p_heat_full_kw: f64,
    /// Energy requirement from active packet [kWh]; 0.0 when absent
    e_heat_req_kwh: f64,
    /// Comfort reward for meeting deadline (MayRun only) [€]
    v_heat_eur: f64,
    // WM: always MustNotRun — deferred to a future phase; no WM fields added yet.
}

// ── Builder functions ────────────────────────────────────────────────────────

/// Build the MILP objective weights from the profile's planner configuration.
/// When `objective != Custom`, the preset overrides the individual weight fields.
fn build_milp_weights(profile: &Profile) -> MilpWeights {
    let p = &profile.planner;
    match p.objective {
        PlannerObjective::MinCost => MilpWeights {
            w_energy: 1.0,
            w_ghg: 0.20,
            w_grid: 0.02,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: 0.03,
            w_services: 1.0,
        },
        PlannerObjective::MinGhg => MilpWeights {
            w_energy: 0.0,
            w_ghg: 10.0,
            w_grid: 0.0,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: 0.0,
            w_services: 1.0,
        },
        PlannerObjective::MinGrid => MilpWeights {
            w_energy: 0.0,
            w_ghg: 0.0,
            w_grid: 1.0,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: 0.0,
            w_services: 1.0,
        },
        PlannerObjective::MaxRevenue => MilpWeights {
            w_energy: 1.0,
            w_ghg: 0.0,
            w_grid: 0.0,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: 0.03,
            w_services: 1.0,
        },
        PlannerObjective::Custom => MilpWeights {
            w_energy: p.w_energy,
            w_ghg: p.w_ghg,
            w_grid: p.w_grid,
            w_viol: p.w_viol,
            c_bat_wear_eur_kwh: p.c_bat_wear_eur_kwh,
            w_services: 1.0,
        },
    }
}

/// Returns the first active-or-pending/scheduled packet for `asset_id`.
/// Prefers Active over Pending/Scheduled when both exist.
fn active_packet<'a>(packets: &'a [EnergyPacket], asset_id: &str) -> Option<&'a EnergyPacket> {
    packets
        .iter()
        .filter(|p| p.asset_id == asset_id)
        .find(|p| p.status == PacketStatus::Active)
        .or_else(|| {
            packets
                .iter()
                .filter(|p| p.asset_id == asset_id)
                .find(|p| {
                    matches!(
                        p.status,
                        PacketStatus::Pending | PacketStatus::Scheduled
                    )
                })
        })
}

/// Translate an optional active packet into a MILP LoadMode.
///
/// `None` (no packet found by `active_packet`) → MustNotRun.
/// Any remaining terminal status → MustNotRun (defensive: active_packet filters these out).
/// Hard-commitment modes (Asap, ByDeadline) → MustRun.
/// All other modes → MayRun.
fn packet_load_mode(packet: Option<&EnergyPacket>) -> MilpLoadMode {
    match packet {
        None => MilpLoadMode::MustNotRun,
        Some(p) => match p.request_mode {
            UserRequestMode::Asap | UserRequestMode::ByDeadline => MilpLoadMode::MustRun,
            UserRequestMode::AsapFree
            | UserRequestMode::ByDeadlineFree
            | UserRequestMode::MaxCost
            | UserRequestMode::Opportunistic => MilpLoadMode::MayRun,
        },
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
/// The resulting `MilpInputs` is ready to pass directly to `solve_milp()`.
fn build_milp_inputs(
    assets: &SimState,
    tariffs: &TariffTimeSeries,
    packets: &[EnergyPacket],
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
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
        p_pv.push(pv_cfg.map(|c| c.forecast_kw(slot_t)).unwrap_or(0.0));
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
        let live_soc = assets
            .battery_state()
            .map(|s| s.soc)
            .unwrap_or(bat_cfg.initial_soc);
        let cap = bat_cfg.capacity_kwh;
        let eff = bat_cfg.round_trip_efficiency.sqrt();
        (
            Some(cap),
            Some(live_soc * cap),
            Some(bat_cfg.min_soc * cap),
            Some(cap),
            Some(bat_cfg.max_charge_kw),
            Some(bat_cfg.max_discharge_kw),
            Some(eff),
            Some(eff),
        )
    } else {
        (None, None, None, None, None, None, None, None)
    };

    // ── EV ────────────────────────────────────────────────────────────────────
    let (a_ev, ev_mode, t_ev_dead, p_ev_max, p_ev_min, e_ev_core, e_ev_extra, v_ev_extra) =
        if let Some(ev_cfg) = profile.ev_config() {
            let plugged = assets.ev_state().map(|s| s.plugged).unwrap_or(false);

            if !plugged {
                // EV present in profile but currently unplugged — cannot schedule
                (
                    vec![false; n],
                    MilpLoadMode::MustNotRun,
                    None,
                    ev_cfg.max_charge_kw,
                    ev_cfg.min_charge_kw,
                    0.0,
                    ev_cfg.battery_kwh * (1.0 - ev_cfg.soc_target),
                    profile.planner.v_ev_extra_eur_kwh,
                )
            } else {
                let pkt = active_packet(packets, &ev_cfg.id);
                let mode = packet_load_mode(pkt);

                let deadline_step = pkt
                    .and_then(|p| p.value_curve.active_deadline())
                    .map(|tier| deadline_to_step(tier.deadline, now, step_s, n));

                let mask: Vec<bool> = (0..n)
                    .map(|t| deadline_step.map(|d| t <= d).unwrap_or(true))
                    .collect();

                let core_kwh = pkt.map(|p| p.target_energy_kwh).unwrap_or(0.0);
                (
                    mask,
                    mode,
                    deadline_step,
                    ev_cfg.max_charge_kw,
                    ev_cfg.min_charge_kw,
                    core_kwh,
                    ev_cfg.battery_kwh * (1.0 - ev_cfg.soc_target),
                    profile.planner.v_ev_extra_eur_kwh,
                )
            }
        } else {
            // No EV asset in profile
            (
                vec![false; n],
                MilpLoadMode::MustNotRun,
                None,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            )
        };

    // ── Heater ────────────────────────────────────────────────────────────────
    let (heater_mode, t_heat_dead, p_mid, p_full, e_heat_req) =
        if let Some(heat_cfg) = profile.heater_config() {
            let pkt = active_packet(packets, &heat_cfg.id);
            let mode = packet_load_mode(pkt);
            let deadline_step = pkt
                .and_then(|p| p.value_curve.active_deadline())
                .map(|tier| deadline_to_step(tier.deadline, now, step_s, n));
            let req_kwh = pkt.map(|p| p.target_energy_kwh).unwrap_or(0.0);
            let mid = heat_cfg.mid_kw.unwrap_or(heat_cfg.max_kw / 2.0);
            (mode, deadline_step, mid, heat_cfg.max_kw, req_kwh)
        } else {
            (MilpLoadMode::MustNotRun, None, 0.0, 0.0, 0.0)
        };

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
        e_heat_req_kwh: e_heat_req,
        v_heat_eur: profile.planner.v_heat_eur,
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
    // WM fields omitted — deferred to a future phase
}

/// Run the MILP model and return the optimal schedule.
///
/// Ported from `d:/Tinker/milp_demo/src/main.rs` with the following adaptations:
/// - Fixed-size arrays `[f64; N]` replaced by dynamic `Vec<f64>` (N = `inputs.n`)
/// - Battery/EV/heater presence handled via Option unwrap with zero fallbacks
/// - Washing machine block disabled (no WM asset in current profile)
fn solve_milp(
    inputs: &MilpInputs,
    weights: &MilpWeights,
) -> Result<SolveOutput, Box<dyn std::error::Error>> {
    let n = inputs.n;
    let dt_h = inputs.dt_h;

    // ── Unwrap optional battery params (zero = absent → vars fixed to 0) ─────
    let bat_present = inputs.e_bat_nom_kwh.is_some();
    let bat_init = inputs.e_bat_init_kwh.unwrap_or(0.0);
    let bat_min = inputs.e_bat_min_kwh.unwrap_or(0.0);
    let bat_max = inputs.e_bat_max_kwh.unwrap_or(0.0);
    let bat_ch_max = inputs.p_bat_ch_max_kw.unwrap_or(0.0);
    let bat_dis_max = inputs.p_bat_dis_max_kw.unwrap_or(0.0);
    let eff_ch = inputs.eff_bat_ch.unwrap_or(1.0);
    let eff_dis = inputs.eff_bat_dis.unwrap_or(1.0);

    let mut vars = variables!();

    // ── Grid variables ────────────────────────────────────────────────────────
    let p_imp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let p_exp: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let u_grid: Vec<Variable> = (0..n).map(|_| vars.add(variable().binary())).collect();
    let s_imp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();
    let s_exp_viol: Vec<Variable> = (0..n).map(|_| vars.add(variable().min(0.0))).collect();

    // ── Battery variables (fixed to 0 when no battery) ────────────────────────
    let p_bat_ch: Vec<Variable> = (0..n)
        .map(|_| {
            if bat_present {
                vars.add(variable().min(0.0).max(bat_ch_max))
            } else {
                vars.add(variable().min(0.0).max(0.0))
            }
        })
        .collect();
    let p_bat_dis: Vec<Variable> = (0..n)
        .map(|_| {
            if bat_present {
                vars.add(variable().min(0.0).max(bat_dis_max))
            } else {
                vars.add(variable().min(0.0).max(0.0))
            }
        })
        .collect();
    let u_bat: Vec<Variable> = (0..n)
        .map(|_| {
            if bat_present {
                vars.add(variable().binary())
            } else {
                vars.add(variable().min(0.0).max(0.0))
            }
        })
        .collect();
    let e_bat: Vec<Variable> = (0..=n)
        .map(|i| {
            if !bat_present {
                vars.add(variable().min(0.0).max(0.0))
            } else if i == 0 {
                vars.add(variable().min(bat_init).max(bat_init))
            } else {
                vars.add(variable().min(bat_min).max(bat_max))
            }
        })
        .collect();

    // ── EV variables ──────────────────────────────────────────────────────────
    let p_ev: Vec<Variable> = (0..n)
        .map(|_| {
            if inputs.ev_mode == MilpLoadMode::MustNotRun {
                vars.add(variable().min(0.0).max(0.0))
            } else {
                vars.add(variable().min(0.0).max(inputs.p_ev_max_kw))
            }
        })
        .collect();
    let z_ev_on: Vec<Variable> = (0..n)
        .map(|t| {
            if inputs.ev_mode == MilpLoadMode::MustNotRun {
                vars.add(variable().min(0.0).max(0.0))
            } else {
                let ub = if inputs.a_ev[t] { 1.0 } else { 0.0 };
                vars.add(variable().max(ub).binary())
            }
        })
        .collect();
    let z_ev_core = if inputs.ev_mode == MilpLoadMode::MayRun {
        vars.add(variable().binary())
    } else {
        vars.add(variable().min(0.0).max(0.0))
    };
    let e_ev_extra = if inputs.ev_mode == MilpLoadMode::MustNotRun {
        vars.add(variable().min(0.0).max(0.0))
    } else {
        vars.add(variable().min(0.0).max(inputs.e_ev_extra_max_kwh))
    };

    // ── Heater variables ──────────────────────────────────────────────────────
    let z_heat_mid: Vec<Variable> = (0..n)
        .map(|_| {
            if inputs.heater_mode == MilpLoadMode::MustNotRun {
                vars.add(variable().min(0.0).max(0.0))
            } else {
                vars.add(variable().binary())
            }
        })
        .collect();
    let z_heat_full: Vec<Variable> = (0..n)
        .map(|_| {
            if inputs.heater_mode == MilpLoadMode::MustNotRun {
                vars.add(variable().min(0.0).max(0.0))
            } else {
                vars.add(variable().binary())
            }
        })
        .collect();
    let z_heat_ready = if inputs.heater_mode == MilpLoadMode::MayRun {
        vars.add(variable().binary())
    } else {
        vars.add(variable().min(0.0).max(0.0))
    };

    // ── Objective + deadline energy accumulators (loop 1) ────────────────────
    let t_ev_dlim = inputs.t_ev_dead_step.unwrap_or(n.saturating_sub(1));
    let t_heat_dlim = inputs.t_heat_dead_step.unwrap_or(n.saturating_sub(1));

    let mut objective = Expression::from(0.0);
    let mut ev_energy_expr = Expression::from(0.0);
    let mut heat_energy_expr = Expression::from(0.0);

    for t in 0..n {
        // C_energy: import cost minus export revenue
        objective += (weights.w_energy * dt_h * inputs.c_imp_eur_kwh[t]) * p_imp[t];
        objective += -(weights.w_energy * dt_h * inputs.c_exp_eur_kwh[t]) * p_exp[t];
        // C_GHG: emissions cost
        objective += (weights.w_ghg * dt_h * inputs.g_imp_kgco2_kwh[t]) * p_imp[t];
        // C_grid: penalise total exchange volume (import + export)
        objective += (weights.w_grid * dt_h) * p_imp[t];
        objective += (weights.w_grid * dt_h) * p_exp[t];
        // C_wear: battery cycling cost
        objective += (weights.c_bat_wear_eur_kwh * dt_h) * p_bat_ch[t];
        objective += (weights.c_bat_wear_eur_kwh * dt_h) * p_bat_dis[t];
        // C_violations: contractual limit breaches
        objective += (weights.w_viol * inputs.pen_imp_eur_kwh * dt_h) * s_imp_viol[t];
        objective += (weights.w_viol * inputs.pen_exp_eur_kwh * dt_h) * s_exp_viol[t];

        // Deadline-gated energy accumulators
        if t <= t_ev_dlim {
            ev_energy_expr += dt_h * p_ev[t];
        }
        if t <= t_heat_dlim {
            heat_energy_expr += (dt_h * inputs.p_heat_mid_kw) * z_heat_mid[t];
            heat_energy_expr += (dt_h * inputs.p_heat_full_kw) * z_heat_full[t];
        }
    }

    // V_services rewards (subtracted from objective → incentives)
    if inputs.ev_mode == MilpLoadMode::MayRun {
        objective += -(weights.w_services * inputs.v_ev_core_eur) * z_ev_core;
    }
    if inputs.ev_mode != MilpLoadMode::MustNotRun {
        objective += -(weights.w_services * inputs.v_ev_extra_eur_kwh) * e_ev_extra;
    }
    if inputs.heater_mode == MilpLoadMode::MayRun {
        objective += -(weights.w_services * inputs.v_heat_eur) * z_heat_ready;
    }
    // WM reward omitted

    let mut model = vars.minimise(&objective).using(highs);

    // ── Constraints (loop 2) ─────────────────────────────────────────────────
    for t in 0..n {
        // Heater power expression
        let heat_kw = inputs.p_heat_mid_kw * z_heat_mid[t]
            + inputs.p_heat_full_kw * z_heat_full[t];

        // Power balance (WM term omitted)
        model = model.with(constraint!(
            p_imp[t] + inputs.p_pv_kw[t] + p_bat_dis[t]
                == inputs.p_base_kw[t] + p_ev[t] + heat_kw + p_bat_ch[t] + p_exp[t]
        ));

        // Grid mutual exclusion (no simultaneous import and export)
        model = model.with(constraint!(
            p_imp[t] <= inputs.p_imp_max_phys_kw[t] * u_grid[t]
        ));
        model = model.with(constraint!(
            p_exp[t] <= inputs.p_exp_max_phys_kw[t] * (1.0 - u_grid[t])
        ));
        // Contractual limits with slack
        model = model.with(constraint!(
            p_imp[t] <= inputs.p_imp_max_cont_kw[t] + s_imp_viol[t]
        ));
        model = model.with(constraint!(
            p_exp[t] <= inputs.p_exp_max_cont_kw[t] + s_exp_viol[t]
        ));

        // Battery charge/discharge mutual exclusion + SoC dynamics
        model = model.with(constraint!(p_bat_ch[t] <= bat_ch_max * u_bat[t]));
        model = model.with(constraint!(
            p_bat_dis[t] <= bat_dis_max * (1.0 - u_bat[t])
        ));
        model = model.with(constraint!(
            e_bat[t + 1]
                == e_bat[t]
                    + dt_h * eff_ch * p_bat_ch[t]
                    - dt_h * (1.0 / eff_dis) * p_bat_dis[t]
        ));

        // Heater: at most one active level per step
        model = model.with(constraint!(z_heat_mid[t] + z_heat_full[t] <= 1.0));

        // EV semi-continuous power gate
        if inputs.ev_mode != MilpLoadMode::MustNotRun {
            let ev_ub = if inputs.a_ev[t] {
                inputs.p_ev_max_kw
            } else {
                0.0
            };
            model = model.with(constraint!(
                p_ev[t] >= inputs.p_ev_min_kw * z_ev_on[t]
            ));
            model = model.with(constraint!(p_ev[t] <= ev_ub * z_ev_on[t]));
        }
    }

    // Terminal battery constraint: prevent end-of-horizon depletion
    if bat_present {
        model = model.with(constraint!(e_bat[n] >= bat_init));
    }

    // EV mode-conditional energy constraints
    match inputs.ev_mode {
        MilpLoadMode::MustRun => {
            model = model.with(constraint!(
                ev_energy_expr.clone() >= inputs.e_ev_core_kwh
            ));
            model = model.with(constraint!(
                ev_energy_expr <= inputs.e_ev_core_kwh + e_ev_extra
            ));
        }
        MilpLoadMode::MayRun => {
            model = model.with(constraint!(
                ev_energy_expr.clone() >= inputs.e_ev_core_kwh * z_ev_core
            ));
            model = model.with(constraint!(
                ev_energy_expr <= inputs.e_ev_core_kwh * z_ev_core + e_ev_extra
            ));
            model = model.with(constraint!(
                e_ev_extra <= inputs.e_ev_extra_max_kwh * z_ev_core
            ));
        }
        MilpLoadMode::MustNotRun => {} // p_ev fixed to 0 via variable bounds
    }

    // Heater mode-conditional energy constraints
    match inputs.heater_mode {
        MilpLoadMode::MustRun => {
            model = model.with(constraint!(heat_energy_expr >= inputs.e_heat_req_kwh));
        }
        MilpLoadMode::MayRun => {
            model = model.with(constraint!(
                heat_energy_expr >= inputs.e_heat_req_kwh * z_heat_ready
            ));
        }
        MilpLoadMode::MustNotRun => {} // z_heat_mid/full fixed to 0 via variable bounds
    }

    // WM constraints omitted

    model = model.with_time_limit(60.0);
    model = model.with_mip_gap(0.02)?;
    let solution = model.solve()?;

    // ── Populate output ───────────────────────────────────────────────────────
    let mut out = SolveOutput {
        objective_eur: solution.eval(&objective),
        p_imp_kw: vec![0.0; n],
        p_exp_kw: vec![0.0; n],
        p_bat_ch_kw: vec![0.0; n],
        p_bat_dis_kw: vec![0.0; n],
        p_ev_kw: vec![0.0; n],
        z_heat_mid: vec![0.0; n],
        z_heat_full: vec![0.0; n],
        e_bat_kwh: vec![0.0; n + 1],
        s_imp_viol_kw: vec![0.0; n],
        s_exp_viol_kw: vec![0.0; n],
        z_ev_on: vec![0.0; n],
        e_ev_extra: solution.value(e_ev_extra),
        z_ev_core: solution.value(z_ev_core),
        z_heat_ready: solution.value(z_heat_ready),
    };
    for t in 0..n {
        out.p_imp_kw[t] = solution.value(p_imp[t]);
        out.p_exp_kw[t] = solution.value(p_exp[t]);
        out.p_bat_ch_kw[t] = solution.value(p_bat_ch[t]);
        out.p_bat_dis_kw[t] = solution.value(p_bat_dis[t]);
        out.p_ev_kw[t] = solution.value(p_ev[t]);
        out.z_heat_mid[t] = solution.value(z_heat_mid[t]);
        out.z_heat_full[t] = solution.value(z_heat_full[t]);
        out.s_imp_viol_kw[t] = solution.value(s_imp_viol[t]);
        out.s_exp_viol_kw[t] = solution.value(s_exp_viol[t]);
        out.z_ev_on[t] = solution.value(z_ev_on[t]);
    }
    for t in 0..=n {
        out.e_bat_kwh[t] = solution.value(e_bat[t]);
    }
    Ok(out)
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Run the MILP planner and return a new Plan + audit trail.
///
/// Stub: returns an empty plan covering the configured horizon.
/// The MILP formulation will replace this body in the next implementation phase.
pub fn run_planner(
    _assets: &SimState,
    _tariffs: &TariffTimeSeries,
    packets: &[EnergyPacket],
    _capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
) -> (Plan, Vec<PlanStep>) {
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
        severity: crate::entities::plan::WarningSeverity::Info,
        packet_id: None,
        message: "MILP planner not yet implemented — returning empty plan.".into(),
        suggested_action: None,
    };

    let plan = Plan {
        id: Uuid::new_v4(),
        created_at: now,
        trigger,
        horizon,
        slots: vec![],
        summary: PlanSummary::default(),
        envelopes: vec![],
        packets: packets.to_vec(),
        warnings: vec![warning],
        steps: vec![],
        soc_trajectory_kwh: vec![],
        objective_eur: 0.0,
        cost_breakdown: CostBreakdown::default(),
    };

    (plan, vec![])
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use crate::assets::AssetState;
    use crate::entities::asset::CompletionPolicy;
    use crate::entities::energy_packet::{DeadlineTier, ValueCurve};
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

    /// Build a minimal active EV packet.
    fn make_ev_packet(now: DateTime<Utc>, mode: UserRequestMode, deadline_h: f64) -> EnergyPacket {
        EnergyPacket {
            id: uuid::Uuid::new_v4(),
            asset_id: "ev".into(),
            status: PacketStatus::Active,
            earliest_start: now,
            latest_start: None,
            target_energy_kwh: 20.0,
            target_soc: None,
            desired_power_kw: 7.4,
            value_curve: ValueCurve {
                comfort_rates: vec![],
                deadline_tiers: vec![DeadlineTier {
                    deadline: now + Duration::seconds((deadline_h * 3600.0) as i64),
                    max_total_cost_eur: None,
                    max_marginal_rate_eur_kwh: None,
                    min_completion: 1.0,
                }],
                active_tier_index: 0,
            },
            request_mode: mode,
            completion_policy: CompletionPolicy::Stop,
            post_deadline_comfort_bid: None,
            planned_power_profile: vec![],
            past_power_profile: vec![],
            interruptible: false,
            tolerance_min: None,
            accumulated_cost_eur: 0.0,
            accumulated_co2_g: 0.0,
            estimated_cost_eur: 0.0,
            estimated_co2_g: 0.0,
            estimated_completion: 0.0,
            last_estimate_at: None,
            created_at: now,
            updated_at: now,
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

    // ── Tests ────────────────────────────────────────────────────────────────

    #[test]
    fn co2_g_kwh_divided_by_1000() {
        // CO₂ stored in tariffs as g/kWh; MILP needs kgCO₂/kWh
        let now = fixed_now();
        let profile = make_profile();
        let tariffs = make_tariffs(0.25, 0.08, 450.0); // 450 g/kWh
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &tariffs, &[], &no_capacity(), &profile, now);
        // All slots should have 0.45 kgCO₂/kWh
        assert!(inp.g_imp_kgco2_kwh.iter().all(|&v| (v - 0.45).abs() < 1e-9));
    }

    #[test]
    fn battery_eff_is_sqrt_rte() {
        // Each direction gets √(round_trip_efficiency), not the full RTE
        let now = fixed_now();
        let profile = make_profile(); // battery RTE = 0.9
        let sim = SimState::from_profile(&profile);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[], &no_capacity(), &profile, now);
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
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[], &no_capacity(), &profile, now);
        assert!((inp.e_bat_init_kwh.unwrap() - 3.0).abs() < 1e-9); // 0.3 × 10.0 = 3.0
    }

    #[test]
    fn battery_init_soc_falls_back_to_profile() {
        // When SimState has no battery asset, fall back to profile.battery_config().initial_soc
        let now = fixed_now();
        let profile = make_profile(); // initial_soc=0.5, capacity=10.0
        let mut sim = SimState::from_profile(&profile);
        sim.assets.clear(); // remove all assets → battery_state() returns None
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[], &no_capacity(), &profile, now);
        assert!((inp.e_bat_init_kwh.unwrap() - 5.0).abs() < 1e-9); // 0.5 × 10.0 = 5.0
    }

    #[test]
    fn ev_mask_plugged_no_deadline_all_true() {
        // Plugged EV with no active packet → all slots available
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[], &no_capacity(), &profile, now);
        assert!(inp.a_ev.iter().all(|&v| v));
        assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun); // no packet → MustNotRun (but mask is true)
        assert_eq!(inp.t_ev_dead_step, None);
    }

    #[test]
    fn ev_mask_plugged_with_deadline() {
        // Plugged EV with deadline at 1h from now → first 12 slots true (step=300s, 12×300=3600s)
        let now = fixed_now();
        let profile = make_profile(); // plan_step_s=300, plan_horizon_h=2 → 24 steps
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let pkt = make_ev_packet(now, UserRequestMode::ByDeadline, 1.0); // 1h deadline
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[pkt], &no_capacity(), &profile, now);
        // deadline = 3600s, step_s=300 → deadline_step = 3600/300 = 12
        let d = inp.t_ev_dead_step.unwrap();
        assert_eq!(d, 12);
        // Slots 0..=12 true, slots 13..23 false
        for t in 0..inp.n {
            assert_eq!(inp.a_ev[t], t <= d, "slot {t} mask mismatch");
        }
    }

    #[test]
    fn ev_mask_unplugged_all_false() {
        // Unplugged EV → all slots false regardless of packet
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, false);
        let pkt = make_ev_packet(now, UserRequestMode::ByDeadline, 1.0);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[pkt], &no_capacity(), &profile, now);
        assert!(inp.a_ev.iter().all(|&v| !v));
        assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun);
    }

    #[test]
    fn ev_mode_must_run_for_by_deadline() {
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let pkt = make_ev_packet(now, UserRequestMode::ByDeadline, 2.0);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[pkt], &no_capacity(), &profile, now);
        assert_eq!(inp.ev_mode, MilpLoadMode::MustRun);
    }

    #[test]
    fn ev_mode_must_run_for_asap() {
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let pkt = make_ev_packet(now, UserRequestMode::Asap, 2.0);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[pkt], &no_capacity(), &profile, now);
        assert_eq!(inp.ev_mode, MilpLoadMode::MustRun);
    }

    #[test]
    fn ev_mode_may_run_for_opportunistic() {
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let pkt = make_ev_packet(now, UserRequestMode::Opportunistic, 2.0);
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[pkt], &no_capacity(), &profile, now);
        assert_eq!(inp.ev_mode, MilpLoadMode::MayRun);
    }

    #[test]
    fn ev_mode_must_not_run_for_abandoned() {
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let mut pkt = make_ev_packet(now, UserRequestMode::ByDeadline, 2.0);
        pkt.status = PacketStatus::Abandoned;
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[pkt], &no_capacity(), &profile, now);
        // Abandoned packet is excluded by active_packet() → MustNotRun
        assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun);
    }

    #[test]
    fn ev_mode_must_not_run_for_no_packet() {
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        // No packets at all
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[], &no_capacity(), &profile, now);
        assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun);
    }

    #[test]
    fn load_mode_completed_is_must_not_run() {
        let now = fixed_now();
        let profile = make_profile();
        let mut sim = SimState::from_profile(&profile);
        set_ev_plugged(&mut sim, true);
        let mut pkt = make_ev_packet(now, UserRequestMode::ByDeadline, 2.0);
        pkt.status = PacketStatus::Completed;
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[pkt], &no_capacity(), &profile, now);
        assert_eq!(inp.ev_mode, MilpLoadMode::MustNotRun);
    }

    #[test]
    fn tariff_fallback_when_series_empty() {
        // Empty TariffTimeSeries → defaults: imp=0.25, exp=0.08, co2=300g→0.30kg
        let now = fixed_now();
        let profile = make_profile();
        let sim = SimState::from_profile(&profile);
        let empty_tariffs = TariffTimeSeries::from_snapshots(&[]);
        let inp = build_milp_inputs(&sim, &empty_tariffs, &[], &no_capacity(), &profile, now);
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
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[], &no_capacity(), &profile, now);
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
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[], &no_capacity(), &profile, now);
        assert!((inp.p_heat_mid_kw - 2.0).abs() < 1e-9);
    }

    #[test]
    fn weights_preset_min_cost() {
        let mut profile = make_profile();
        profile.planner.objective = PlannerObjective::MinCost;
        profile.planner.w_energy = 99.0; // should be overridden by preset
        profile.planner.w_ghg = 99.0;
        let w = build_milp_weights(&profile);
        assert!((w.w_energy - 1.0).abs() < 1e-9);
        assert!((w.w_ghg - 0.20).abs() < 1e-9);
        assert!((w.w_grid - 0.02).abs() < 1e-9);
        assert!((w.c_bat_wear_eur_kwh - 0.03).abs() < 1e-9);
    }

    #[test]
    fn weights_preset_min_ghg() {
        let mut profile = make_profile();
        profile.planner.objective = PlannerObjective::MinGhg;
        let w = build_milp_weights(&profile);
        assert!((w.w_energy - 0.0).abs() < 1e-9);
        assert!((w.w_ghg - 10.0).abs() < 1e-9);
        assert!((w.c_bat_wear_eur_kwh - 0.0).abs() < 1e-9);
    }

    #[test]
    fn weights_preset_custom_uses_fields() {
        let mut profile = make_profile();
        profile.planner.objective = PlannerObjective::Custom;
        profile.planner.w_energy = 0.5;
        profile.planner.w_ghg = 0.001;
        profile.planner.w_grid = 0.1;
        profile.planner.c_bat_wear_eur_kwh = 0.02;
        let w = build_milp_weights(&profile);
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
        let inp = build_milp_inputs(&sim, &TariffTimeSeries::from_snapshots(&[]), &[], &capacity, &profile, now);
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
            e_heat_req_kwh: 0.0,
            v_heat_eur: 0.0,
        }
    }

    fn make_solver_weights() -> MilpWeights {
        MilpWeights {
            w_energy: 1.0,
            w_ghg: 0.0,
            w_grid: 0.0,
            w_viol: 1.0,
            c_bat_wear_eur_kwh: 0.0,
            w_services: 1.0,
        }
    }

    #[test]
    fn solve_feasible_no_optional_assets() {
        // Minimal case: no battery, no EV, no heater. Import exactly covers base load.
        let inputs = make_solver_inputs(4, 0.5); // base = 0.5 kW
        let weights = make_solver_weights();
        let result = solve_milp(&inputs, &weights);
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

        let result = solve_milp(&inputs, &make_solver_weights());
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

        let result = solve_milp(&inputs, &make_solver_weights());
        assert!(result.is_ok(), "solver failed: {:?}", result.err());
        let out = result.unwrap();

        // Should charge ≈ 2 kWh total in cheap period and discharge ≈ 2 kWh in expensive period
        let total_ch = out.p_bat_ch_kw[0] + out.p_bat_ch_kw[1];
        let total_dis = out.p_bat_dis_kw[2] + out.p_bat_dis_kw[3];
        assert!(
            (total_ch - 2.0).abs() < 1e-2,
            "total cheap-period charging {total_ch:.4} kW-steps should be ≈ 2.0"
        );
        assert!(
            (total_dis - 2.0).abs() < 1e-2,
            "total expensive-period discharge {total_dis:.4} kW-steps should be ≈ 2.0"
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

        let out = solve_milp(&inputs, &make_solver_weights()).expect("solver failed");

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
}
