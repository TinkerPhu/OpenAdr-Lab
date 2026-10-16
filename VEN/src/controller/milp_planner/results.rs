use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::controller::milp_planner::{
    AllocationBasis, AssetMilpContext, AssetSolution, Participation,
};
use crate::entities::asset::PlanTrigger;
use crate::entities::plan::{
    AssetAllocation, CostBreakdown, Plan, PlanSummary, PlanTimeSlot, PlanWarning, PlanningHorizon,
    SolveStatus, WarningKind, WarningSeverity,
};
use crate::entities::planner_params::{PlannerObjective, PlannerParams};

use super::forecast_readback::{forecast_by_asset, generation_used_kw};
use super::session_flexibility::build_session_flexibility;
use super::types::*;

/// Fallback plan returned when the MILP solver fails.
/// When `inputs` is `Some`, emits populated slots with zero allocations
/// so tests asserting on per-slot fields still find data, and each asset's session
/// flexibility estimated without a schedule.
#[allow(clippy::too_many_arguments)]
/// The plan the opportunistic objective adopts while its forecast is not built: no slots, a
/// simulated status and a warning saying so (openspec `opportunistic-objective` 4.1). Dispatch
/// runs without look-ahead all the same; the plan only carries the objective to the tick.
pub fn plan_without_forecast(
    planner: &PlannerParams,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    objective: PlannerObjective,
) -> Plan {
    let mut plan = fallback_plan(planner, now, trigger, &[], None, String::new(), objective);
    plan.solve_status = SolveStatus::Simulated;
    plan.warnings = vec![PlanWarning {
        severity: WarningSeverity::Info,
        kind: WarningKind::OpportunisticNoForecast,
        message: "Opportunistic objective: dispatch runs without look-ahead; no forecast yet"
            .into(),
        suggested_action: None,
    }];
    plan
}

pub(crate) fn fallback_plan(
    planner: &PlannerParams,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    inputs: Option<&MilpInputs>,
    reason: String,
    objective: PlannerObjective,
) -> Plan {
    let total_steps = crate::entities::time_grid::slot_count(&planner.plan_zones);
    let total_horizon_s: i64 = planner
        .plan_zones
        .iter()
        .map(|z| z.step_s as i64 * z.slots as i64)
        .sum();
    let horizon_end = now + Duration::seconds(total_horizon_s);

    let horizon = PlanningHorizon {
        start_time: now,
        end_time: horizon_end,
        step_size_s: planner.plan_step_s,
        num_steps: total_steps,
        far_horizon: horizon_end,
        zones: planner.plan_zones.clone(),
    };
    let warning = PlanWarning {
        severity: WarningSeverity::Critical,
        kind: WarningKind::SolverInfeasible,
        message: reason,
        suggested_action: None,
    };
    let slots: Vec<PlanTimeSlot> = match inputs {
        Some(inp) => {
            let surplus_kw = inp.forecast_surplus_kw();
            (0..inp.n)
                .map(|t| PlanTimeSlot {
                    forecast_by_asset: forecast_by_asset(asset_contexts, None, t)
                        .into_iter()
                        .collect(),
                    ..slot_skeleton(inp, &surplus_kw, now, t)
                })
                .collect()
        }
        None => vec![],
    };
    let session_flexibility = match inputs {
        Some(inp) => build_session_flexibility(
            asset_contexts,
            inp,
            planner,
            now,
            None, // no solution — estimates fall back to energy × avg tariff
        ),
        None => vec![],
    };
    let penalty_rules_active = inputs
        .map(|i| active_penalty_rules(&i.penalty_rules))
        .unwrap_or_default();
    Plan {
        id: Uuid::new_v4(),
        created_at: now,
        trigger,
        horizon,
        planned_asset_ids: planned_asset_ids(asset_contexts),
        slots,
        summary: PlanSummary::default(),
        session_flexibility,
        warnings: vec![warning],
        objective,
        objective_eur: 0.0,
        friction_eur: 0.0,
        cost_breakdown: CostBreakdown::default(),
        solve_status: SolveStatus::Infeasible,
        // No two-phase solve happened, so there is no split to report.
        phase_report: None,
        penalty_rules_active,
        solver_ms: None,
        mip_gap_target: Some(planner.mip_gap_target),
    }
}

