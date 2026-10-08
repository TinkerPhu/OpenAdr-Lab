//! Guard for the planner's three solves: whatever restructuring happens around them, each must keep
//! handing HiGHS the same model, and HiGHS must keep returning the same answer.
//!
//! Each scenario runs `solve_milp_two_phase` (phase 1, phase 2, then the marginal-cost pass) and
//! records, through `probe!` (see `model_probe.rs`), every variable definition, the objective(s),
//! the warm start and every constraint in the order they were added, plus a full-precision digest
//! of what came back. The result is compared with `golden/model_fingerprints.txt`.
//!
//! The golden was captured from the code *before* the solver skeleton was shared
//! (`model_skeleton.rs`), so a difference is a change in what the solver is given or returns, never
//! a matter of taste. If one appears, find the first differing line (it names the scenario and the
//! solve) and fix the code. Regenerate only for an intended model change:
//! `UPDATE_MODEL_GOLDEN=1 cargo test -p ven-app model_fingerprint`.
//!
//! Scenarios are small enough to solve to optimality in well under their time limits, so no
//! wall-clock budget decides an outcome and the records are reproducible.

use super::*;

use crate::controller::milp_planner::model_probe::recorder;
use crate::controller::milp_planner::solver_phase2::solve_milp_two_phase;
use crate::entities::device_session::{EvSession, EvSessionOrigin, ShiftableLoad};
use crate::entities::planner_params::PenaltyRuleParams;
use rand::{rngs::StdRng, SeedableRng};

const GOLDEN: &str = include_str!("golden/model_fingerprints.txt");

struct Fixture {
    name: &'static str,
    inputs: MilpInputs,
    ctxs: Vec<Box<dyn crate::controller::milp_planner::AssetMilpContext>>,
    p1w: Phase1Weights,
    p2w: Phase2Weights,
    epsilon_eur: f64,
    timeout_s: f64,
    phase2_timeout_s: f64,
}

/// A snapshot of the profile's real assets with a fixed RNG and clock, so nothing in it depends on
/// when or where the test runs.
fn snapshot(profile: &Profile) -> SimSnapshot {
    crate::simulator::SimState::from_params_seeded(
        &profile.assets,
        fixed_now(),
        StdRng::seed_from_u64(7),
    )
    .to_sim_snapshot()
}

fn ev_session() -> EvSession {
    let now = fixed_now();
    EvSession {
        mode: Default::default(),
        origin: EvSessionOrigin::UserRequest,
        id: uuid::Uuid::from_u128(1),
        target_soc_frac: 0.8,
        window_start: now,
        expected_trip_distance_km: None,
        expected_return_time: None,
        departure_time: now + Duration::hours(2),
        soft_deadline: false,
        budget_eur: None,
        comfort_rates: vec![],
        created_at: now,
        updated_at: now,
    }
}

fn shiftable_load() -> ShiftableLoad {
    let now = fixed_now();
    ShiftableLoad {
        id: uuid::Uuid::from_u128(2),
        asset_id: "wm".to_string(),
        power_kw: 2.0,
        duration_min: 30,
        earliest_start: now,
        latest_end: now + Duration::minutes(65),
        mode: Default::default(),
        created_at: now,
        updated_at: now,
    }
}

fn assemble(
    name: &'static str,
    profile: &Profile,
    tariffs: &TariffTimeSeries,
    cap: &OadrCapacityState,
    ev: Option<&EvSession>,
    shiftables: &[ShiftableLoad],
    pv_override: Option<f64>,
) -> Fixture {
    let now = fixed_now();
    let mut sim = snapshot(profile);
    set_ev_plugged(&mut sim, true);
    let mut ctxs = build_asset_contexts(profile, &sim, now, ev, None, tariffs);
    push_shiftable_load_contexts(&mut ctxs, shiftables, profile, now);
    let inputs =
        build_milp_inputs_with_override(&ctxs, tariffs, cap, profile, now, &[], None, pv_override);
    let p1w = build_phase1_weights(profile, PlannerObjective::MinCost);
    let p2w = build_phase2_weights(&inputs, &profile.planner);
    Fixture {
        name,
        inputs,
        ctxs,
        p1w,
        p2w,
        epsilon_eur: profile.planner.phase2_epsilon_eur,
        timeout_s: profile.planner.solver_timeout_s as f64,
        phase2_timeout_s: profile.planner.phase2_solver_timeout_s as f64,
    }
}

fn keep(profile: &mut Profile, keep: fn(&AssetProfile) -> bool) {
    profile.assets.retain(keep);
}

