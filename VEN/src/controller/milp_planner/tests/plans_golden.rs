//! Guard for the plan the planner hands out: whatever restructuring happens between the solution
//! and the `Plan`, every scenario must keep producing the same plan, field for field.
//!
//! `model_fingerprint.rs` proves the solver is given the same model and returns the same answer;
//! this proves the answer is still translated into the same plan. Each scenario of
//! `model_fingerprint::scenarios()`, plus the ones below that exercise what those do not (a heater
//! target, all three kinds of session at once, an EV budget warning, an EV firm shortfall, the
//! fallback plan), is solved, translated
//! and recorded as the `Plan` JSON the BFF and the UI receive, with object keys sorted and floats
//! at full precision. The result is compared with `golden/plans.txt`.
//!
//! Only the values that cannot repeat are left out: the plan's random `id` and the wall-clock
//! solve times. Floats are compared to 1e-9 relative (`golden_compare.rs`), so the golden holds on
//! x86 and on aarch64. Regenerate only for an intended plan change:
//! `UPDATE_PLANS_GOLDEN=1 cargo test -p ven-app plans_golden`.

use super::golden_compare::assert_matches_golden;
use super::model_fingerprint::{
    assemble_with, ev_session, keep, scenarios, shiftable_load, Fixture,
};
use super::*;

use crate::controller::milp_planner::solver_phase2::solve_milp_two_phase;
use crate::entities::design_vocabulary::UserRequestMode;
use crate::entities::device_session::{EvSession, HeaterTarget};
use serde_json::Value;

const GOLDEN: &str = include_str!("golden/plans.txt");

/// What the fingerprint scenarios leave out: a heater target, an EV whose budget cannot buy its
/// target, and an EV whose firm target cannot be reached before departure.
fn plan_only_scenarios() -> Vec<Fixture> {
    let now = fixed_now();
    let flat = make_tariffs(0.25, 0.08, 300.0);
    let two_zone = make_two_zone_tariffs(0.10, 0.40);
    let uncapped = no_capacity();
    let mut out = Vec::new();

    let target = HeaterTarget {
        asset_id: "heater".into(),
        id: uuid::Uuid::from_u128(3),
        target_temp_c: 20.5,
        ready_by: now + Duration::minutes(90),
        mode: Default::default(),
        comfort_rates: vec![],
        created_at: now,
        updated_at: now,
    };
    out.push(assemble_with(
        "heater_with_target",
        &make_heater_only_profile(None, 18.0, 23.0, 20.0),
        &two_zone,
        &uncapped,
        None,
        Some(&target),
        &[],
        None,
    ));

    out.push(assemble_with(
        "all_sessions",
        &make_profile(),
        &two_zone,
        &uncapped,
        Some(&ev_session()),
        Some(&target),
        &[shiftable_load()],
        None,
    ));

    let mut ev_only = make_profile();
    keep(&mut ev_only, |a| {
        matches!(a, AssetParams::Ev(_) | AssetParams::BaseLoad(_))
    });
    let over_budget = EvSession {
        mode: UserRequestMode::MaxCost,
        budget_eur: Some(0.01),
        ..ev_session()
    };
    out.push(assemble_with(
        "ev_budget_too_low",
        &ev_only,
        &flat,
        &uncapped,
        Some(&over_budget),
        None,
        &[],
        None,
    ));

    let unreachable = EvSession {
        target_soc_frac: 1.0,
        departure_time: now + Duration::minutes(15),
        ..ev_session()
    };
    out.push(assemble_with(
        "ev_firm_shortfall",
        &ev_only,
        &flat,
        &uncapped,
        Some(&unreachable),
        None,
        &[],
        None,
    ));

    out
}