/// What slot `t` of any plan says about its inputs, whatever fills its allocations: its span,
/// the tariffs and CO2 intensity, the caps, the baseline and PV forecasts and the PV surplus
/// left after the baseline. It carries no allocation, no grid flow and no marginal cost, and
/// counts the whole PV forecast as used; a plan built from a solution or a forecast overrides
/// those fields, and only those.
fn slot_skeleton(
    inp: &MilpInputs,
    surplus_kw: &[f64],
    now: DateTime<Utc>,
    t: usize,
) -> PlanTimeSlot {
    PlanTimeSlot {
        slot_index: t,
        start: now + Duration::seconds(inp.cum_s[t]),
        end: now + Duration::seconds(inp.cum_s[t + 1]),
        import_tariff_eur_kwh: inp.c_imp_eur_kwh[t],
        export_tariff_eur_kwh: inp.c_exp_eur_kwh[t],
        co2_g_kwh: inp.g_imp_kgco2_kwh[t] * 1000.0,
        grid_effective_cost: inp.c_imp_eur_kwh[t],
        rate_estimated: inp.rate_stale[t],
        import_cap_kw: inp.p_imp_max_cont_kw[t],
        export_cap_kw: inp.p_exp_max_cont_kw[t],
        baseline_kw: inp.load_forecast_kw[t],
        pv_forecast_kw: inp.generation_forecast_kw[t],
        pv_used_kw: inp.generation_forecast_kw[t],
        surplus_available_kw: surplus_kw[t].max(0.0),
        marginal_cost_import_eur_per_kwh: 0.0,
        marginal_cost_export_eur_per_kwh: 0.0,
        allocations: vec![],
        net_import_kw: 0.0,
        net_export_kw: 0.0,
        import_flexibility_kw: 0.0,
        export_flexibility_kw: 0.0,
        planned_kw_by_asset: std::collections::HashMap::new(),
        planned_state_by_asset: std::collections::HashMap::new(),
        forecast_by_asset: std::collections::HashMap::new(),
    }
}

/// Whether the plan dispatches an asset (forecast participants are planned around, not
/// dispatched: no allocation, no planned power, not among the planned assets).
fn dispatched(ctx: &dyn AssetMilpContext) -> bool {
    matches!(ctx.participation(), Participation::Dispatched)
}

/// The roster ids of the assets a plan schedules, in planner input order.
fn planned_asset_ids(asset_contexts: &[Box<dyn AssetMilpContext>]) -> Vec<String> {
    asset_contexts
        .iter()
        .filter(|c| dispatched(c.as_ref()))
        .map(|c| c.asset_id().to_string())
        .collect()
}

/// The dispatched assets' positions in the order they claim a slot's PV surplus: the rank each
/// declares, equal ranks in planner input order. The solution holds one `AssetSolution` per
/// context, in context order.
fn surplus_claim_order(
    sol: &SolveOutput,
    asset_contexts: &[Box<dyn AssetMilpContext>],
) -> Vec<usize> {
    debug_assert!(
        sol.assets
            .iter()
            .zip(asset_contexts)
            .all(|(a, c)| a.asset_id == c.asset_id()),
        "translate_to_plan: the solution's assets are not the contexts it was solved with"
    );
    let mut order: Vec<usize> = (0..asset_contexts.len().min(sol.assets.len()))
        .filter(|&i| dispatched(asset_contexts[i].as_ref()))
        .collect();
    order.sort_by_key(|&i| asset_contexts[i].surplus_claim_rank());
    order
}

