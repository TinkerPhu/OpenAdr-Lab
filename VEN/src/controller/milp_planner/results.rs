use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::entities::asset_params::{BatteryParams, EvParams, HeaterParams};
use crate::entities::asset::PlanTrigger;
use crate::entities::device_session::ShiftableLoad;
use crate::entities::plan::{
    AssetAllocation, CostBreakdown, Plan, PlanSummary, PlanTimeSlot,
    PlanWarning, PlanningHorizon, WarningSeverity,
};
use crate::entities::planner_params::{PlannerObjective, PlannerParams};

use super::asset_port::{battery_future_state, ev_future_state_at, ev_soc_trajectory, heater_future_state};
use super::envelopes::build_plan_envelopes;
use super::types::*;

/// Fallback plan returned when the MILP solver fails.
/// When `inputs` is `Some`, emits populated slots with zero allocations
/// so tests asserting on per-slot fields still find data.
pub(crate) fn fallback_plan(
    planner: &PlannerParams,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    inputs: Option<&MilpInputs>,
    reason: String,
    objective: PlannerObjective,
    ev_cfg: Option<&EvParams>,
    heat_cfg: Option<&HeaterParams>,
) -> Plan {
    let step_s = planner.plan_step_s;
    let horizon_h = planner.plan_horizon_h;
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
                    planned_state_by_asset: std::collections::HashMap::new(),
                }
            })
            .collect(),
        None => vec![],
    };
    let envelopes = match inputs {
        Some(inp) => build_plan_envelopes(
            ev_session,
            heater_target,
            shiftable_loads,
            inp,
            planner,
            ev_cfg,
            heat_cfg,
            now,
        ),
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
pub(crate) fn translate_to_plan(
    sol: &SolveOutput,
    inputs: &MilpInputs,
    weights: &Phase1Weights,
    planner: &PlannerParams,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    ev_session: Option<&crate::entities::device_session::EvSession>,
    heater_target: Option<&crate::entities::device_session::HeaterTarget>,
    shiftable_loads: &[ShiftableLoad],
    objective: PlannerObjective,
    phase1_cost_eur: f64,
    friction_eur: f64,
    battery_cfg: Option<&BatteryParams>,
    ev_cfg: Option<&EvParams>,
    heat_cfg: Option<&HeaterParams>,
) -> Plan {
    let step_s = planner.plan_step_s;
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

    let ev_id = ev_cfg.map(|c| c.id.clone());
    let heater_id = heat_cfg.map(|c| c.id.clone());
    let bat_id = battery_cfg.map(|c| c.id.clone());

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
            let heat_kw = sol.z_heat_mid[t] * inputs.p_heat_mid_kw
                + sol.z_heat_full[t] * inputs.p_heat_full_kw;
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
                    (
                        sp,
                        gp,
                        gp * inputs.c_imp_eur_kwh[t] * dt_h - sp * inputs.c_exp_eur_kwh[t] * dt_h,
                        gp * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h,
                    )
                } else {
                    // Discharging: negative power_kw = net injection; revenue = negative cost
                    let dis_kw = sol.p_bat_dis_kw[t];
                    (
                        0.0,
                        bat_net_kw,
                        -(dis_kw * inputs.c_exp_eur_kwh[t] * dt_h),
                        -(dis_kw * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h),
                    )
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
            planned_kw_by_asset: allocations
                .iter()
                .map(|a| (a.asset_id.clone(), a.power_kw))
                .collect(),
            allocations,
            net_import_kw: sol.p_imp_kw[t],
            net_export_kw: sol.p_exp_kw[t],
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            bat_charge_kw: sol.p_bat_ch_kw[t],
            bat_discharge_kw: sol.p_bat_dis_kw[t],
            planned_state_by_asset: std::collections::HashMap::new(),
        });
    }

    // ── SoC trajectory ──────────────────────────────────────────────────
    let soc_trajectory_kwh = sol.e_bat_kwh.clone();

    // ── Planned state by asset (T008/T013/T017) ──────────────────────────
    // Battery SoC forecast — e_bat_kwh[t] is start-of-slot stored energy.
    if let (Some(ref bid), Some(bat_cfg)) = (&bat_id, battery_cfg) {
        let capacity_kwh = bat_cfg.capacity_kwh;
        for t in 0..n {
            slots[t]
                .planned_state_by_asset
                .insert(bid.clone(), battery_future_state(sol.e_bat_kwh[t], capacity_kwh));
        }
    }
    // EV SoC forecast — requires soc_ev_init captured in MilpInputs.
    if let (Some(ref eid), Some(soc_init), Some(ev_cfg)) = (&ev_id, inputs.soc_ev_init, ev_cfg) {
        let traj = ev_soc_trajectory(&sol.p_ev_kw, soc_init, ev_cfg.battery_kwh, dt_h);
        for t in 0..n {
            slots[t]
                .planned_state_by_asset
                .insert(eid.clone(), ev_future_state_at(traj[t]));
        }
    }
    // Heater T_tank forecast — e_heat_tank_kwh[t] is stored energy above temp_min_c.
    if let (Some(ref hid), Some(heat_cfg)) = (&heater_id, heat_cfg) {
        if !sol.e_heat_tank_kwh.is_empty() {
            let thermal_mass = heat_cfg.thermal_mass_kwh_per_c;
            let temp_min = heat_cfg.temp_min_c;
            for t in 0..n {
                slots[t].planned_state_by_asset.insert(
                    hid.clone(),
                    heater_future_state(sol.e_heat_tank_kwh[t], temp_min, thermal_mass),
                );
            }
        }
    }

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
            .map(|t| weights.c_bat_wear_eur_kwh * (sol.p_bat_ch_kw[t] + sol.p_bat_dis_kw[t]) * dt_h)
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
        envelopes: build_plan_envelopes(
            ev_session,
            heater_target,
            shiftable_loads,
            inputs,
            planner,
            ev_cfg,
            heat_cfg,
            now,
        ),
        warnings,
        objective,
        soc_trajectory_kwh,
        objective_eur: phase1_cost_eur,
        friction_eur,
        cost_breakdown,
    };
    plan
}