/// The scenario solved and translated, exactly as `run_planner` does after building its inputs.
fn solved_plan(f: &Fixture) -> Plan {
    let (sol, phase1_cost_eur, friction_eur, marginal, phase_report) = solve_milp_two_phase(
        &f.inputs,
        &f.p1w,
        f.epsilon_eur,
        &f.ctxs,
        f.timeout_s,
        f.phase2_timeout_s,
    )
    .unwrap_or_else(|e| panic!("scenario {} must be solvable: {e}", f.name));
    translate_to_plan(
        &sol,
        &f.inputs,
        &f.ctxs,
        &f.p1w,
        &f.profile.planner,
        fixed_now(),
        PlanTrigger::Periodic,
        PlannerObjective::MinCost,
        phase1_cost_eur,
        friction_eur,
        &marginal,
        Some(phase_report),
    )
}

/// The plan the solver-failure path returns for the same scenario.
fn fallback(f: &Fixture) -> Plan {
    fallback_plan(
        &f.profile.planner,
        fixed_now(),
        PlanTrigger::Periodic,
        &f.ctxs,
        Some(&f.inputs),
        "MILP solver failed: recorded fallback".to_string(),
        PlannerObjective::MinCost,
    )
}

/// `v` on one line, object keys sorted, numbers as serde_json writes them (shortest round-trip).
fn canonical(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{k:?}:{}", canonical(&map[k])))
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

/// One line per top-level plan field and one per slot, so a difference names its field and slot.
fn plan_lines(name: &str, plan: &Plan) -> Vec<String> {
    let mut v = serde_json::to_value(plan).expect("a plan serializes");
    let obj = v.as_object_mut().expect("a plan is an object");
    obj.remove("id");
    obj.remove("solver_ms");
    if let Some(Value::Object(report)) = obj.get_mut("phase_report") {
        report.remove("phase1_ms");
        report.remove("phase2_ms");
    }
    let mut keys: Vec<String> = obj.keys().cloned().collect();
    keys.sort();
    let mut lines = vec![format!("## {name}")];
    for k in keys {
        match &obj[&k] {
            Value::Array(slots) if k == "slots" => {
                lines.extend(slots.iter().map(|s| format!("slot {}", canonical(s))))
            }
            value => lines.push(format!("{k} {}", canonical(value))),
        }
    }
    lines
}

fn record_all() -> Vec<String> {
    let mut lines = Vec::new();
    for f in scenarios().iter().chain(plan_only_scenarios().iter()) {
        lines.extend(plan_lines(f.name, &solved_plan(f)));
    }
    // The fallback path reports session flexibility without a solution; the scenario with an
    // EV session, a heater target and a shiftable load exercises every branch of it.
    let all = plan_only_scenarios()
        .into_iter()
        .find(|f| f.name == "all_sessions")
        .expect("scenario exists");
    lines.extend(plan_lines("fallback_all_sessions", &fallback(&all)));
    lines
}

fn golden_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/controller/milp_planner/tests/golden/plans.txt")
}

#[test]
fn plans_match_the_recorded_golden() {
    let actual = record_all();

    if std::env::var_os("UPDATE_PLANS_GOLDEN").is_some() {
        std::fs::write(golden_path(), actual.join("\n") + "\n").expect("write golden");
        eprintln!("plan golden rewritten: {} lines", actual.len());
        return;
    }

    assert_matches_golden(&actual, GOLDEN, "plan");
}

/// A plan names every asset it scheduled, in planner input order, and the fallback plan for the
/// same contexts names the same ones: a reader asks the plan, not its slots, which assets it covers.
#[test]
fn solved_and_fallback_plans_name_the_assets_they_planned() {
    let all = plan_only_scenarios()
        .into_iter()
        .find(|f| f.name == "all_sessions")
        .expect("scenario exists");
    // The planned assets are the dispatched ones; PV and base load are only forecast.
    let ids: Vec<String> = all
        .ctxs
        .iter()
        .filter(|c| {
            matches!(
                c.participation(),
                crate::controller::milp_planner::Participation::Dispatched
            )
        })
        .map(|c| c.asset_id().to_string())
        .collect();
    assert!(ids.len() >= 3, "the scenario plans several assets: {ids:?}");
    assert_eq!(solved_plan(&all).planned_asset_ids, ids);
    assert_eq!(fallback(&all).planned_asset_ids, ids);
}