/// One asset's allocation in slot `t`, priced by the basis the asset declares, claiming the
/// PV surplus still left in the slot. `None` when the asset does not run in that slot.
fn allocation(
    sol: &AssetSolution,
    basis: AllocationBasis,
    t: usize,
    inputs: &MilpInputs,
    surplus_remaining_kw: &mut f64,
) -> Option<AssetAllocation> {
    let power_kw = sol.net_power_kw[t];
    if power_kw.abs() <= 0.01 {
        return None;
    }
    let (c_imp, c_exp, dt_h) = (
        inputs.c_imp_eur_kwh[t],
        inputs.c_exp_eur_kwh[t],
        inputs.dt_h[t],
    );
    let marginal_value = match basis {
        AllocationBasis::Load => c_imp,
        AllocationBasis::Storage => c_exp,
    };
    let discharging = basis == AllocationBasis::Storage && power_kw < 0.0;
    let (surplus_power_kw, grid_power_kw, cost_eur, co2_g) = if discharging {
        // Discharging: negative power_kw = net injection; revenue = negative cost
        let dis_kw = sol
            .storage
            .as_ref()
            .map_or(-power_kw, |legs| legs.discharge_kw[t]);
        (
            0.0,
            power_kw,
            -(dis_kw * c_exp * dt_h),
            -(dis_kw * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h),
        )
    } else {
        // Consuming: PV surplus first, then grid
        let surplus_power_kw = surplus_remaining_kw.min(power_kw);
        let grid_power_kw = power_kw - surplus_power_kw;
        *surplus_remaining_kw -= surplus_power_kw;
        (
            surplus_power_kw,
            grid_power_kw,
            grid_power_kw * c_imp * dt_h + surplus_power_kw * c_exp * dt_h,
            grid_power_kw * inputs.g_imp_kgco2_kwh[t] * 1000.0 * dt_h,
        )
    };
    Some(AssetAllocation {
        asset_id: sol.asset_id.clone(),
        power_kw,
        surplus_power_kw,
        grid_power_kw,
        marginal_value,
        cost_eur,
        co2_g,
    })
}

/// Every storage asset's throughput per slot, charge plus discharge [kW], summed over the assets:
/// what the cost breakdown prices as wear. Zeros when there is no storage asset.
fn storage_throughput_kw(sol: &SolveOutput, n: usize) -> Vec<f64> {
    let mut throughput_kw = vec![0.0; n];
    for legs in sol.assets.iter().filter_map(|a| a.storage.as_ref()) {
        for (t, kw) in throughput_kw.iter_mut().enumerate() {
            *kw += legs.charge_kw[t] + legs.discharge_kw[t];
        }
    }
    throughput_kw
}

