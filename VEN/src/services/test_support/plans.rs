//! A flat plan fixture: `slots` equal slots of `step_s` seconds from `start`, uniform tariffs,
//! no allocations. Shared by the tests that need "a plan" but not any particular one.

use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap as Map;
use uuid::Uuid;

use crate::entities::asset::PlanTrigger;
use crate::entities::plan::{Plan, PlanTimeSlot, PlanZone, PlanningHorizon, SolveStatus};
use crate::entities::planner_params::PlannerObjective;

pub fn flat_plan(step_s: u64, slots: usize, start: DateTime<Utc>) -> Plan {
    let horizon = PlanningHorizon {
        start_time: start,
        end_time: start + Duration::seconds((step_s * slots as u64) as i64),
        step_size_s: step_s,
        num_steps: slots,
        far_horizon: start + Duration::seconds((step_s * slots as u64) as i64),
        zones: vec![PlanZone { step_s, slots }],
    };
    let plan_slots: Vec<PlanTimeSlot> = (0..slots)
        .map(|i| PlanTimeSlot {
            slot_index: i,
            start: start + Duration::seconds((step_s * i as u64) as i64),
            end: start + Duration::seconds((step_s * (i + 1) as u64) as i64),
            import_tariff_eur_kwh: 0.25,
            export_tariff_eur_kwh: 0.08,
            co2_g_kwh: 300.0,
            grid_effective_cost: 0.25,
            marginal_cost_import_eur_per_kwh: 0.25,
            marginal_cost_export_eur_per_kwh: 0.25,
            rate_estimated: false,
            import_cap_kw: 25.0,
            export_cap_kw: 10.0,
            allocations: vec![],
            pv_forecast_kw: 0.0,
            pv_used_kw: 0.0,
            baseline_kw: 0.0,
            surplus_available_kw: 0.0,
            net_import_kw: 0.0,
            net_export_kw: 0.0,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
            planned_kw_by_asset: Map::new(),
            planned_state_by_asset: Map::new(),
            bat_charge_kw: 0.0,
            bat_discharge_kw: 0.0,
        })
        .collect();
    Plan {
        id: Uuid::new_v4(),
        created_at: start,
        trigger: PlanTrigger::Periodic,
        objective: PlannerObjective::MinCost,
        horizon,
        slots: plan_slots,
        objective_eur: 0.0,
        friction_eur: 0.0,
        cost_breakdown: Default::default(),
        soc_trajectory_kwh: vec![],
        summary: Default::default(),
        envelopes: vec![],
        warnings: vec![],
        solve_status: SolveStatus::Optimal,
        phase_report: None,
        penalty_rules_active: vec![],
        solver_ms: None,
        mip_gap_target: None,
    }
}
