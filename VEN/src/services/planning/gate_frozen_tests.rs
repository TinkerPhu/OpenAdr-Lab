//! Between two plans of planning objectives the adoption gate must decide exactly as it did
//! before the opportunistic objective added a rule for regime changes (openspec
//! `opportunistic-objective` D0, D7). `frozen_gate` is a verbatim copy of
//! `evaluate_acceptance_gate` as it stood then (minus its log line); the test compares both on
//! random pairs of planning plans: solved and failed, aged, expired, cheaper and dearer, with
//! and without heater relay switches, under periodic and hard triggers.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, TimeZone, Utc};
use rand::{Rng, SeedableRng};

use super::{count_heater_switches, evaluate_acceptance_gate};
use crate::entities::asset::PlanTrigger;
use crate::entities::plan::{AssetAllocation, Plan, SolveStatus};
use crate::entities::planner_params::PlannerObjective;

/// `evaluate_acceptance_gate` before `opportunistic-objective`. Do not edit.
#[allow(clippy::too_many_arguments)]
fn frozen_gate(
    current: Option<&Plan>,
    new_plan: &Plan,
    trigger: &PlanTrigger,
    threshold_eur: f64,
    decay_s: f64,
    gate_switch_penalty_eur: f64,
    heater_stage_sizes_kw: &BTreeMap<String, f64>,
    now: DateTime<Utc>,
) -> bool {
    if let Some(current) = current {
        match (
            current.solve_status.solved(),
            new_plan.solve_status.solved(),
        ) {
            (true, false) => return false,
            (false, true) => return true,
            _ => {}
        }
    }
    let is_hard_trigger = !matches!(trigger, PlanTrigger::Periodic);
    if is_hard_trigger || (threshold_eur == 0.0 && gate_switch_penalty_eur == 0.0) {
        return true;
    }
    let Some(current) = current else {
        return true;
    };
    let current_expired = current
        .all_slots()
        .map(|s| s.end)
        .max()
        .is_some_and(|end| end <= now);
    if current_expired {
        return true;
    }
    let elapsed_s = (now - current.created_at).num_seconds().max(0) as f64;
    let decay_factor = if decay_s > 0.0 {
        (1.0 - elapsed_s / decay_s).max(0.0)
    } else {
        1.0
    };
    let effective_threshold = threshold_eur * decay_factor;
    let fully_decayed = decay_s > 0.0 && elapsed_s >= decay_s;
    let current_total = current.objective_eur + current.friction_eur;
    let new_total = new_plan.objective_eur + new_plan.friction_eur;
    let improvement = current_total - new_total;
    let switch_surcharge = if gate_switch_penalty_eur > 0.0 {
        let extra = (count_heater_switches(new_plan, now, heater_stage_sizes_kw)
            - count_heater_switches(current, now, heater_stage_sizes_kw))
        .max(0.0);
        extra * gate_switch_penalty_eur
    } else {
        0.0
    };
    fully_decayed || improvement > effective_threshold + switch_surcharge
}

const PLANNING_OBJECTIVES: [PlannerObjective; 6] = [
    PlannerObjective::MinCost,
    PlannerObjective::MinGhg,
    PlannerObjective::MinGrid,
    PlannerObjective::MinImport,
    PlannerObjective::MaxRevenue,
    PlannerObjective::Custom,
];

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap()
}

fn random_plan(rng: &mut rand::rngs::StdRng) -> Plan {
    let age_s = [0, 60, 600, 1500, 4000][rng.gen_range(0..5)];
    let created = now() - Duration::seconds(age_s);
    // Expired: the whole plan ended before `now`.
    let start = if rng.gen_bool(0.15) {
        now() - Duration::hours(3)
    } else {
        created
    };
    let mut plan = crate::services::test_support::plans::flat_plan(300, 6, start);
    plan.created_at = created;
    plan.objective = PLANNING_OBJECTIVES[rng.gen_range(0..PLANNING_OBJECTIVES.len())];
    plan.objective_eur = rng.gen_range(-2.0..2.0);
    plan.friction_eur = [0.0, 0.0, 0.05, 0.3][rng.gen_range(0..4)];
    if rng.gen_bool(0.15) {
        plan.solve_status = SolveStatus::Infeasible;
    }
    for slot in &mut plan.slots {
        let kw = [0.0, 1.5, 3.0][rng.gen_range(0..3)];
        if kw > 0.0 {
            slot.allocations = vec![AssetAllocation {
                asset_id: "heater".into(),
                power_kw: kw,
                surplus_power_kw: 0.0,
                grid_power_kw: kw,
                marginal_value: 0.25,
                cost_eur: 0.0,
                co2_g: 0.0,
            }];
            slot.planned_kw_by_asset = [("heater".to_string(), kw)].into();
        }
    }
    plan
}

#[test]
fn gate_matches_the_frozen_gate_between_planning_plans() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(20261016);
    let stages: BTreeMap<String, f64> = [("heater".to_string(), 1.5)].into();
    for case in 0..10_000 {
        let current = rng.gen_bool(0.85).then(|| random_plan(&mut rng));
        let new_plan = random_plan(&mut rng);
        let trigger = if rng.gen_bool(0.6) {
            PlanTrigger::Periodic
        } else {
            PlanTrigger::UserRequest
        };
        let threshold = [0.0, 0.2][rng.gen_range(0..2)];
        let decay = [0.0, 1500.0][rng.gen_range(0..2)];
        let penalty = [0.0, 0.05][rng.gen_range(0..2)];
        let args = (
            current.as_ref(),
            &new_plan,
            &trigger,
            threshold,
            decay,
            penalty,
        );
        let got = evaluate_acceptance_gate(
            args.0,
            args.1,
            args.2,
            args.3,
            args.4,
            args.5,
            &stages,
            now(),
        );
        let want = frozen_gate(
            args.0,
            args.1,
            args.2,
            args.3,
            args.4,
            args.5,
            &stages,
            now(),
        );
        assert_eq!(got, want, "case {case}");
    }
}