/// Translate a MILP solution into a `Plan` with per-slot allocations. Every per-asset fact
/// comes from what the asset declared about its own solution; nothing here knows its kind.
#[allow(clippy::too_many_arguments)]
pub(crate) fn translate_to_plan(
    sol: &SolveOutput,
    inputs: &MilpInputs,
    asset_contexts: &[Box<dyn AssetMilpContext>],
    weights: &Phase1Weights,
    planner: &PlannerParams,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    objective: PlannerObjective,
    phase1_cost_eur: f64,
    friction_eur: f64,
    marginal_cost_eur_per_kwh: &[f64],
    phase_report: Option<crate::entities::plan::PlanPhaseReport>,
) -> Plan {
    let step_s = planner.plan_step_s;
    let n = inputs.n;
    let cum_s = &inputs.cum_s;
    let horizon_end = now + Duration::seconds(cum_s[n]);

    let horizon = PlanningHorizon {
        start_time: now,
        end_time: horizon_end,
        step_size_s: step_s,
        num_steps: n,
        far_horizon: horizon_end,
        zones: planner.plan_zones.clone(),
    };

    let claim_order = surplus_claim_order(sol, asset_contexts);
    let storage_throughput_kw = storage_throughput_kw(sol, n);

    let forecast_surplus_kw = inputs.forecast_surplus_kw();
    let mut slots = Vec::with_capacity(n);
    let mut violation_count: usize = 0;

    debug_assert_eq!(
        marginal_cost_eur_per_kwh.len(),
        n,
        "one marginal cost per slot"
    );
    for (t, &marginal_cost) in marginal_cost_eur_per_kwh.iter().enumerate().take(n) {
        let skeleton = slot_skeleton(inputs, &forecast_surplus_kw, now, t);
        let mut surplus_remaining_kw = skeleton.surplus_available_kw;
        let forecasts = forecast_by_asset(asset_contexts, Some(sol), t);

        let allocations: Vec<AssetAllocation> = claim_order
            .iter()
            .filter_map(|&i| {
                allocation(
                    &sol.assets[i],
                    asset_contexts[i].allocation_basis(),
                    t,
                    inputs,
                    &mut surplus_remaining_kw,
                )
            })
            .collect();

        // ── Track violations ────────────────────────────────────────────
        if sol.s_imp_viol_kw[t] > 0.01 || sol.s_exp_viol_kw[t] > 0.01 {
            violation_count += 1;
        }

        // ── Assemble slot ───────────────────────────────────────────────
        slots.push(PlanTimeSlot {
            marginal_cost_import_eur_per_kwh: marginal_cost,
            marginal_cost_export_eur_per_kwh: marginal_cost,
            pv_used_kw: generation_used_kw(&forecasts),
            planned_kw_by_asset: allocations
                .iter()
                .map(|a| (a.asset_id.clone(), a.power_kw))
                .collect(),
            allocations,
            net_import_kw: sol.p_imp_kw[t],
            net_export_kw: sol.p_exp_kw[t],
            // Each asset's own planned state for the slot (T008/T013/T017).
            planned_state_by_asset: sol
                .assets
                .iter()
                .filter_map(|a| Some((a.asset_id.clone(), a.planned_state.get(t)?.clone())))
                .collect(),
            forecast_by_asset: forecasts.into_iter().collect(),
            ..skeleton
        });
    }

    // ── Summary (raw energy economics, no weights) ──────────────────────
    let summary = PlanSummary {
        total_cost_eur: (0..n)
            .map(|t| {
                (inputs.c_imp_eur_kwh[t] * sol.p_imp_kw[t]
                    - inputs.c_exp_eur_kwh[t] * sol.p_exp_kw[t])
                    * inputs.dt_h[t]
            })
            .sum(),
        total_co2_g: (0..n)
            .map(|t| inputs.g_imp_kgco2_kwh[t] * 1000.0 * sol.p_imp_kw[t] * inputs.dt_h[t])
            .sum(),
        total_import_kwh: sol
            .p_imp_kw
            .iter()
            .zip(inputs.dt_h.iter())
            .map(|(p, &d)| p * d)
            .sum(),
        total_export_kwh: sol
            .p_exp_kw
            .iter()
            .zip(inputs.dt_h.iter())
            .map(|(p, &d)| p * d)
            .sum(),
    };

    // ── Cost breakdown (post-hoc from solution × weights) ───────────────
    let cost_breakdown = CostBreakdown {
        c_energy_eur: (0..n)
            .map(|t| {
                weights.w_energy
                    * (inputs.c_imp_eur_kwh[t] * sol.p_imp_kw[t]
                        - inputs.c_exp_eur_kwh[t] * sol.p_exp_kw[t])
                    * inputs.dt_h[t]
            })
            .sum(),
        c_ghg_eur: (0..n)
            .map(|t| weights.w_ghg * inputs.g_imp_kgco2_kwh[t] * sol.p_imp_kw[t] * inputs.dt_h[t])
            .sum(),
        c_grid_eur: (0..n)
            .map(|t| weights.w_grid * (sol.p_imp_kw[t] + sol.p_exp_kw[t]) * inputs.dt_h[t])
            .sum(),
        c_wear_eur: (0..n)
            .map(|t| weights.c_storage_wear_eur_kwh * storage_throughput_kw[t] * inputs.dt_h[t])
            .sum(),
        c_violations_eur: (0..n)
            .map(|t| {
                weights.w_viol
                    * (inputs.pen_imp_eur_kwh * sol.s_imp_viol_kw[t]
                        + inputs.pen_exp_eur_kwh * sol.s_exp_viol_kw[t])
                    * inputs.dt_h[t]
            })
            .sum(),
        v_services_eur: 0.0,
        c_peak_penalty_eur: inputs
            .penalty_rules
            .iter()
            .zip(sol.s_penalty_kw.iter())
            .map(|(rule, windows)| rule.penalty_eur_per_kw * windows.iter().sum::<f64>())
            .sum(),
    };

    // ── Warnings ────────────────────────────────────────────────────────
    let mut warnings = Vec::new();
    // WP4.4 (BL-07) import tariff + BL-17 CO2: independent stale-rate warnings
    // (coverage can end at different times), same stable-text contract —
    // WP4.3's notification dedup keys on the message text.
    for msg in [&inputs.stale_rate_warning, &inputs.co2_stale_rate_warning]
        .into_iter()
        .flatten()
    {
        warnings.push(PlanWarning {
            severity: WarningSeverity::Warning,
            kind: WarningKind::StaleRateEstimate,
            message: msg.clone(),
            suggested_action: None,
        });
    }
    // What each asset can tell before the solve (e.g. a MAX_COST budget shortfall), in planner
    // input order.
    for ctx in asset_contexts {
        warnings.extend(ctx.input_warnings(&inputs.c_imp_eur_kwh));
    }
    if violation_count > 0 {
        warnings.push(PlanWarning {
            severity: WarningSeverity::Warning,
            kind: WarningKind::CapacityViolation,
            message: format!(
                "Grid capacity violation in {violation_count} slot(s) — solver used slack"
            ),
            suggested_action: None,
        });
    }
    // WP6.3 (BL-09): a window still exceeding its penalty threshold after
    // solving means the penalty was accepted (cheaper than reallocating).
    for (rule, windows) in inputs.penalty_rules.iter().zip(sol.s_penalty_kw.iter()) {
        for (w, &slack_kw) in windows.iter().enumerate() {
            if slack_kw > 0.01 {
                let window_start =
                    now + Duration::seconds(w as i64 * rule.measurement_window_s as i64);
                let window_end =
                    now + Duration::seconds((w as i64 + 1) * rule.measurement_window_s as i64);
                let peak_kw = rule.threshold_kw + slack_kw;
                warnings.push(PlanWarning {
                    severity: WarningSeverity::Warning,
                    kind: WarningKind::PeakPenaltyExceeded,
                    message: format!(
                        "Penalty rule '{}' exceeded in window {}–{}: peak {:.1} kW > threshold {:.1} kW (€{:.2} accepted)",
                        rule.rule_id,
                        window_start.format("%H:%M"),
                        window_end.format("%H:%M"),
                        peak_kw,
                        rule.threshold_kw,
                        rule.penalty_eur_per_kw * slack_kw
                    ),
                    suggested_action: Some(
                        "Increase available flexible capacity or raise the threshold if this recurs"
                            .to_string(),
                    ),
                });
            }
        }
    }

    // What each asset reports about its own solution, in planner input order.
    for asset in &sol.assets {
        warnings.extend(asset.warnings.iter().cloned());
    }
    // ── Assemble plan ───────────────────────────────────────────────────
    let session_flexibility =
        build_session_flexibility(asset_contexts, inputs, planner, now, Some(&slots));
    let penalty_rules_active = active_penalty_rules(&inputs.penalty_rules);
    Plan {
        id: Uuid::new_v4(),
        created_at: now,
        trigger,
        horizon,
        planned_asset_ids: planned_asset_ids(asset_contexts),
        slots,
        summary,
        session_flexibility,
        warnings,
        objective,
        objective_eur: phase1_cost_eur,
        friction_eur,
        cost_breakdown,
        solve_status: map_solve_status(sol.status),
        phase_report,
        penalty_rules_active,
        solver_ms: None,
        mip_gap_target: Some(planner.mip_gap_target),
    }
}