/// Every branch of the three solves: each asset alone and together, both cross-asset interactions,
/// shiftable loads, a penalty rule, import and export slack, and phase 2 skipped.
fn scenarios() -> Vec<Fixture> {
    let flat = make_tariffs(0.25, 0.08, 300.0);
    let two_zone = make_two_zone_tariffs(0.10, 0.40);
    let uncapped = no_capacity();
    let session = ev_session();
    let mut out = Vec::new();

    out.push(assemble(
        "all_assets_default",
        &make_profile(),
        &flat,
        &uncapped,
        Some(&session),
        &[],
        None,
    ));

    let mut malus = make_profile();
    malus.planner.c_ctrl_imp_malus_eur_kwh = 0.22;
    out.push(assemble(
        "all_assets_both_interactions",
        &malus,
        &two_zone,
        &uncapped,
        Some(&session),
        &[],
        None,
    ));

    let mut battery = make_profile();
    keep(&mut battery, |a| {
        matches!(
            a,
            AssetProfile::Battery(_) | AssetProfile::Pv(_) | AssetProfile::BaseLoad(_)
        )
    });
    out.push(assemble(
        "battery_with_pv",
        &battery,
        &two_zone,
        &uncapped,
        None,
        &[],
        None,
    ));

    let mut ev_only = make_profile();
    keep(&mut ev_only, |a| {
        matches!(a, AssetProfile::Ev(_) | AssetProfile::BaseLoad(_))
    });
    out.push(assemble(
        "ev_with_session",
        &ev_only,
        &two_zone,
        &uncapped,
        Some(&session),
        &[],
        None,
    ));

    out.push(assemble(
        "heater_only",
        &make_heater_only_profile(None, 18.0, 23.0, 20.0),
        &two_zone,
        &uncapped,
        None,
        &[],
        None,
    ));

    let mut shift = make_profile_1800s();
    keep(&mut shift, |a| matches!(a, AssetProfile::BaseLoad(_)));
    out.push(assemble(
        "shiftable_load",
        &shift,
        &flat,
        &uncapped,
        None,
        &[shiftable_load()],
        None,
    ));

    let mut penalised = assemble(
        "penalty_rule",
        &make_profile(),
        &flat,
        &uncapped,
        Some(&session),
        &[],
        None,
    );
    penalised.inputs.penalty_rules = vec![PenaltyRuleParams {
        rule_id: "peak-3kw".to_string(),
        threshold_kw: 3.0,
        measurement_window_s: 3600,
        penalty_eur_per_kw: 5.0,
    }];
    out.push(penalised);

    let squeezed = OadrCapacityState {
        import_limit_kw: Some(1.0),
        export_limit_kw: Some(0.5),
        ..no_capacity()
    };
    out.push(assemble(
        "import_and_export_slack",
        &make_profile(),
        &flat,
        &squeezed,
        Some(&session),
        &[],
        Some(1.0),
    ));

    let mut no_phase2 = make_profile();
    no_phase2.planner.phase2_epsilon_eur = 0.0;
    out.push(assemble(
        "phase2_skipped",
        &no_phase2,
        &flat,
        &uncapped,
        Some(&session),
        &[],
        None,
    ));

    out
}

/// Everything the solver returned, to full `f64` precision. Written field by field because
/// `mode_decisions.y_shift` is a `HashMap`, whose `Debug` order differs from run to run.
fn solution_text(out: &SolveOutput) -> String {
    let mut y_shift: Vec<_> = out.mode_decisions.y_shift.iter().collect();
    y_shift.sort_by(|a, b| a.0.cmp(b.0));
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        out.status,
        out.objective_eur,
        out.p_imp_kw,
        out.p_exp_kw,
        out.p_pv_used_kw,
        out.p_bat_ch_kw,
        out.p_bat_dis_kw,
        out.p_ev_kw,
        out.soc_ev,
        out.ev_shortfall_kwh,
        out.y_heat,
        out.e_bat_kwh,
        out.s_imp_viol_kw,
        out.s_exp_viol_kw,
        out.z_ev_on,
        out.e_ev_extra,
        out.e_seg_kwh,
        out.z_heat_ready,
        out.e_heat_tank_kwh,
        out.p_shiftable_kw,
        out.s_penalty_kw,
        out.mode_decisions.u_bat,
        out.mode_decisions.z_heat_ready,
        y_shift,
    )
}

/// The records of one scenario, headed by its name.
fn record(f: &Fixture) -> Vec<String> {
    let (result, mut lines) = recorder::capture(|| {
        solve_milp_two_phase(
            &f.inputs,
            &f.p1w,
            &f.p2w,
            f.epsilon_eur,
            &f.ctxs,
            f.timeout_s,
            f.phase2_timeout_s,
        )
    });
    let (winning, c_star, friction_eur, marginal, _phase_report) =
        result.unwrap_or_else(|e| panic!("scenario {} must be solvable: {e}", f.name));
    lines.insert(0, format!("## {}", f.name));
    lines.push(format!(
        "result c_star={c_star:?} friction={friction_eur:?} marginal={marginal:?}"
    ));
    // In the clear, so a scenario that ever stops reaching optimality is visible in the golden
    // instead of hiding inside the digest.
    lines.push(format!("status {:?}", winning.status));
    lines.push(format!(
        "solution {}",
        recorder::digest(&solution_text(&winning))
    ));
    lines
}

fn record_all() -> Vec<String> {
    scenarios().iter().flat_map(record).collect()
}

fn golden_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/controller/milp_planner/tests/golden/model_fingerprints.txt")
}

#[test]
fn solve_models_match_the_recorded_golden() {
    let actual = record_all();

    if std::env::var_os("UPDATE_MODEL_GOLDEN").is_some() {
        std::fs::write(golden_path(), actual.join("\n") + "\n").expect("write golden");
        eprintln!("model fingerprint golden rewritten: {} lines", actual.len());
        return;
    }

    let expected: Vec<&str> = GOLDEN.lines().collect();
    let mut scenario = "(before the first scenario)";
    for (i, got) in actual.iter().enumerate() {
        if let Some(name) = got.strip_prefix("## ") {
            scenario = name;
        }
        let want = expected.get(i).copied();
        assert_eq!(
            Some(got.as_str()),
            want,
            "model differs at record {i} of scenario `{scenario}`: either the solver is given a \
             different model or it returned a different answer. `sorted=` equal but `sent=` \
             different means only the order inside one expression changed."
        );
    }
    assert_eq!(
        actual.len(),
        expected.len(),
        "the golden holds {} records but this run produced {}",
        expected.len(),
        actual.len()
    );
}

/// The golden is only worth anything if a run reproduces itself.
#[test]
fn solve_models_are_the_same_on_every_run() {
    let scenario = scenarios()
        .into_iter()
        .find(|f| f.name == "battery_with_pv")
        .expect("scenario exists");
    assert_eq!(record(&scenario), record(&scenario));
}
