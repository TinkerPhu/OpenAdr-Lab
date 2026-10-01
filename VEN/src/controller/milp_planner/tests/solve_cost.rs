//! GB-40 benchmark: what a heater costs the MILP, measured rather than argued.
//!
//! The fleet measurement (`docs/history/fleet_run_journal.md`) found VENs with a
//! heater average 84.2 s per solve against 18.0 s without — 4.7× — with the six
//! slowest pinned to the `solver_timeout_s` two-phase ceiling. This reproduces
//! that locally and isolates the heater as the variable: the same ven-3-shaped
//! site, same three-zone 288-slot grid, same tariffs, solved with and without
//! the heater asset.
//!
//! Ignored by default — it deliberately runs a production-sized solve and is a
//! measurement, not an assertion about correctness. Run it explicitly:
//!
//! ```text
//! wsl cargo test -p ven-app --release solve_cost -- --ignored --nocapture
//! ```
//!
//! Anything changing the heater formulation (dwell-time constraints, a
//! tier-bounded continuous power variable — see GB-40) should be judged against
//! the numbers this prints, before and after.

use super::*;
use std::time::Instant;

/// A ven-3-shaped site on the production three-zone grid, optionally without
/// the heater. Everything except the heater is held identical between the two
/// variants so the delta is attributable.
fn bench_profile(with_heater: bool) -> Profile {
    let volume_l = 200.0_f64;
    let thermal_mass = volume_l * 4.186 / 3600.0;

    let mut assets: Vec<AssetProfile> = Vec::new();
    if with_heater {
        assets.push(AssetProfile::Heater(HeaterParams {
            id: "heater".into(),
            max_kw: 6.0,
            power_stages: 2,
            temp_initial_c: 47.82,
            temp_min_c: 45.0,
            temp_max_c: 60.0,
            temp_safety_max_c: 60.0,
            thermostat_delta_c: 3.0,
            thermal_mass_kwh_per_c: thermal_mass,
            k_loss_kw_per_c: 0.005,
            draw_kw: 0.3,
            switching_penalty_eur: 0.50,
            c_terminal_eur_kwh: None,
        }));
    }
    assets.push(AssetProfile::Ev(EvParams {
        id: "ev".into(),
        max_charge_kw: 11.0,
        max_discharge_kw: 0.0,
        initial_soc: 0.30,
        battery_kwh: 75.0,
        soc_target: 0.80,
        default_charge_kw: 0.0,
        min_charge_kw: 0.0,
        response_delay_s: 10.0,
        v2g_capable: false,
        usage_sim: None,
    }));
    assets.push(AssetProfile::Pv(PvParams {
        id: "pv".into(),
        rated_kw: 6.0,
        inverter_max_kw: 6.0,
        co2_g_kwh: 0.0,
    }));
    assets.push(AssetProfile::BaseLoad(BaseLoadParams {
        id: "base_load".into(),
        baseline_kw: 0.6,
        spikes: vec![],
    }));

    Profile {
        assets,
        simulator: SimulatorConfig,
        planner: PlannerConfig {
            plan_step_s: 300,
            plan_horizon_h: 48,
            plan_zones: vec![
                crate::entities::plan::PlanZone {
                    step_s: 300,
                    slots: 96,
                },
                crate::entities::plan::PlanZone {
                    step_s: 600,
                    slots: 96,
                },
                crate::entities::plan::PlanZone {
                    step_s: 900,
                    slots: 96,
                },
            ],
            c_ctrl_imp_malus_eur_kwh: 0.22,
            phase2_epsilon_eur: 0.17,
            ..PlannerConfig::default()
        },
        grid: GridConfig {
            max_import_kw: 25.0,
            max_export_kw: 10.0,
        },
        packets: vec![],
    }
}

fn time_one_solve(with_heater: bool) -> (f64, usize) {
    let now = fixed_now();
    let profile = bench_profile(with_heater);
    let mut sim = make_snap_from_profile(&profile);
    if with_heater {
        // Emergency thermostat state: forces initial_z_full=1.0, the live
        // condition the fleet VENs were actually in.
        set_heater_power(&mut sim, 6.0);
    }
    let tariffs = make_tariffs(0.25, 0.08, 300.0);

    let started = Instant::now();
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, None, None, &tariffs),
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        None,
        None,
        &[],
        None,
        None,
    );
    (started.elapsed().as_secs_f64(), plan.slots.len())
}

#[test]
#[ignore = "GB-40 benchmark: production-sized solve, run explicitly with --ignored --nocapture"]
fn bench_heater_solve_cost() {
    // One untimed warm-up so solver/allocator start-up does not land in the
    // first measured figure.
    let _ = time_one_solve(false);

    let (without_s, slots_without) = time_one_solve(false);
    let (with_s, slots_with) = time_one_solve(true);

    println!("\n=== GB-40: heater MILP solve cost ===");
    println!("  slots (grid):     {slots_without} without heater / {slots_with} with");
    println!("  without heater:   {without_s:8.2} s");
    println!("  with heater:      {with_s:8.2} s");
    if without_s > 0.0 {
        println!("  ratio:            {:8.2}x", with_s / without_s);
    }
    println!(
        "  fleet reference:  4.7x (84.2 s vs 18.0 s, 20 VENs, docs/history/fleet_run_journal.md)\n"
    );

    // Not asserting a ratio: this is a measurement, and pinning a threshold
    // here would make it fail on a slower machine for reasons unrelated to the
    // formulation. The heater must at least cost *something* extra, though --
    // if it ever stops doing so, the benchmark has stopped measuring what it
    // claims to.
    assert!(
        with_s > without_s,
        "expected the heater variant to be slower; got {with_s:.2}s with vs {without_s:.2}s without"
    );
}

// ── GB-40 A/B harness ────────────────────────────────────────────────────────
//
// Ten fixed start conditions, held in a const so every arm of the experiment
// provably solves the same instances. They vary tank fill, the stage the
// hardware is already in, and the price signal — ten genuinely different MILP
// instances rather than repeats of a few, which is what lets a per-gap mean
// across the set distinguish a real quality/gap relationship from per-instance
// branch-and-bound incumbent variance (the original 5-instance sweep's +4-7%
// noisy band above 13% could not tell those apart; see GB-40 2026-08-29).
// Extended from the original 5 (kept as the first five entries, so this run
// is directly comparable to that one) to cover both temperature extremes
// (T_min=45/T_max=60 boundaries, not just the mid-band the original 5 leaned
// toward), all three power stages evenly (0/3/6 kW), and a wider price range
// (0.10-0.60 vs the original 0.25-0.40), including two intentionally
// "contradictory" combinations (full power already committed near T_max;
// full power at a warm tank with cheap import) that a real fleet VEN can
// land in mid-transition and that the original 5 didn't exercise.
//
// (tank °C, initial heater kW, import €/kWh, label)
const HEATER_VARIANTS: [(f64, f64, f64, &str); 10] = [
    (47.82, 6.0, 0.25, "cool tank, emergency full"),
    (46.0, 0.0, 0.25, "near T_min, starting off"),
    (50.0, 3.0, 0.25, "mid-band, mid stage"),
    (55.0, 0.0, 0.25, "warm tank, little need"),
    (47.82, 6.0, 0.40, "cool tank, expensive power"),
    (45.5, 3.0, 0.25, "near T_min, mid stage"),
    (59.5, 6.0, 0.25, "near T_max, emergency full"),
    (52.0, 0.0, 0.15, "mid-band, off, cheap power"),
    (48.0, 3.0, 0.60, "cool-ish, mid stage, very expensive power"),
    (56.0, 6.0, 0.10, "warm tank, full power, very cheap power"),
];

struct VariantResult {
    p1_s: f64,
    p1_status: String,
    p1_objective: f64,
    p2_s: f64,
    p2_status: String,
}

/// Solve one variant at a given MIP gap tolerance, phases run separately.
///
/// `solve_milp_two_phase` returns the *winning* (phase-2) solution, so a single
/// status hides which phase actually ran out of clock — a conflation that
/// already produced one wrong conclusion in this investigation. Phase 1 is where
/// cost optimality is decided, so it is the phase whose status and objective
/// matter here.
fn solve_at(
    temp_c: f64,
    initial_kw: f64,
    import_eur_kwh: f64,
    mip_gap_target: f64,
) -> VariantResult {
    let now = fixed_now();
    let mut profile = bench_profile(true);
    profile.planner.mip_gap_target = mip_gap_target;
    let mut sim = make_snap_from_profile(&profile);
    set_heater_temp(&mut sim, temp_c);
    set_heater_power(&mut sim, initial_kw);
    let tariffs = make_tariffs(import_eur_kwh, 0.08, 300.0);
    let cap = no_capacity();

    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p2w = build_phase2_weights(&inputs, &profile.planner);
    let timeout = profile.planner.solver_timeout_s as f64;

    let t = Instant::now();
    let p1 = solve_phase1(&inputs, &p1w, &ctxs, timeout).expect("phase 1 must be feasible");
    let p1_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let p2 = solve_phase2(
        &inputs,
        &p1w,
        &p2w,
        p1.objective_eur,
        profile.planner.phase2_epsilon_eur,
        &p1,
        &ctxs,
        timeout,
    );
    let p2_s = t.elapsed().as_secs_f64();

    VariantResult {
        p1_s,
        p1_status: format!("{:?}", p1.status),
        p1_objective: p1.objective_eur,
        p2_s,
        p2_status: match &p2 {
            Ok((sol, _)) => format!("{:?}", sol.status),
            Err(_) => "Err".to_string(),
        },
    }
}

/// Solve one variant at the profile's default MIP gap (0.02).
fn solve_variant(temp_c: f64, initial_kw: f64, import_eur_kwh: f64) -> VariantResult {
    solve_at(temp_c, initial_kw, import_eur_kwh, 0.02)
}

/// GB-40 A/B: one row per start condition, for comparing a formulation change
/// against the committed baseline on identical instances.
///
/// Read the result this way: a genuine win is phase 1 flipping off `TimeLimit`
/// together with a large time drop. The same-instance run-to-run spread on this
/// machine is ~±4 s on ~112 s (three baseline runs gave 108.6 / 116.6 / 112.0),
/// so anything inside that band is noise. `p1_objective` is comparable across
/// arms because the instance is byte-identical, and catches a "faster but
/// worse" outcome that timing alone would hide.
#[test]
#[ignore = "GB-40 A/B harness: 5 production-sized solves, run with --ignored --nocapture"]
fn bench_heater_variants() {
    println!("\n=== GB-40 heater A/B: 10 start conditions, phases timed separately ===");
    println!(
        "  {:>3}  {:>8} {:>10} {:>13}  {:>8} {:>10}  {:>8}  condition",
        "#", "p1 s", "p1 status", "p1 objective", "p2 s", "p2 status", "total s"
    );
    for (i, (temp_c, kw, imp, label)) in HEATER_VARIANTS.iter().enumerate() {
        let r = solve_variant(*temp_c, *kw, *imp);
        println!(
            "  {:>3}  {:>8.2} {:>10} {:>13.4}  {:>8.2} {:>10}  {:>8.2}  {}",
            i + 1,
            r.p1_s,
            r.p1_status,
            r.p1_objective,
            r.p2_s,
            r.p2_status,
            r.p1_s + r.p2_s,
            label
        );
    }
    println!();
}

/// GB-40: fine-grained MIP-gap quality sweep across all 10 fixed heater
/// instances, searching for the point where loosening the gap stops being
/// (nearly) free.
///
/// The 2026-08-28 coarse sweep (2/5/10/20/35/50%, one instance) established
/// that phase 1 flips `TimeLimit` -> `GapLimit` somewhere between 10% and 20%,
/// and that 20% costs a mean +3.9% on phase 1's objective across the 5
/// instances (measured separately, one gap at a time). This sweep interleaves
/// both axes at once: 9 gap values x 10 instances = 90 solves, so the
/// quality-vs-gap curve can be read per instance and averaged, rather than
/// inferred from two endpoints.
///
/// Read it the same way as `bench_heater_variants`: phase 1's status is the
/// primary signal (`TimeLimit` -> `GapLimit` is the step change that matters),
/// `p1_objective` is the quality cost relative to the tightest gap (2%) on the
/// same instance, and the "optimum" this is searching for is the loosest gap
/// whose mean quality cost is still small next to the time it buys back.
#[test]
#[ignore = "GB-40 gap-quality sweep: 45 production-sized solves, run with --ignored --nocapture"]
fn bench_mip_gap_quality_sweep() {
    const GAPS: [f64; 9] = [0.02, 0.04, 0.07, 0.10, 0.13, 0.16, 0.18, 0.20, 0.22];

    println!("\n=== GB-40 MIP-gap quality sweep: 9 gaps x 10 heater instances ===");
    println!(
        "  {:>4}  {:>3}  {:>8} {:>10} {:>13} {:>9}  {:>8} {:>10}  condition",
        "gap", "#", "p1 s", "p1 status", "p1 objective", "vs 2%", "p2 s", "p2 status"
    );

    // baseline[i] = instance i's phase-1 objective at the tightest gap (2%),
    // established before the sweep so every row's "vs 2%" is against the same
    // per-instance reference rather than a running one.
    let baseline: Vec<f64> = HEATER_VARIANTS
        .iter()
        .map(|(temp_c, kw, imp, _)| solve_at(*temp_c, *kw, *imp, GAPS[0]).p1_objective)
        .collect();

    // gap_means[g] accumulates the per-instance %-deltas at gap g, so the
    // closing summary can report a mean quality cost per gap across all 5
    // instances rather than leaving the reader to eyeball 45 rows.
    let mut gap_means: Vec<(f64, f64, usize)> = GAPS.iter().map(|g| (*g, 0.0, 0)).collect();

    for (gi, &gap) in GAPS.iter().enumerate() {
        for (i, (temp_c, kw, imp, label)) in HEATER_VARIANTS.iter().enumerate() {
            let r = solve_at(*temp_c, *kw, *imp, gap);
            let base = baseline[i];
            let delta_pct = if base.abs() > 1e-9 {
                (r.p1_objective - base) / base.abs() * 100.0
            } else {
                0.0
            };
            println!(
                "  {:>3.0}%  {:>3}  {:>8.2} {:>10} {:>13.4} {:>+8.2}%  {:>8.2} {:>10}  {}",
                gap * 100.0,
                i + 1,
                r.p1_s,
                r.p1_status,
                r.p1_objective,
                delta_pct,
                r.p2_s,
                r.p2_status,
                label
            );
            gap_means[gi].1 += delta_pct;
            gap_means[gi].2 += 1;
        }
    }

    println!(
        "\n  --- mean quality cost per gap, across all 10 instances (vs each instance's own 2%) ---"
    );
    println!("  {:>4}  {:>10}", "gap", "mean Δ%");
    for (gap, sum, n) in &gap_means {
        println!("  {:>3.0}%  {:>+9.2}%", gap * 100.0, sum / *n as f64);
    }
    println!(
        "\n  A phase reporting GapLimit stopped on the gap; TimeLimit means it ran out of clock.\n\
           This is a measurement, not a threshold -- read the printed table, not an assertion.\n"
    );
}

// ── R-97: the EV band model through the FULL two-phase planner ──────────────
//
// The 288-slot phase-1 A/B in `gb41_soft_deadline_core.rs` showed every band
// shape solving in 0.04-0.10 s, i.e. the bands are not expensive on their own —
// yet ven-11 (EV + base load, no heater) went from a 114 ms median to 6.4 s when
// `ev-comfort-piecewise-core` shipped. The difference between that harness and
// production is that the fleet runs `run_planner`: two phases plus duals, and
// GB-40 records phase 2 as the half that never binds and burns its full budget.
// This measures the same site through the real entry point.
//
//   wsl cargo test -p ven-app --release bench_ev_session_solve_cost -- --ignored --nocapture

/// One full `run_planner` pass on the EV-only bench site, with or without a
/// firm EV session (the fleet's `engage_charge_planning` shape).
fn time_one_ev_solve(with_session: bool) -> (f64, String) {
    time_ev_solve_cfg(with_session, false)
}

/// As above, optionally giving the EV the fleet's `usage_forecast` schedule with
/// `engage_charge_planning` — which adds the away-window availability mask, the
/// projected SoC drops and a mid-horizon departure deadline. That is the part of
/// the fleet's shape the plain bench profile does not have.
fn time_ev_solve_cfg(with_session: bool, usage_forecast: bool) -> (f64, String) {
    time_ev_solve_at(with_session, usage_forecast, 0, 20.0)
}

/// `hours_after_now` shifts the clock so the car can be *already away*, and
/// `drop_pct` sets how much SoC the trip consumes — the two things that vary
/// day to day in the fleet and that R-97 identifies as the real cost driver.
fn time_ev_solve_at(
    with_session: bool,
    usage_forecast: bool,
    hours_after_now: i64,
    drop_pct: f64,
) -> (f64, String) {
    let now = fixed_now() + chrono::Duration::hours(hours_after_now);
    let profile = ev_bench_profile(usage_forecast, drop_pct);
    let sim = make_snap_from_profile(&profile);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let session = crate::entities::device_session::EvSession {
        id: uuid::Uuid::new_v4(),
        target_soc: 0.80,
        departure_time: now + chrono::Duration::hours(12),
        soft_deadline: false,
        origin: crate::entities::device_session::EvSessionOrigin::UserRequest,
        budget_eur: None,
        comfort_rates: vec![],
        mode: crate::entities::design_vocabulary::UserRequestMode::ByDeadline,
        created_at: now,
        updated_at: now,
    };
    let sess = with_session.then_some(&session);

    let started = Instant::now();
    let plan = run_planner(
        build_asset_contexts(&profile, &sim, now, sess, None, &tariffs),
        &tariffs,
        &no_capacity(),
        &profile,
        now,
        crate::entities::asset::PlanTrigger::Periodic,
        sess,
        None,
        &[],
        None,
        None,
    );
    (
        started.elapsed().as_secs_f64(),
        format!("{:?}", plan.solve_status),
    )
}

#[test]
#[ignore = "R-97 benchmark: full two-phase solves, run with --ignored --nocapture"]
fn bench_ev_session_solve_cost() {
    let _ = time_one_ev_solve(false); // warm-up, discarded

    println!(
        "
── R-97: EV-only site through the full two-phase planner ──
"
    );
    // Repeat each variant and report the MINIMUM, not one sample. A laptop under
    // any other load inflates individual solves by 3-6x — enough to invent or hide
    // an effect entirely (R-97 records a whole wrong conclusion built on single
    // samples). The minimum is the least contaminated estimator here: noise only
    // ever adds time.
    const REPEATS: usize = 5;
    for (label, sess, fc, dh, drop) in [
        ("no EV session", false, false, 0, 20.0),
        ("firm EV session", true, false, 0, 20.0),
        ("forecast, car HOME (leaves 08:00)", false, true, 0, 20.0),
        ("forecast, car AWAY, 20% drop", false, true, 3, 20.0),
        ("forecast, car AWAY, 65% drop", false, true, 3, 65.0),
        ("forecast, car AWAY, 90% drop", false, true, 3, 90.0),
        ("forecast, just RETURNED, 65% drop", false, true, 12, 65.0),
    ] {
        let mut times = Vec::with_capacity(REPEATS);
        let mut status = String::new();
        for _ in 0..REPEATS {
            let (secs, st) = time_ev_solve_at(sess, fc, dh, drop);
            times.push(secs);
            status = st;
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  {label:36} min {:7.2} s  median {:7.2} s  max {:7.2} s  {status}",
            times[0],
            times[REPEATS / 2],
            times[REPEATS - 1]
        );
    }
    println!();
}

/// The EV-only bench profile, optionally carrying the fleet's `usage_forecast`
/// schedule with `engage_charge_planning`.
fn ev_bench_profile(usage_forecast: bool, drop_pct: f64) -> Profile {
    ev_bench_profile_with_heater(usage_forecast, drop_pct, false)
}

fn ev_bench_profile_with_heater(usage_forecast: bool, drop_pct: f64, heater: bool) -> Profile {
    use crate::entities::asset_params::{EvUsageDayParams, EvUsageMode, EvUsageSimParams};
    let mut profile = bench_profile(heater);
    if usage_forecast {
        let day = EvUsageDayParams {
            leave_time: chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            leave_jitter_min: 0.0,
            return_time: chrono::NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            return_jitter_min: 0.0,
            leave_probability: 1.0,
            soc_drop_pct_mean: drop_pct,
            soc_drop_pct_stddev: 0.0,
        };
        for a in profile.assets.iter_mut() {
            if let crate::entities::asset_params::AssetParams::Ev(ev) = a {
                ev.usage_sim = Some(EvUsageSimParams {
                    mode: EvUsageMode::Forecast,
                    engage_charge_planning: true,
                    weekday: day.clone(),
                    weekend: day.clone(),
                    min_soc_after_drop_pct: 5.0,
                });
            }
        }
    }
    profile
}

/// R-97: where does the away-window cost land — phase 1 or phase 2?
///
/// GB-40 established that for the heater phase 2 never binds and burns its whole
/// budget, which is why every model-size idea aimed at phase 1 failed. Before
/// attempting anything on the EV, find out which half its ~5x actually lives in:
/// an EV-specific model change can only help the half that is actually spending
/// the time.
///
///   wsl cargo test -p ven-app --release bench_ev_phase_split -- --ignored --nocapture
fn ev_phase_split(
    usage_forecast: bool,
    hours_after_now: i64,
    drop_pct: f64,
) -> (f64, f64, String, String) {
    ev_phase_split_cfg(usage_forecast, hours_after_now, drop_pct, false)
}

fn ev_phase_split_cfg(
    usage_forecast: bool,
    hours_after_now: i64,
    drop_pct: f64,
    heater: bool,
) -> (f64, f64, String, String) {
    let now = fixed_now() + chrono::Duration::hours(hours_after_now);
    let profile = ev_bench_profile_with_heater(usage_forecast, drop_pct, heater);
    let mut sim = make_snap_from_profile(&profile);
    if heater {
        // The live condition the fleet's heater VENs are in (same as GB-40's bench).
        set_heater_power(&mut sim, 6.0);
    }
    let sim = sim;
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let cap = no_capacity();

    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p2w = build_phase2_weights(&inputs, &profile.planner);
    let timeout = profile.planner.solver_timeout_s as f64;

    let t = Instant::now();
    let p1 = solve_phase1(&inputs, &p1w, &ctxs, timeout).expect("phase 1 must be feasible");
    let p1_s = t.elapsed().as_secs_f64();
    let p1_status = format!("{:?}", p1.status);

    let t = Instant::now();
    let p2 = solve_phase2(
        &inputs,
        &p1w,
        &p2w,
        p1.objective_eur,
        profile.planner.phase2_epsilon_eur,
        &p1,
        &ctxs,
        timeout,
    );
    let p2_s = t.elapsed().as_secs_f64();
    let p2_status = match &p2 {
        Ok((sol, _)) => format!("{:?}", sol.status),
        Err(_) => "Err".to_string(),
    };
    (p1_s, p2_s, p1_status, p2_status)
}

#[test]
#[ignore = "R-97 benchmark: phase-split EV solves, run with --ignored --nocapture"]
fn bench_ev_phase_split() {
    let _ = ev_phase_split(false, 0, 20.0); // warm-up

    println!(
        "
── R-97: which phase pays for the EV away window ──
"
    );
    println!(
        "  {:34} {:>9} {:>9}   {:24}",
        "variant", "phase1", "phase2", "statuses"
    );
    const REPEATS: usize = 5;
    for (label, fc, dh, drop) in [
        ("no forecast (plain EV site)", false, 0, 20.0),
        ("forecast, car HOME", true, 0, 20.0),
        ("forecast, car AWAY", true, 3, 20.0),
        ("forecast, car AWAY, 90% drop", true, 3, 90.0),
        ("forecast, just RETURNED", true, 12, 65.0),
    ] {
        let mut p1s = Vec::new();
        let mut p2s = Vec::new();
        let mut st = (String::new(), String::new());
        for _ in 0..REPEATS {
            let (a, b, s1, s2) = ev_phase_split(fc, dh, drop);
            p1s.push(a);
            p2s.push(b);
            st = (s1, s2);
        }
        p1s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        p2s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  {label:34} {:9.3} {:9.3}   {} / {}",
            p1s[0], p2s[0], st.0, st.1
        );
    }
    // The question this whole line of work hangs on: on a heater VEN — the shape
    // that actually hits the timeout — does the EV leaving add anything, or is
    // phase 2 already saturated by the heater? If it is saturated, an EV-specific
    // fix cannot move the VENs that hurt.
    println!("\n  -- with a heater (the shape that actually times out) --\n");
    for (label, fc, dh) in [
        ("heater + EV, no forecast", false, 0),
        ("heater + EV, car HOME", true, 0),
        ("heater + EV, car AWAY", true, 3),
    ] {
        let mut p1s = Vec::new();
        let mut p2s = Vec::new();
        let mut st = (String::new(), String::new());
        for _ in 0..3 {
            let (a, b, s1, s2) = ev_phase_split_cfg(fc, dh, 20.0, true);
            p1s.push(a);
            p2s.push(b);
            st = (s1, s2);
        }
        p1s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        p2s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  {label:34} {:9.3} {:9.3}   {} / {}",
            p1s[0], p2s[0], st.0, st.1
        );
    }
    println!("\n  (minimum of {REPEATS} runs per phase; 3 for the heater rows)\n");
}

// ── R-97 / GB-40: is phase 2 worth the budget it burns? ─────────────────────
//
// Phase 2 minimises friction subject to `phase1_cap_expr <= c_star + epsilon`.
// That cap is a HARD constraint in its model, so any feasible incumbent — however
// early it is taken — already respects the cost bound. Everything phase 2 can
// change is therefore bounded by `phase2_epsilon_eur` by construction, while on a
// heater VEN it spends a full 57 s and still returns TimeLimit: half the total
// solve budget buying a refinement it cannot prove, worth at most one epsilon.
//
// This sweeps phase 2's timeout independently of phase 1's and reports what is
// actually lost: the friction it achieved, and the cost of the plan it returned.
//
//   wsl cargo test -p ven-app --release bench_phase2_timeout_sweep -- --ignored --nocapture

struct P2Result {
    p2_s: f64,
    p2_status: String,
    friction_eur: f64,
}

/// Solve the heater+EV bench site with phase 1 at its normal budget and phase 2
/// capped at `p2_timeout_s`.
fn phase2_at_timeout(p2_timeout_s: f64) -> P2Result {
    phase2_at_timeout_for(47.82, 6.0, 0.25, p2_timeout_s)
}

fn phase2_at_timeout_for(
    temp_c: f64,
    initial_kw: f64,
    import_eur_kwh: f64,
    p2_timeout_s: f64,
) -> P2Result {
    let now = fixed_now();
    let profile = ev_bench_profile_with_heater(true, 20.0, true);
    let mut sim = make_snap_from_profile(&profile);
    set_heater_temp(&mut sim, temp_c);
    set_heater_power(&mut sim, initial_kw);
    let tariffs = make_tariffs(import_eur_kwh, 0.08, 300.0);
    let cap = no_capacity();

    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p2w = build_phase2_weights(&inputs, &profile.planner);
    let p1_timeout = profile.planner.solver_timeout_s as f64;

    let p1 = solve_phase1(&inputs, &p1w, &ctxs, p1_timeout).expect("phase 1 must be feasible");

    let t = Instant::now();
    let p2 = solve_phase2(
        &inputs,
        &p1w,
        &p2w,
        p1.objective_eur,
        profile.planner.phase2_epsilon_eur,
        &p1,
        &ctxs,
        p2_timeout_s,
    );
    let p2_s = t.elapsed().as_secs_f64();
    match p2 {
        Ok((sol, friction_eur)) => P2Result {
            p2_s,
            p2_status: format!("{:?}", sol.status),
            friction_eur,
        },
        Err(_) => P2Result {
            p2_s,
            p2_status: "Err (falls back to phase 1)".to_string(),
            friction_eur: f64::NAN,
        },
    }
}

#[test]
#[ignore = "R-97 sweep: heater-sized phase-2 solves, run with --ignored --nocapture"]
fn bench_phase2_timeout_sweep() {
    let profile = ev_bench_profile_with_heater(true, 20.0, true);
    let epsilon = profile.planner.phase2_epsilon_eur;
    println!("\n── R-97: what a shorter phase-2 budget actually costs ──");
    println!("   (heater + EV, 288 slots; phase2_epsilon_eur = {epsilon})\n");
    // NB: phase 2's objective *is* the friction objective, so there is no separate
    // "plan cost" to print here — the economic cost is bounded by c_star + epsilon
    // structurally, because that cap is a hard constraint in phase 2's own model,
    // not because this bench measured it.
    println!(
        "  {:>10} {:>9} {:>14}  status",
        "p2 budget", "p2 time", "friction EUR"
    );
    for budget in [60.0, 20.0, 10.0, 5.0, 2.0, 1.0] {
        let r = phase2_at_timeout(budget);
        println!(
            "  {budget:>9.0}s {:>8.2}s {:>14.4}  {}",
            r.p2_s, r.friction_eur, r.p2_status
        );
    }
    println!(
        "\n  Friction is what phase 2 buys; plan cost is bounded by c_star + {epsilon} EUR\n  \
         at every budget, because that cap is a hard constraint in phase 2's own model.\n"
    );
}

/// R-97: does phase 2 change anything, or does it return its warm start?
///
/// The timeout sweep found phase 2's friction identical from a 1 s budget to a
/// 60 s one. Phase 2 is warm-started from phase 1, so the natural reading is that
/// it reaches an incumbent immediately and then spends the rest of its budget
/// failing to *prove* optimality. If that incumbent is simply phase 1's own
/// schedule, phase 2 contributes nothing on this instance and the question is not
/// how long to let it run but whether to run it.
///
///   wsl cargo test -p ven-app --release bench_does_phase2_change_the_schedule -- --ignored --nocapture
#[test]
#[ignore = "R-97 probe: two heater-sized solves, run with --ignored --nocapture"]
fn bench_does_phase2_change_the_schedule() {
    // A heater site (phase 1 and 2 both time out) and an EV-only site (both solve
    // cleanly). If phase 2 moves on the EV site but not the heater one, the no-op
    // is a property of timing out, not of phase 2 itself.
    for (label, heater) in [
        ("heater + EV (both phases TimeLimit)", true),
        ("EV only (both solve)", false),
    ] {
        println!(
            "
== {label}"
        );
        phase2_schedule_diff(heater);
    }
}

fn phase2_schedule_diff(heater: bool) {
    let now = fixed_now();
    let profile = ev_bench_profile_with_heater(true, 20.0, heater);
    let mut sim = make_snap_from_profile(&profile);
    if heater {
        set_heater_power(&mut sim, 6.0);
    }
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
    let cap = no_capacity();

    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p2w = build_phase2_weights(&inputs, &profile.planner);
    let timeout = profile.planner.solver_timeout_s as f64;

    let p1 = solve_phase1(&inputs, &p1w, &ctxs, timeout).expect("phase 1 must be feasible");
    let (p2, _friction) = solve_phase2(
        &inputs,
        &p1w,
        &p2w,
        p1.objective_eur,
        profile.planner.phase2_epsilon_eur,
        &p1,
        &ctxs,
        timeout,
    )
    .expect("phase 2 must be feasible");

    let diff = |a: &[f64], b: &[f64]| -> (usize, f64) {
        let mut n = 0;
        let mut worst = 0.0_f64;
        for (x, y) in a.iter().zip(b.iter()) {
            let d = (x - y).abs();
            if d > 1e-6 {
                n += 1;
            }
            if d > worst {
                worst = d;
            }
        }
        (n, worst)
    };

    println!("\n── R-97: what phase 2 actually changed ──\n");
    for (name, a, b) in [
        ("p_imp_kw", &p1.p_imp_kw, &p2.p_imp_kw),
        ("p_ev_kw", &p1.p_ev_kw, &p2.p_ev_kw),
        ("y_heat (stage)", &p1.y_heat, &p2.y_heat),
        ("p_bat_ch_kw", &p1.p_bat_ch_kw, &p2.p_bat_ch_kw),
    ] {
        let (n, worst) = diff(a, b);
        println!(
            "  {name:12} slots differing: {n:4} / {:4}   largest change: {worst:8.4}",
            a.len()
        );
    }
    println!(
        "\n  phase1 objective {:.4} EUR   phase2 objective {:.4} EUR\n",
        p1.objective_eur, p2.objective_eur
    );
}

/// R-97: across all ten heater instances, does phase 2 EVER change the schedule?
///
/// Two instances showed it returning phase 1's schedule byte-for-byte — one where
/// both phases time out, one where both solve cleanly. Two is not a result (GB-40's
/// MIP-gap sweep had to be redone from five instances to ten), so this runs the
/// whole `HEATER_VARIANTS` set and prints, per instance, how many of the 288 slots
/// phase 2 moved and what friction it reports.
///
///   wsl cargo test -p ven-app --release bench_phase2_changes_across_instances -- --ignored --nocapture
#[test]
#[ignore = "R-97 sweep: 20 heater-sized solves (~20 min), run with --ignored --nocapture"]
fn bench_phase2_changes_across_instances() {
    println!("\n── R-97: does phase 2 move the schedule on any instance? ──\n");
    println!(
        "  {:44} {:>7} {:>7} {:>7} {:>11} {:>12}",
        "instance", "imp", "ev", "heat", "friction", "p2 status"
    );
    let mut any_change = false;
    for (temp_c, initial_kw, import_eur_kwh, label) in HEATER_VARIANTS {
        let now = fixed_now();
        let profile = ev_bench_profile_with_heater(true, 20.0, true);
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, temp_c);
        set_heater_power(&mut sim, initial_kw);
        let tariffs = make_tariffs(import_eur_kwh, 0.08, 300.0);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let p2w = build_phase2_weights(&inputs, &profile.planner);
        let timeout = profile.planner.solver_timeout_s as f64;

        let p1 = solve_phase1(&inputs, &p1w, &ctxs, timeout).expect("phase 1 feasible");
        let (p2, friction) = match solve_phase2(
            &inputs,
            &p1w,
            &p2w,
            p1.objective_eur,
            profile.planner.phase2_epsilon_eur,
            &p1,
            &ctxs,
            timeout,
        ) {
            Ok(v) => v,
            Err(_) => {
                println!(
                    "  {label:44} {:>7} {:>7} {:>7} {:>11} {:>12}",
                    "-", "-", "-", "-", "Err"
                );
                continue;
            }
        };
        let count = |a: &[f64], b: &[f64]| {
            a.iter()
                .zip(b.iter())
                .filter(|(x, y)| (*x - *y).abs() > 1e-6)
                .count()
        };
        let (di, de, dh) = (
            count(&p1.p_imp_kw, &p2.p_imp_kw),
            count(&p1.p_ev_kw, &p2.p_ev_kw),
            count(&p1.y_heat, &p2.y_heat),
        );
        if di + de + dh > 0 {
            any_change = true;
        }
        println!(
            "  {label:44} {di:>7} {de:>7} {dh:>7} {friction:>11.4} {:>12?}",
            p2.status
        );
    }
    println!(
        "\n  Any instance where phase 2 moved the schedule: {any_change}\n  \
         (all counts are out of 288 slots; phase 1 ran at the full 60 s budget)\n"
    );
}

/// R-97 cause analysis, ineffective planning: is phase 2 *starved* or *failing*?
///
/// Phase 2 never moves the heater on any instance. Two candidate causes:
///   (a) starved — the `phase1_cost <= c_star + epsilon` cap is too tight for any
///       cheaper-switching schedule to fit, so there is genuinely nothing legal to
///       find. Rescheduling a heater stage moves real money; epsilon is 0.17 EUR.
///   (b) failing — improvements exist within the cap and branch-and-bound does not
///       find them in the time available.
/// Raising epsilon separates the two: under (a) phase 2 starts moving once the cap
/// is loose enough to admit a swap; under (b) it stays inert however loose it gets.
///
///   wsl cargo test -p ven-app --release bench_phase2_epsilon_sweep -- --ignored --nocapture
#[test]
#[ignore = "R-97 cause analysis: 8 heater-sized solves, run with --ignored --nocapture"]
fn bench_phase2_epsilon_sweep() {
    println!(
        "
── R-97: is phase 2 starved by its cost cap, or failing to search? ──"
    );
    println!(
        "   (ven-2 runs mip_gap 0.06 / epsilon 1.00 in production; the earlier bench
             used 0.02 / 0.17, where phase 1 times out and phase 2 inherits a poor
             incumbent and c_star. Both gaps are swept so that difference is visible.)
"
    );
    println!(
        "  {:>8} {:>9} {:>9} {:>10} {:>9} {:>8} {:>8} {:>8} {:>11}",
        "mip_gap", "epsilon", "p1 s", "p1 status", "p2 s", "imp", "ev", "heat", "friction"
    );
    for mip_gap in [0.02, 0.06] {
        for epsilon in [0.17, 0.5, 1.0, 5.0] {
            let now = fixed_now();
            let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
            profile.planner.phase2_epsilon_eur = epsilon;
            profile.planner.mip_gap_target = mip_gap;
            let mut sim = make_snap_from_profile(&profile);
            set_heater_power(&mut sim, 6.0);
            let tariffs = make_diurnal_tariffs(50);
            let cap = no_capacity();
            let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
            let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
            let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
            let p2w = build_phase2_weights(&inputs, &profile.planner);

            let t = Instant::now();
            let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
            let p1_s = t.elapsed().as_secs_f64();

            // Deliberately a full 60 s: this asks whether an improvement EXISTS
            // within the cap, not how fast it is found.
            let t = Instant::now();
            let r = solve_phase2(
                &inputs,
                &p1w,
                &p2w,
                p1.objective_eur,
                epsilon,
                &p1,
                &ctxs,
                60.0,
            );
            let p2_s = t.elapsed().as_secs_f64();
            match r {
                Ok((p2, friction)) => {
                    let c = |a: &[f64], b: &[f64]| {
                        a.iter()
                            .zip(b.iter())
                            .filter(|(x, y)| (*x - *y).abs() > 1e-6)
                            .count()
                    };
                    println!(
                        "  {mip_gap:>8.2} {epsilon:>9.2} {p1_s:>9.2} {:>10?} {p2_s:>9.2} {:>8} {:>8} {:>8} {friction:>11.4}",
                        p1.status,
                        c(&p1.p_imp_kw, &p2.p_imp_kw),
                        c(&p1.p_ev_kw, &p2.p_ev_kw),
                        c(&p1.y_heat, &p2.y_heat),
                    );
                }
                Err(_) => println!(
                    "  {mip_gap:>8.2} {epsilon:>9.2} {p1_s:>9.2} {:>10?} {p2_s:>9.2}   phase 2 Err",
                    p1.status
                ),
            }
        }
    }
    println!(
        "
  Starved -> heat/imp counts rise as epsilon loosens. Failing -> they stay 0
           however loose the cap gets. Slot counts are out of 288.
"
    );
}

/// R-97 cause analysis, long planning time: how does phase 1 scale with horizon?
///
/// GB-40 lists horizon truncation as untried, and GB-42 records that everything
/// past the end of published tariff data is priced by a flat hold. Flat prices make
/// far-horizon schedules tie, and ties are what stop branch-and-bound pruning — so
/// the far half of the horizon may contribute almost no information while carrying
/// half the heater's stage integers. This measures phase 1 alone against the number
/// of slots, holding everything else fixed.
///
///   wsl cargo test -p ven-app --release bench_phase1_vs_horizon -- --ignored --nocapture
#[test]
#[ignore = "R-97 cause analysis: 5 heater-sized phase-1 solves, run with --ignored --nocapture"]
fn bench_phase1_vs_horizon() {
    println!("\n── R-97: how phase 1's cost scales with horizon length ──\n");
    println!(
        "  {:>7} {:>10} {:>14}  status",
        "slots", "phase1 s", "objective EUR"
    );
    for zones in [
        vec![crate::entities::plan::PlanZone {
            step_s: 300,
            slots: 48,
        }],
        vec![crate::entities::plan::PlanZone {
            step_s: 300,
            slots: 96,
        }],
        vec![
            crate::entities::plan::PlanZone {
                step_s: 300,
                slots: 96,
            },
            crate::entities::plan::PlanZone {
                step_s: 600,
                slots: 96,
            },
        ],
        vec![
            crate::entities::plan::PlanZone {
                step_s: 300,
                slots: 96,
            },
            crate::entities::plan::PlanZone {
                step_s: 600,
                slots: 96,
            },
            crate::entities::plan::PlanZone {
                step_s: 900,
                slots: 96,
            },
        ],
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        let n: usize = zones.iter().map(|z| z.slots).sum();
        profile.planner.plan_zones = zones;
        let mut sim = make_snap_from_profile(&profile);
        set_heater_power(&mut sim, 6.0);
        let tariffs = make_tariffs(0.25, 0.08, 300.0);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let t = Instant::now();
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        println!(
            "  {n:>7} {:>10.2} {:>14.4}  {:?}",
            t.elapsed().as_secs_f64(),
            p1.objective_eur,
            p1.status
        );
    }
    println!("\n  (the production grid is the last row: 288 slots over three zones)\n");
}

/// A diurnal tariff series covering `hours` ahead, one snapshot per hour, so the
/// far horizon carries real price variation instead of a flat stale-rate hold.
fn make_diurnal_tariffs(hours: i64) -> TariffTimeSeries {
    let now = fixed_now();
    let snaps: Vec<TariffSnapshot> = (0..hours)
        .map(|h| {
            let start = now - chrono::Duration::hours(1) + chrono::Duration::hours(h);
            // Cheap at night, dear in the evening peak — the shape a real feed has.
            let hour_of_day = (chrono::Timelike::hour(&start) as f64) % 24.0;
            let imp = 0.15 + 0.15 * (1.0 + ((hour_of_day - 18.0) / 3.0).cos()) / 2.0;
            TariffSnapshot {
                interval_start: start,
                interval_end: start + chrono::Duration::hours(1),
                import_tariff_eur_kwh: Some(imp),
                export_tariff_eur_kwh: Some(0.08),
                co2_g_kwh: Some(300.0),
            }
        })
        .collect();
    TariffTimeSeries::from_snapshots(&snaps)
}

/// R-97 root cause: is phase 1's 288-slot cliff caused by the *number* of slots,
/// or by the far horizon being priced by a flat stale-rate hold?
///
/// The horizon sweep found 192 slots solving in 1.09 s and 288 in 60.04 s — 1.5x
/// the slots for 55x the time. But the two are confounded: the bench tariff covers
/// 25 h, so 192 slots (24 h) is fully priced while 288 (48 h) runs ~23 h past
/// coverage into `StaleRatePolicy`'s flat hold (GB-42). Flat prices make far-horizon
/// schedules tie, and ties are what stop branch-and-bound pruning, while those
/// slots still carry ~96 of the heater's stage integers.
///
/// This holds the slot count at 288 and varies only whether the far half is priced.
///
///   wsl cargo test -p ven-app --release bench_phase1_flat_vs_priced_far_horizon -- --ignored --nocapture
#[test]
#[ignore = "R-97 root cause: 4 heater-sized phase-1 solves, run with --ignored --nocapture"]
fn bench_phase1_flat_vs_priced_far_horizon() {
    println!("\n── R-97: does the FLAT far horizon cause the cliff, or the slot count? ──\n");
    println!(
        "  {:>7} {:>26} {:>10} {:>14}  status",
        "slots", "far horizon", "phase1 s", "objective EUR"
    );
    for (slots_label, zones) in [
        (
            192,
            vec![
                crate::entities::plan::PlanZone {
                    step_s: 300,
                    slots: 96,
                },
                crate::entities::plan::PlanZone {
                    step_s: 600,
                    slots: 96,
                },
            ],
        ),
        (
            288,
            vec![
                crate::entities::plan::PlanZone {
                    step_s: 300,
                    slots: 96,
                },
                crate::entities::plan::PlanZone {
                    step_s: 600,
                    slots: 96,
                },
                crate::entities::plan::PlanZone {
                    step_s: 900,
                    slots: 96,
                },
            ],
        ),
    ] {
        for (tariff_label, tariffs) in [
            ("flat hold past 25 h", make_tariffs(0.25, 0.08, 300.0)),
            ("priced all 49 h", make_diurnal_tariffs(49)),
        ] {
            let now = fixed_now();
            let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
            profile.planner.plan_zones = zones.clone();
            let mut sim = make_snap_from_profile(&profile);
            set_heater_power(&mut sim, 6.0);
            let cap = no_capacity();
            let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
            let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
            let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
            let t = Instant::now();
            let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
            println!(
                "  {slots_label:>7} {tariff_label:>26} {:>10.2} {:>14.4}  {:?}",
                t.elapsed().as_secs_f64(),
                p1.objective_eur,
                p1.status
            );
        }
    }
    println!(
        "\n  If 288 + priced is fast, the cliff is the flat hold (GB-42), not the model size.\n"
    );
}

/// R-97 root cause, continued: slot COUNT or horizon DURATION?
///
/// The flat-vs-priced control refuted pricing as the cause: 288 slots times out at
/// 60 s either way. The production grid is 288 slots spanning 48 h, so count and
/// duration are still confounded. This varies them independently:
///
///   288 slots / 24 h  — production's count, half its span
///   192 slots / 48 h  — production's span, two thirds its count
///   288 slots / 48 h  — production
///
/// If the 24 h variant is fast, the difficulty is the horizon's *length in time*
/// (tank trajectory, terminal conditions). If the 48 h/192 variant is fast, it is
/// the *number of integer decisions*.
///
///   wsl cargo test -p ven-app --release bench_phase1_count_vs_duration -- --ignored --nocapture
#[test]
#[ignore = "R-97 root cause: 4 heater-sized phase-1 solves, run with --ignored --nocapture"]
fn bench_phase1_count_vs_duration() {
    println!("\n── R-97: is it the slot count or the horizon duration? ──\n");
    println!(
        "  {:>34} {:>8} {:>6} {:>6} {:>10} {:>13}  status",
        "grid", "mip_gap", "slots", "hours", "phase1 s", "objective"
    );
    let z = |step_s: u64, slots: usize| crate::entities::plan::PlanZone { step_s, slots };
    for (label, zones) in [
        ("96x300s (8 h, reference)", vec![z(300, 96)]),
        ("288x300s (24 h, prod count)", vec![z(300, 288)]),
        ("192x900s (48 h, prod span)", vec![z(900, 192)]),
        (
            "96x300+96x600+96x900 (48 h, PROD)",
            vec![z(300, 96), z(600, 96), z(900, 96)],
        ),
    ] {
        // Both gaps. The first run of this bench used the 0.02 default while every
        // heater VEN in the fleet runs 0.06, and the two differ enough to have
        // turned a grid that solves in ~5 s at production settings into a reported
        // "60 s cliff". Sweeping both keeps that confound visible in the log.
        for mip_gap in [0.02, 0.06] {
            let now = fixed_now();
            let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
            profile.planner.mip_gap_target = mip_gap;
            let n: usize = zones.iter().map(|x| x.slots).sum();
            let hours: f64 = zones
                .iter()
                .map(|x| x.slots as f64 * x.step_s as f64 / 3600.0)
                .sum();
            profile.planner.plan_zones = zones.clone();
            let mut sim = make_snap_from_profile(&profile);
            set_heater_power(&mut sim, 6.0);
            let tariffs = make_diurnal_tariffs(50);
            let cap = no_capacity();
            let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
            let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
            let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
            let t = Instant::now();
            let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
            let secs = t.elapsed().as_secs_f64();
            println!(
                "  {label:>34} {mip_gap:>8.2} {n:>6} {hours:>6.0} {secs:>10.2} {:>13.4}  {:?}",
                p1.objective_eur, p1.status
            );
            emit_result(&format!(
                r#"{{"params":{{"grid":"{label}","slots":{n},"hours":{hours:.0},"mip_gap":{mip_gap},"volume_l":200,"band_k":15}},"results":{{"phase1_s":{secs:.3},"objective_eur":{:.4},"status":"{:?}"}}}}"#,
                p1.objective_eur, p1.status
            ));
        }
    }
    println!("\n  (all fully priced; gap is the only other variable)\n");
}

/// R-97: does the 48 h horizon make the EXECUTED part of the plan worse?
///
/// Count-vs-duration showed 288 slots solving in 2.14 s over 24 h and timing out at
/// 60 s over 48 h, so duration is the cost driver. But a plan only executes its
/// first slots before the next replan, so a bad far horizon is only *harmful* if it
/// changes those near-term decisions. This solves the same site at both spans and
/// compares the first 8 h: the schedule, and its cost under identical pricing.
///
///   wsl cargo test -p ven-app --release bench_does_the_far_horizon_harm_execution -- --ignored --nocapture
#[test]
#[ignore = "R-97: 2 heater-sized phase-1 solves, run with --ignored --nocapture"]
fn bench_does_the_far_horizon_harm_execution() {
    let z = |step_s: u64, slots: usize| crate::entities::plan::PlanZone { step_s, slots };
    let mut results = Vec::new();
    for (label, zones) in [
        ("24 h (288x300s)", vec![z(300, 288)]),
        (
            "48 h (PROD 3-zone)",
            vec![z(300, 96), z(600, 96), z(900, 96)],
        ),
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        profile.planner.plan_zones = zones;
        let mut sim = make_snap_from_profile(&profile);
        set_heater_power(&mut sim, 6.0);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let t = Instant::now();
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let secs = t.elapsed().as_secs_f64();

        // Economic cost of the first 8 hours only — the part that actually runs
        // before the next cycle replaces it. Same pricing in both, so comparable.
        let mut cost_8h = 0.0;
        let mut elapsed_h = 0.0;
        let mut heater_switches = 0usize;
        let mut ev_kwh_8h = 0.0;
        for t_i in 0..inputs.n {
            if elapsed_h >= 8.0 {
                break;
            }
            let dt = inputs.dt_h[t_i];
            cost_8h += (inputs.c_imp_eur_kwh[t_i] * p1.p_imp_kw[t_i]
                - inputs.c_exp_eur_kwh[t_i] * p1.p_exp_kw[t_i])
                * dt;
            ev_kwh_8h += p1.p_ev_kw[t_i] * dt;
            if t_i > 0 && (p1.y_heat[t_i] - p1.y_heat[t_i - 1]).abs() > 1e-6 {
                heater_switches += 1;
            }
            elapsed_h += dt;
        }
        results.push((
            label,
            secs,
            p1.objective_eur,
            cost_8h,
            ev_kwh_8h,
            heater_switches,
            format!("{:?}", p1.status),
        ));
    }

    println!("\n── R-97: what the 48 h horizon does to the first 8 hours ──\n");
    println!(
        "  {:>20} {:>9} {:>13} {:>13} {:>11} {:>10}  status",
        "horizon", "phase1 s", "full obj EUR", "first-8h EUR", "EV kWh 8h", "heat sw 8h"
    );
    for (label, secs, obj, c8, ev8, sw, st) in &results {
        println!("  {label:>20} {secs:>9.2} {obj:>13.4} {c8:>13.4} {ev8:>11.2} {sw:>10}  {st}");
    }
    println!(
        "\n  first-8h EUR is the cost of the part that actually executes, priced identically\n  \
         in both rows. If the 48 h row is worse there, the long horizon is not merely\n  \
         expensive to solve — it degrades the decisions the VEN carries out.\n"
    );
}

/// R-97 URGENT: with realistic prices, how fast does phase 2 earn its friction?
///
/// The original timeout sweep used a FLAT tariff, where phase 1 has no reason to
/// fragment the schedule and phase 2 consequently has nothing to clean up — which
/// is why it looked inert at every budget. With diurnal prices at ven-2's own
/// settings (mip_gap 0.06, epsilon 1.00) phase 2 moves 68 of 288 heater slots and
/// halves friction, taking ~59 s to do it. The deployed 5 s cap may therefore be
/// cutting off real work.
///
/// This re-runs the budget sweep on the production-shaped configuration. If
/// friction at 5 s is no better than doing nothing, the cap is a regression.
///
///   wsl cargo test -p ven-app --release bench_phase2_budget_with_real_prices -- --ignored --nocapture
#[test]
#[ignore = "R-97 urgent: 6 heater-sized solves, run with --ignored --nocapture"]
fn bench_phase2_budget_with_real_prices() {
    let now = fixed_now();
    let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
    profile.planner.mip_gap_target = 0.06; // ven-2 production
    profile.planner.phase2_epsilon_eur = 1.00; // ven-2 production
    let mut sim = make_snap_from_profile(&profile);
    set_heater_power(&mut sim, 6.0);
    let tariffs = make_diurnal_tariffs(50);
    let cap = no_capacity();
    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p2w = build_phase2_weights(&inputs, &profile.planner);
    let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");

    println!(
        "
── R-97: phase 2 vs its budget, at ven-2's settings with real prices ──"
    );
    println!(
        "   (mip_gap 0.06, epsilon 1.00, diurnal tariff; phase 1 {:?})
",
        p1.status
    );
    println!(
        "  {:>10} {:>9} {:>8} {:>8} {:>11}  status",
        "p2 budget", "p2 time", "imp", "heat", "friction"
    );
    for budget in [1.0, 2.0, 5.0, 10.0, 20.0, 60.0] {
        let t = Instant::now();
        let r = solve_phase2(
            &inputs,
            &p1w,
            &p2w,
            p1.objective_eur,
            1.00,
            &p1,
            &ctxs,
            budget,
        );
        let secs = t.elapsed().as_secs_f64();
        match r {
            Ok((p2, friction)) => {
                let c = |a: &[f64], b: &[f64]| {
                    a.iter()
                        .zip(b.iter())
                        .filter(|(x, y)| (*x - *y).abs() > 1e-6)
                        .count()
                };
                println!(
                    "  {budget:>9.0}s {secs:>8.2}s {:>8} {:>8} {friction:>11.4}  {:?}",
                    c(&p1.p_imp_kw, &p2.p_imp_kw),
                    c(&p1.y_heat, &p2.y_heat),
                    p2.status
                );
            }
            Err(_) => {
                println!("  {budget:>9.0}s {secs:>8.2}s   phase 2 Err (falls back to phase 1)")
            }
        }
    }
    println!(
        "
  Lower friction is better. If 5 s sits at the do-nothing value while 60 s
           halves it, the deployed phase2_solver_timeout_s default is a regression.
"
    );
}

/// R-97: does the phase-2 budget change what actually gets EXECUTED?
///
/// Horizon-wide friction overstates the real cost of a short budget: the plan is
/// replanned every `replan_interval_s` (300 s), so only its first slots ever run
/// and far-horizon switches never happen. 5 s captures ~46 % of the horizon-wide
/// friction reduction that 60 s achieves — this asks how much of that difference
/// lands in the part of the plan the relay actually follows.
///
///   wsl cargo test -p ven-app --release bench_phase2_budget_executed_window -- --ignored --nocapture
#[test]
#[ignore = "R-97: 2 heater-sized phase-2 solves, run with --ignored --nocapture"]
fn bench_phase2_budget_executed_window() {
    let now = fixed_now();
    let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
    profile.planner.mip_gap_target = 0.06;
    profile.planner.phase2_epsilon_eur = 1.00;
    let mut sim = make_snap_from_profile(&profile);
    set_heater_power(&mut sim, 6.0);
    let tariffs = make_diurnal_tariffs(50);
    let cap = no_capacity();
    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p2w = build_phase2_weights(&inputs, &profile.planner);
    let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");

    // Switches inside the first `hours` of a solution.
    let switches_within = |y: &[f64], hours: f64| -> usize {
        let mut n = 0;
        let mut elapsed = 0.0;
        for t in 1..inputs.n {
            if elapsed >= hours {
                break;
            }
            if (y[t] - y[t - 1]).abs() > 1e-6 {
                n += 1;
            }
            elapsed += inputs.dt_h[t];
        }
        n
    };

    println!(
        "
── R-97: phase-2 budget vs the window that actually executes ──"
    );
    println!(
        "   (ven-2 settings; replan every 300 s, so the first slots are what run)
"
    );
    println!(
        "  {:>12} {:>11} {:>12} {:>12} {:>12} {:>12}",
        "p2 budget", "friction", "switches 25m", "switches 1h", "switches 8h", "switches 48h"
    );
    println!(
        "  {:>12} {:>11.4} {:>12} {:>12} {:>12} {:>12}",
        "phase 1 only",
        f64::NAN,
        switches_within(&p1.y_heat, 25.0 / 60.0),
        switches_within(&p1.y_heat, 1.0),
        switches_within(&p1.y_heat, 8.0),
        switches_within(&p1.y_heat, 48.0),
    );
    for budget in [5.0, 60.0] {
        let (p2, friction) = solve_phase2(
            &inputs,
            &p1w,
            &p2w,
            p1.objective_eur,
            1.00,
            &p1,
            &ctxs,
            budget,
        )
        .expect("phase 2 feasible");
        println!(
            "  {:>11.0}s {friction:>11.4} {:>12} {:>12} {:>12} {:>12}",
            budget,
            switches_within(&p2.y_heat, 25.0 / 60.0),
            switches_within(&p2.y_heat, 1.0),
            switches_within(&p2.y_heat, 8.0),
            switches_within(&p2.y_heat, 48.0),
        );
    }
    println!(
        "
  25m is roughly five replan cycles — beyond that the plan is almost certainly
           replaced before it runs. If 5 s and 60 s agree there, the budget costs nothing
           the relay ever feels.
"
    );
}

/// R-97: what is actually inside phase 2's "friction"?
///
/// `PV_USE_TIEBREAK_EUR_PER_KWH` is 0.005 EUR/kWh, documented as a bias "small
/// enough that any real constraint still dominates". That was calibrated against
/// **phase 1's** objective, which carries tens of euros of energy cost. Phase 2's
/// objective is friction-only — a few euros — so over 288 slots with a 6 kW array
/// the same term accumulates to roughly 2 EUR, the same order as the friction it is
/// mixed into. If so, phase 2 is not minimising switching; it is trading switching
/// against PV utilisation, and `friction_eur` is not a switching metric.
///
/// That would also explain the otherwise contradictory result that the 60 s solution
/// has MORE heater switches than the 5 s one (15 vs 11 over 8 h) yet reports LOWER
/// friction (2.08 vs 3.46).
///
///   wsl cargo test -p ven-app --release bench_what_is_in_phase2_friction -- --ignored --nocapture
#[test]
#[ignore = "R-97: 2 heater-sized phase-2 solves, run with --ignored --nocapture"]
fn bench_what_is_in_phase2_friction() {
    let now = fixed_now();
    let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
    profile.planner.mip_gap_target = 0.06;
    profile.planner.phase2_epsilon_eur = 1.00;
    let switch_penalty = 0.50_f64; // heater switching_penalty_eur in bench_profile
    let mut sim = make_snap_from_profile(&profile);
    set_heater_power(&mut sim, 6.0);
    let tariffs = make_diurnal_tariffs(50);
    let cap = no_capacity();
    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p2w = build_phase2_weights(&inputs, &profile.planner);
    let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");

    let decompose = |sol: &SolveOutput| -> (usize, f64, f64, f64) {
        let switches = (1..inputs.n)
            .filter(|&t| (sol.y_heat[t] - sol.y_heat[t - 1]).abs() > 1e-6)
            .count();
        let pv_kwh: f64 = sol
            .p_pv_used_kw
            .iter()
            .zip(inputs.dt_h.iter())
            .map(|(p, d)| p * d)
            .sum();
        // The same coefficient the objective uses, with its sign: a reward.
        let pv_tiebreak =
            -crate::controller::milp_interactions::PV_USE_TIEBREAK_EUR_PER_KWH * pv_kwh;
        (
            switches,
            switches as f64 * switch_penalty,
            pv_kwh,
            pv_tiebreak,
        )
    };

    println!("\n── R-97: decomposing phase 2's objective ──");
    println!(
        "   (PV_USE_TIEBREAK_EUR_PER_KWH = {}, heater switching_penalty = {switch_penalty})\n",
        crate::controller::milp_interactions::PV_USE_TIEBREAK_EUR_PER_KWH
    );
    println!(
        "  {:>16} {:>11} {:>9} {:>13} {:>11} {:>13}",
        "solution", "friction", "switches", "switch cost", "PV kWh", "PV tiebreak"
    );
    let (s, sc, pv, tb) = decompose(&p1);
    println!(
        "  {:>16} {:>11} {s:>9} {sc:>13.4} {pv:>11.2} {tb:>13.4}",
        "phase 1", "-"
    );
    for budget in [5.0, 60.0] {
        let (p2, friction) = solve_phase2(
            &inputs,
            &p1w,
            &p2w,
            p1.objective_eur,
            1.00,
            &p1,
            &ctxs,
            budget,
        )
        .expect("phase 2 feasible");
        let (s, sc, pv, tb) = decompose(&p2);
        println!(
            "  {:>15}s {friction:>11.4} {s:>9} {sc:>13.4} {pv:>11.2} {tb:>13.4}",
            budget as i64
        );
    }
    println!(
        "\n  If the PV tiebreak column moves by the same order as the friction column,\n  \
         then `friction_eur` is not a switching metric and phase 2 is optimising a\n  \
         blend its own name does not describe.\n"
    );
}

/// R-97 / GB-40: is heater MILP difficulty set by the tank's thermal slack?
///
/// Live ven-2 and ven-3 run the same assets, the same 288-slot 48 h grid and the
/// same `mip_gap_target` (0.06), yet phase 1 takes **227-309 ms** on ven-2 and
/// **11-46 s** on ven-3. The profiles differ in tank physics:
///
///   ven-2: volume 2000 L, band 40-80 C (40 K)   -> phase 1 fast
///   ven-3: volume  200 L, band 45-60 C (15 K)   -> phase 1 slow
///
/// A large tank with a wide band has enormous thermal slack, so the heater can be
/// placed almost anywhere and the schedule is barely constrained. A small tank with
/// a narrow band must cycle frequently and precisely. If difficulty tracks slack,
/// that explains GB-40's own unexplained variance ("not every heater VEN is slow --
/// ven-2 18.2 s, ven-20 29.0 s") and points at a *physical* lever rather than a
/// reformulation.
///
/// Held fixed: grid, tariffs, gap, assets. Varied: volume and band only.
///
///   wsl cargo test -p ven-app --release bench_phase1_vs_tank_slack -- --ignored --nocapture
#[test]
#[ignore = "R-97/GB-40: 8 heater-sized phase-1 solves, run with --ignored --nocapture"]
fn bench_phase1_vs_tank_slack() {
    println!("\n── R-97/GB-40: phase-1 cost vs the tank's thermal slack ──\n");
    println!(
        "  {:>9} {:>12} {:>10} {:>12} {:>10} {:>14}  status",
        "volume L", "band K", "slack kWh", "phase1 s", "switches", "objective EUR"
    );
    for (volume_l, tmin, tmax) in [
        (200.0, 45.0, 60.0), // ven-3
        (200.0, 40.0, 80.0), // small tank, wide band
        (500.0, 45.0, 60.0),
        (1000.0, 45.0, 60.0),
        (2000.0, 45.0, 60.0), // big tank, narrow band
        (2000.0, 40.0, 80.0), // ven-2
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        profile.planner.mip_gap_target = 0.06; // both live VENs
        let thermal_mass = volume_l * 4.186 / 3600.0;
        for a in profile.assets.iter_mut() {
            if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                h.thermal_mass_kwh_per_c = thermal_mass;
                h.temp_min_c = tmin;
                h.temp_max_c = tmax;
                h.temp_safety_max_c = tmax;
                h.temp_initial_c = (tmin + tmax) / 2.0;
            }
        }
        // Usable thermal energy between the band limits — the slack the planner has.
        let slack_kwh = thermal_mass * (tmax - tmin);
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, (tmin + tmax) / 2.0);
        set_heater_power(&mut sim, 6.0);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let t = Instant::now();
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let secs = t.elapsed().as_secs_f64();
        let switches = (1..inputs.n)
            .filter(|&i| (p1.y_heat[i] - p1.y_heat[i - 1]).abs() > 1e-6)
            .count();
        println!(
            "  {volume_l:>9.0} {:>12.0} {slack_kwh:>10.1} {secs:>12.2} {switches:>10} {:>14.4}  {:?}",
            tmax - tmin,
            p1.objective_eur,
            p1.status
        );
        emit_result(&format!(
            r#"{{"params":{{"volume_l":{volume_l},"band_k":{},"slack_kwh":{slack_kwh:.2},"mip_gap":0.06,"slots":{},"hours":48}},"results":{{"phase1_s":{secs:.3},"switches":{switches},"status":"{:?}","objective_eur":{:.4}}}}}"#,
            tmax - tmin,
            inputs.n,
            p1.status,
            p1.objective_eur
        ));
    }
    println!(
        "\n  If phase-1 time falls as slack rises, heater difficulty is a property of the\n  \
         installation, not of the formulation — and GB-40's per-VEN variance is explained.\n"
    );
}

/// Print one experiment record for `scripts/run_planner_experiment.sh` to stamp and
/// append to `experiments/results/planner/solve_cost.jsonl`. Keeping the emission in
/// the benchmark means the log cannot drift from the code, which hand-copied prose
/// tables always eventually do.
fn emit_result(json_body: &str) {
    println!("@@RESULT {json_body}");
}

/// R-97: can the 48 h horizon be kept while cutting phase-1 cost?
///
/// The 48 h span is a requirement — a receding-horizon controller needs lookahead
/// past the window it optimises, or end-of-horizon effects distort the near term.
/// But phase-1 cost tracks *duration*, and at a fixed 48 h span fewer slots helped
/// (192 slots took 23 s against 288's 60 s). So coarsen the far zones: keep the
/// executed near term at 5-minute resolution and spend fewer integer decisions on
/// hours 8-48, where the plan is replaced long before it runs.
///
/// Run on a **slack-poor** heater (200 L / 15 K, ven-3's shape), because the
/// tank-slack result says that is the hard case and the only one worth optimising.
/// The executed-window columns guard the near term: if first-8 h cost or switching
/// degrades, the coarsening is not free.
///
///   bash scripts/run_planner_experiment.sh bench_phase1_vs_zones
#[test]
#[ignore = "R-97: 4 heater-sized phase-1 solves, run with --ignored --nocapture"]
fn bench_phase1_vs_zones() {
    let z = |step_s: u64, slots: usize| crate::entities::plan::PlanZone { step_s, slots };
    println!(
        "
── R-97: far-zone coarsening at a fixed 48 h span (slack-poor heater) ──
"
    );
    println!(
        "  {:>34} {:>6} {:>9} {:>11} {:>13} {:>12}  status",
        "grid (all 48 h)", "slots", "phase1 s", "switches 8h", "first-8h EUR", "objective"
    );
    for (label, zones) in [
        (
            "96x300 + 96x600 + 96x900 (PROD)",
            vec![z(300, 96), z(600, 96), z(900, 96)],
        ),
        (
            "96x300 + 48x1200 + 24x3600",
            vec![z(300, 96), z(1200, 48), z(3600, 24)],
        ),
        (
            "96x300 + 24x2400 + 12x7200",
            vec![z(300, 96), z(2400, 24), z(7200, 12)],
        ),
        (
            "48x300 + 44x1200 + 28x3600",
            vec![z(300, 48), z(1200, 44), z(3600, 28)],
        ),
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        profile.planner.mip_gap_target = 0.06;
        // ven-3's tank: 200 L across 45-60 C, the slack-poor case.
        let thermal_mass = 200.0 * 4.186 / 3600.0;
        for a in profile.assets.iter_mut() {
            if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                h.thermal_mass_kwh_per_c = thermal_mass;
                h.temp_min_c = 45.0;
                h.temp_max_c = 60.0;
                h.temp_safety_max_c = 60.0;
                h.temp_initial_c = 47.82;
            }
        }
        let n: usize = zones.iter().map(|x| x.slots).sum();
        let hours: f64 = zones
            .iter()
            .map(|x| x.slots as f64 * x.step_s as f64 / 3600.0)
            .sum();
        profile.planner.plan_zones = zones;
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, 47.82);
        set_heater_power(&mut sim, 6.0);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let t = Instant::now();
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let secs = t.elapsed().as_secs_f64();

        // Executed window: the part that runs before the next replan replaces it.
        let mut cost_8h = 0.0;
        let mut sw_8h = 0usize;
        let mut elapsed = 0.0;
        for i in 0..inputs.n {
            if elapsed >= 8.0 {
                break;
            }
            cost_8h += (inputs.c_imp_eur_kwh[i] * p1.p_imp_kw[i]
                - inputs.c_exp_eur_kwh[i] * p1.p_exp_kw[i])
                * inputs.dt_h[i];
            if i > 0 && (p1.y_heat[i] - p1.y_heat[i - 1]).abs() > 1e-6 {
                sw_8h += 1;
            }
            elapsed += inputs.dt_h[i];
        }
        println!(
            "  {label:>34} {n:>6} {secs:>9.2} {sw_8h:>11} {cost_8h:>13.4} {:>12.4}  {:?}",
            p1.objective_eur, p1.status
        );
        emit_result(&format!(
            r#"{{"params":{{"grid":"{label}","slots":{n},"hours":{hours:.0},"mip_gap":0.06,"volume_l":200,"band_k":15}},"results":{{"phase1_s":{secs:.3},"switches_8h":{sw_8h},"first_8h_eur":{cost_8h:.4},"objective_eur":{:.4},"status":"{:?}"}}}}"#,
            p1.objective_eur, p1.status
        ));
    }
    println!(
        "
  first-8h EUR and switches 8h guard the executed window: coarsening the far
           zones is only free if those hold while phase1 s falls.
"
    );
}

/// R-97: bisect the 3-12x gap between the bench and live ven-3.
///
/// At the production gap (0.06) the synthetic bench solves ven-3's grid and tank in
/// 3.9-5.3 s, while the `planner: phase timings` log shows live ven-3 at
/// 11 131-45 566 ms. The heater parameters are already faithful (200 L, 45-60 C,
/// k_loss 0.005, draw 0.3, switching 0.50 — all match ven-3.yaml), so the gap is
/// elsewhere. The bench differs from the real profile in three places:
///
///   - `spikes: vec![]` vs three real appliance spikes (coffee/lunch/dinner)
///   - PV rated 6.0 kW vs 8.0 kW (inverter 7.5)
///   - heater `temp_initial_c` 47.82 vs 50.0
///
/// Added one at a time, so whichever carries the cost is attributable rather than
/// guessed. A spiky base load is the prime suspect: it makes the net-load shape far
/// richer, and every spike is a window where the heater's placement interacts with
/// a load peak.
///
///   bash scripts/run_planner_experiment.sh bench_bisect_ven3_gap
#[test]
#[ignore = "R-97: 5 heater-sized phase-1 solves, run with --ignored --nocapture"]
fn bench_bisect_ven3_gap() {
    use crate::entities::asset_params::ApplianceSpikeParams;
    // ven-3.yaml's real spikes.
    let real_spikes = || {
        vec![
            ApplianceSpikeParams {
                center_hour: 8.0,
                jitter_h: 0.2,
                amplitude_kw: 1.2,
                duration_h: 0.25,
                ramp_h: 0.03,
                probability: 1.0,
                weekdays: vec![0, 1, 2, 3, 4],
            },
            ApplianceSpikeParams {
                center_hour: 12.0,
                jitter_h: 0.25,
                amplitude_kw: 2.0,
                duration_h: 0.5,
                ramp_h: 0.05,
                probability: 0.6,
                weekdays: vec![0, 1, 2, 3, 4],
            },
            ApplianceSpikeParams {
                center_hour: 18.0,
                jitter_h: 0.3,
                amplitude_kw: 2.5,
                duration_h: 0.75,
                ramp_h: 0.08,
                probability: 1.0,
                weekdays: vec![0, 1, 2, 3, 4],
            },
        ]
    };

    println!("\n── R-97: bisecting the bench-vs-live-ven-3 phase-1 gap ──");
    println!("   (gap 0.06, 288-slot production grid, ven-3's 200 L / 45-60 C tank)\n");
    println!(
        "  {:>38} {:>10} {:>11} {:>13}  status",
        "variant", "phase1 s", "switches", "objective"
    );
    for (label, spikes, pv_kw, temp_init) in [
        ("bench baseline (flat load, 6 kW PV)", false, 6.0, 47.82),
        ("+ ven-3 base-load spikes", true, 6.0, 47.82),
        ("+ ven-3 PV 8.0 kW", false, 8.0, 47.82),
        ("+ ven-3 temp_initial 50 C", false, 6.0, 50.0),
        ("all three (closest to live ven-3)", true, 8.0, 50.0),
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        profile.planner.mip_gap_target = 0.06;
        let thermal_mass = 200.0 * 4.186 / 3600.0;
        for a in profile.assets.iter_mut() {
            match a {
                crate::entities::asset_params::AssetParams::Heater(h) => {
                    h.thermal_mass_kwh_per_c = thermal_mass;
                    h.temp_min_c = 45.0;
                    h.temp_max_c = 60.0;
                    h.temp_safety_max_c = 60.0;
                    h.temp_initial_c = temp_init;
                    h.switching_penalty_eur = 0.50;
                }
                crate::entities::asset_params::AssetParams::BaseLoad(b) => {
                    if spikes {
                        b.spikes = real_spikes();
                    }
                }
                crate::entities::asset_params::AssetParams::Pv(pv) => {
                    pv.rated_kw = pv_kw;
                    pv.inverter_max_kw = pv_kw.min(7.5);
                }
                _ => {}
            }
        }
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, temp_init);
        set_heater_power(&mut sim, 6.0);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let t = Instant::now();
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let secs = t.elapsed().as_secs_f64();
        let switches = (1..inputs.n)
            .filter(|&i| (p1.y_heat[i] - p1.y_heat[i - 1]).abs() > 1e-6)
            .count();
        println!(
            "  {label:>38} {secs:>10.2} {switches:>11} {:>13.4}  {:?}",
            p1.objective_eur, p1.status
        );
        emit_result(&format!(
            r#"{{"params":{{"variant":"{label}","spikes":{spikes},"pv_rated_kw":{pv_kw},"temp_initial_c":{temp_init},"mip_gap":0.06,"slots":{},"volume_l":200,"band_k":15}},"results":{{"phase1_s":{secs:.3},"switches":{switches},"objective_eur":{:.4},"status":"{:?}"}}}}"#,
            inputs.n, p1.objective_eur, p1.status
        ));
    }
    println!(
        "\n  Live ven-3 phase 1: 11 131-45 566 ms. Whichever row approaches that is the\n  \
         cause; if none does, the remaining difference is live state (real tariff\n  \
         shape, actual SoC/tank, capacity events) rather than the profile.\n"
    );
}

/// R-97: do the EV/battery startup binaries make phase 2 unsolvable?
///
/// `declare_vars` emits `n-1` **binary** `delta_ev` variables whenever
/// `c_ev_startup_eur > 0`, plus `n-1` continuous ramp variables when
/// `c_ev_ramp_eur_kw > 0` — and the same for the battery. Those weights are
/// **phase-2 only** (`build_phase2_weights`), and their defaults are non-zero:
/// startup 0.01 EUR, ramp 0.005 EUR/kW. No fleet profile overrides them.
///
/// So phase 2 carries ~287 extra binaries per storage asset in order to express a
/// 0.01 EUR anti-chatter preference, on top of a hard cost cap — and phase 2 is
/// precisely the half that never proves optimality (its friction is non-monotone in
/// epsilon, which is only possible for suboptimal incumbents).
///
/// This measures phase-2 time against those terms. **The behavioural columns are the
/// point**: startup costs exist to stop the EV and battery chattering, so a faster
/// phase 2 is only acceptable if EV/battery switching does not grow. Changing the
/// character of VEN behaviour is not on the table.
///
///   bash scripts/run_planner_experiment.sh bench_phase2_startup_binaries 2
#[test]
#[ignore = "R-97: 4 heater-sized two-phase solves, run with --ignored --nocapture"]
fn bench_phase2_startup_binaries() {
    println!("\n── R-97: phase-2 cost of the EV/battery startup+ramp binaries ──");
    println!("   (ven-3 tank, gap 0.06, epsilon 1.00, diurnal prices, 288 slots)\n");
    println!(
        "  {:>30} {:>9} {:>11} {:>10} {:>10} {:>10}  p2 status",
        "variant", "p2 s", "friction", "heat sw", "ev sw", "bat sw"
    );
    for (label, startup, ramp) in [
        ("defaults (0.01 / 0.005)", 0.01, 0.005),
        ("startup 0, ramp kept", 0.0, 0.005),
        ("startup kept, ramp 0", 0.01, 0.0),
        ("both 0 (no delta vars)", 0.0, 0.0),
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        profile.planner.mip_gap_target = 0.06;
        profile.planner.phase2_epsilon_eur = 1.00;
        profile.planner.c_ev_startup_eur = startup;
        profile.planner.c_bat_startup_eur = startup;
        profile.planner.c_ev_ramp_eur_kw = ramp;
        profile.planner.c_bat_ramp_eur_kw = ramp;
        let thermal_mass = 200.0 * 4.186 / 3600.0;
        for a in profile.assets.iter_mut() {
            if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                h.thermal_mass_kwh_per_c = thermal_mass;
                h.temp_min_c = 45.0;
                h.temp_max_c = 60.0;
                h.temp_safety_max_c = 60.0;
                h.temp_initial_c = 47.82;
            }
        }
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, 47.82);
        set_heater_power(&mut sim, 6.0);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let p2w = build_phase2_weights(&inputs, &profile.planner);
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");

        let t = Instant::now();
        let r = solve_phase2(
            &inputs,
            &p1w,
            &p2w,
            p1.objective_eur,
            profile.planner.phase2_epsilon_eur,
            &p1,
            &ctxs,
            60.0,
        );
        let p2_s = t.elapsed().as_secs_f64();
        let (p2, friction, status) = match r {
            Ok((sol, f)) => {
                let st = format!("{:?}", sol.status);
                (sol, f, st)
            }
            Err(_) => {
                println!("  {label:>30} {p2_s:>9.2}   phase 2 Err");
                continue;
            }
        };
        // Behaviour: how often each controllable asset starts/stops. These must not
        // grow, or the anti-chatter purpose of the startup cost has been defeated.
        let onoff = |v: &[f64]| {
            (1..v.len())
                .filter(|&i| (v[i] > 1e-6) != (v[i - 1] > 1e-6))
                .count()
        };
        println!(
            "  {label:>30} {p2_s:>9.2} {friction:>11.4} {:>10} {:>10} {:>10}  {status}",
            (1..inputs.n)
                .filter(|&i| (p2.y_heat[i] - p2.y_heat[i - 1]).abs() > 1e-6)
                .count(),
            onoff(&p2.p_ev_kw),
            onoff(&p2.p_bat_ch_kw) + onoff(&p2.p_bat_dis_kw),
        );
        emit_result(&format!(
            r#"{{"params":{{"c_startup_eur":{startup},"c_ramp_eur_kw":{ramp},"mip_gap":0.06,"epsilon":1.0,"slots":{},"volume_l":200,"band_k":15}},"results":{{"phase2_s":{p2_s:.3},"friction_eur":{friction:.4},"heat_switches":{},"ev_onoff":{},"bat_onoff":{},"p2_status":"{status}"}}}}"#,
            inputs.n,
            (1..inputs.n)
                .filter(|&i| (p2.y_heat[i] - p2.y_heat[i - 1]).abs() > 1e-6)
                .count(),
            onoff(&p2.p_ev_kw),
            onoff(&p2.p_bat_ch_kw) + onoff(&p2.p_bat_dis_kw),
        ));
    }
    println!(
        "\n  A faster phase 2 is only acceptable if ev sw and bat sw do NOT rise —\n  \
         that is what the startup cost exists to prevent.\n"
    );
}

/// R-97: solve cost across the fleet's real asset-mix classes.
///
/// Every bench so far has used one mix (EV + heater + PV + base load). The fleet
/// actually has four classes, and the heaviest one has never been measured here:
///
///   heater + battery : ven-5, ven-14, ven-17   <- never tested; ven-5 was GB-40's worst (120 s)
///   heater only      : ven-2, ven-3, ven-10, ven-12, ven-15, ven-18, ven-20
///   battery only     : ven-1, ven-4, ven-6, ven-13, ven-16, ven-19
///   neither          : ven-7, ven-8, ven-11
///
/// A battery adds its own direction binaries and SoC trajectory on top of the
/// heater's tier integers, so this is where binary interaction should show up if it
/// shows up anywhere. Battery parameters are ven-5's (11 kWh, 5.5 kW, eta 0.92,
/// min_soc 0.10). Both phases timed separately.
///
///   bash scripts/run_planner_experiment.sh bench_asset_mix_solve_cost 2
#[test]
#[ignore = "R-97: 4 two-phase heater-sized solves, run with --ignored --nocapture"]
fn bench_asset_mix_solve_cost() {
    use crate::entities::asset_params::BatteryParams;
    println!("\n── R-97: solve cost by fleet asset-mix class ──");
    println!("   (gap 0.06, epsilon 1.00, 288 slots, ven-3 tank where a heater is present)\n");
    println!(
        "  {:>34} {:>9} {:>9} {:>12} {:>12}  statuses",
        "class", "phase1 s", "phase2 s", "p1 status", "p2 status"
    );
    for (label, heater, battery) in [
        ("heater + battery (ven-5/14/17)", true, true),
        ("heater only (ven-2/3/...)", true, false),
        ("battery only (ven-1/4/...)", false, true),
        ("neither (ven-7/8/11)", false, false),
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, heater);
        profile.planner.mip_gap_target = 0.06;
        profile.planner.phase2_epsilon_eur = 1.00;
        if heater {
            let thermal_mass = 200.0 * 4.186 / 3600.0;
            for a in profile.assets.iter_mut() {
                if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                    h.thermal_mass_kwh_per_c = thermal_mass;
                    h.temp_min_c = 45.0;
                    h.temp_max_c = 60.0;
                    h.temp_safety_max_c = 60.0;
                    h.temp_initial_c = 47.82;
                }
            }
        }
        if battery {
            // ven-5's battery.
            profile
                .assets
                .push(crate::entities::asset_params::AssetParams::Battery(
                    BatteryParams {
                        id: "battery".into(),
                        capacity_kwh: 11.0,
                        max_charge_kw: 5.5,
                        max_discharge_kw: 5.5,
                        initial_soc: 0.50,
                        round_trip_efficiency: 0.92,
                        min_soc: 0.10,
                        c_terminal_eur_kwh: None,
                    },
                ));
        }
        let mut sim = make_snap_from_profile(&profile);
        if heater {
            set_heater_temp(&mut sim, 47.82);
            set_heater_power(&mut sim, 6.0);
        }
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let p2w = build_phase2_weights(&inputs, &profile.planner);

        let t = Instant::now();
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let p1_s = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let r = solve_phase2(
            &inputs,
            &p1w,
            &p2w,
            p1.objective_eur,
            1.00,
            &p1,
            &ctxs,
            60.0,
        );
        let p2_s = t.elapsed().as_secs_f64();
        let p2_status = match &r {
            Ok((sol, _)) => format!("{:?}", sol.status),
            Err(_) => "Err".to_string(),
        };
        println!(
            "  {label:>34} {p1_s:>9.2} {p2_s:>9.2} {:>12?} {p2_status:>12}",
            p1.status
        );
        emit_result(&format!(
            r#"{{"params":{{"class":"{label}","heater":{heater},"battery":{battery},"mip_gap":0.06,"epsilon":1.0,"slots":{}}},"results":{{"phase1_s":{p1_s:.3},"phase2_s":{p2_s:.3},"p1_status":"{:?}","p2_status":"{p2_status}"}}}}"#,
            inputs.n, p1.status
        ));
    }
    println!(
        "\n  Live GB-40 reference: ven-5 (heater+battery) was its worst at 120 s.\n  \
         If heater+battery is far worse here too, binary interaction is real and the\n  \
         fleet's hard cases are the three VENs with both.\n"
    );
}

/// R-97: the minimum epsilon that buys smoothing, per asset-mix class.
///
/// Two open questions from the asset-mix result:
///   1. Can phase 2 succeed at **any** epsilon on heater+battery, or is
///      `phase2_epsilon_eur = 0.0` (disable) the only honest setting for
///      ven-5/14/17? Production shows `NoSolutionFound` at the 0.02 default on
///      every cycle.
///   2. For heater-only VENs, what is the *minimum* epsilon that actually moves the
///      schedule? ven-2 runs 1.00 and works, but that value was chosen as "2x the
///      effective switching cost", not measured. Recommending it elsewhere by
///      analogy is a guess; a tighter working value gives the solver less cost
///      slack to spend, which is strictly better for plan cost.
///
/// Phase 1 does not depend on epsilon, so it is solved once per class and reused
/// across the sweep.
///
///   bash scripts/run_planner_experiment.sh bench_min_epsilon_by_class 2
#[test]
#[ignore = "R-97: 2 phase-1 + 16 phase-2 solves, run with --ignored --nocapture"]
fn bench_min_epsilon_by_class() {
    use crate::entities::asset_params::BatteryParams;
    const P2_BUDGET_S: f64 = 30.0;
    println!("\n── R-97: minimum working epsilon by asset-mix class ──");
    println!("   (gap 0.06, 288 slots, ven-3 tank, phase-2 budget {P2_BUDGET_S:.0} s)\n");
    for (class, battery) in [("heater only", false), ("heater + battery", true)] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        profile.planner.mip_gap_target = 0.06;
        let thermal_mass = 200.0 * 4.186 / 3600.0;
        for a in profile.assets.iter_mut() {
            if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                h.thermal_mass_kwh_per_c = thermal_mass;
                h.temp_min_c = 45.0;
                h.temp_max_c = 60.0;
                h.temp_safety_max_c = 60.0;
                h.temp_initial_c = 47.82;
            }
        }
        if battery {
            profile
                .assets
                .push(crate::entities::asset_params::AssetParams::Battery(
                    BatteryParams {
                        id: "battery".into(),
                        capacity_kwh: 11.0,
                        max_charge_kw: 5.5,
                        max_discharge_kw: 5.5,
                        initial_soc: 0.50,
                        round_trip_efficiency: 0.92,
                        min_soc: 0.10,
                        c_terminal_eur_kwh: None,
                    },
                ));
        }
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, 47.82);
        set_heater_power(&mut sim, 6.0);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        // Phase 1 is independent of epsilon — solve once, reuse across the sweep.
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let p1_switches = (1..inputs.n)
            .filter(|&i| (p1.y_heat[i] - p1.y_heat[i - 1]).abs() > 1e-6)
            .count();
        println!(
            "  == {class}: phase 1 {:?}, {p1_switches} heater switches\n",
            p1.status
        );
        println!(
            "  {:>9} {:>9} {:>10} {:>12} {:>11}  p2 status",
            "epsilon", "p2 s", "heat moved", "heat switches", "friction"
        );
        for epsilon in [0.02, 0.1, 0.2, 0.3, 0.5, 0.75, 1.0, 2.0] {
            let mut pr = profile.clone();
            pr.planner.phase2_epsilon_eur = epsilon;
            let p2w = build_phase2_weights(&inputs, &pr.planner);
            let t = Instant::now();
            let r = solve_phase2(
                &inputs,
                &p1w,
                &p2w,
                p1.objective_eur,
                epsilon,
                &p1,
                &ctxs,
                P2_BUDGET_S,
            );
            let secs = t.elapsed().as_secs_f64();
            match r {
                Ok((p2, friction)) => {
                    let moved = (0..inputs.n)
                        .filter(|&i| (p1.y_heat[i] - p2.y_heat[i]).abs() > 1e-6)
                        .count();
                    let sw = (1..inputs.n)
                        .filter(|&i| (p2.y_heat[i] - p2.y_heat[i - 1]).abs() > 1e-6)
                        .count();
                    println!(
                        "  {epsilon:>9.2} {secs:>9.2} {moved:>10} {sw:>12} {friction:>11.4}  {:?}",
                        p2.status
                    );
                    emit_result(&format!(
                        r#"{{"params":{{"class":"{class}","battery":{battery},"epsilon":{epsilon},"mip_gap":0.06,"p2_budget_s":{P2_BUDGET_S},"slots":{}}},"results":{{"phase2_s":{secs:.3},"heat_slots_moved":{moved},"heat_switches":{sw},"p1_heat_switches":{p1_switches},"friction_eur":{friction:.4},"p2_status":"{:?}"}}}}"#,
                        inputs.n, p2.status
                    ));
                }
                Err(e) => {
                    let msg = format!("{e}");
                    let short = msg
                        .split(':')
                        .next_back()
                        .unwrap_or("Err")
                        .trim()
                        .to_string();
                    println!(
                        "  {epsilon:>9.2} {secs:>9.2} {:>10} {:>12} {:>11}  Err: {short}",
                        "-", "-", "-"
                    );
                    emit_result(&format!(
                        r#"{{"params":{{"class":"{class}","battery":{battery},"epsilon":{epsilon},"mip_gap":0.06,"p2_budget_s":{P2_BUDGET_S},"slots":{}}},"results":{{"phase2_s":{secs:.3},"p2_status":"Err","error":"{short}"}}}}"#,
                        inputs.n
                    ));
                }
            }
        }
        println!();
    }
    println!(
        "  'heat moved' is slots differing from phase 1 — zero means phase 2 achieved\n  \
         nothing. The lowest epsilon with a non-zero count is the minimum working value;\n  \
         if heater+battery has none, 0.0 (disable) is the only honest setting there.\n"
    );
}

/// R-97 **bug**: the phase-2 warm start is infeasible whenever the battery discharges.
///
/// `u_bat` is the battery's *direction selector*, not an activity flag
/// (`battery_milp.rs`):
///
/// ```text
/// p_ch[t]  <= ch_max  * u_bat[t]          u_bat = 1 -> charging allowed
/// p_dis[t] <= dis_max * (1 - u_bat[t])    u_bat = 1 -> discharging forced to 0
/// ```
///
/// but `build_phase2_warm_start` sets it from an *activity* test:
///
/// ```text
/// let active = if p_ch > 0 || p_dis > 0 { 1.0 } else { 0.0 };
/// iv.push((v.u_bat[t], active));
/// ```
///
/// So in every slot where phase 1 discharges the battery, the warm start asserts
/// `u_bat = 1` alongside `p_dis > 0`, violating `p_dis <= dis_max * (1 - u_bat) = 0`.
/// The start phase 2 is handed is therefore infeasible, and it must find a solution
/// from scratch — which it manages on an easy instance (battery-only converges) and
/// fails on a hard one. That is exactly the production symptom: ven-5, ven-14 and
/// ven-17 (the three heater+battery VENs) log `NoSolutionFound` on essentially every
/// cycle, 29-33 times per 3 h, while ven-11 never does.
///
/// `z_active` is a genuine activity flag (`p_ch + p_dis <= big_m * z_active`), so its
/// warm-start value is correct and must stay as it is.
#[test]
fn phase2_warm_start_respects_the_battery_direction_selector() {
    use crate::entities::asset_params::BatteryParams;
    let now = fixed_now();
    let mut profile = ev_bench_profile_with_heater(true, 20.0, false);
    // Small grid: this is a correctness test, not a benchmark.
    profile.planner.plan_zones = vec![crate::entities::plan::PlanZone {
        step_s: 900,
        slots: 16,
    }];
    profile
        .assets
        .push(crate::entities::asset_params::AssetParams::Battery(
            BatteryParams {
                id: "battery".into(),
                capacity_kwh: 11.0,
                max_charge_kw: 5.5,
                max_discharge_kw: 5.5,
                initial_soc: 0.90, // nearly full, so discharging is attractive
                round_trip_efficiency: 0.92,
                min_soc: 0.10,
                c_terminal_eur_kwh: Some(0.0), // no terminal reward, so it will discharge
            },
        ));
    let sim = make_snap_from_profile(&profile);
    // Expensive import makes discharging the cheap option.
    let tariffs = make_tariffs(0.60, 0.08, 300.0);
    let cap = no_capacity();
    let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
    let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
    let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
    let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");

    let discharging: Vec<usize> = (0..inputs.n)
        .filter(|&t| p1.p_bat_dis_kw[t] > 1e-6)
        .collect();
    assert!(
        !discharging.is_empty(),
        "this test needs phase 1 to discharge the battery; it did not, so the fixture \
         no longer exercises the bug. p_bat_dis_kw = {:?}",
        p1.p_bat_dis_kw
    );

    // Rebuild phase 2's variable pool exactly as solve_phase2 does, then inspect the
    // warm start it would be given.
    let (iv, u_bat) =
        crate::controller::milp_planner::solver_phase2::warm_start_for_test(&inputs, &p1, &ctxs);
    let lookup: std::collections::HashMap<_, _> = iv.into_iter().collect();
    for t in discharging {
        let v = lookup
            .get(&u_bat[t])
            .copied()
            .unwrap_or_else(|| panic!("u_bat[{t}] missing from the warm start"));
        assert!(
            v < 0.5,
            "slot {t}: phase 1 discharges at {:.3} kW, so the warm start must set the \
             direction selector u_bat to 0 (discharge). It is {v}, which asserts \
             'charging' and makes p_dis <= dis_max * (1 - 1) = 0 infeasible — the start \
             phase 2 is handed cannot be used, and on a heater+battery instance it then \
             reports NoSolutionFound (R-97).",
            p1.p_bat_dis_kw[t]
        );
    }
}

/// R-97: is a "working" epsilon repeatable, or did it get lucky once?
///
/// `bench_min_epsilon_by_class` found exactly one epsilon in eight producing a large
/// heater improvement, at a different value per class (1.00 heater-only, 0.75
/// heater+battery). Since raising epsilon strictly enlarges the feasible set, a value
/// that works at 0.75 cannot legitimately fail at 1.00 — so either the search is
/// landing on incumbents by luck, or those particular values are genuinely special.
///
/// This decides it: hold epsilon at the value that worked for each class and repeat.
/// Phase 1 is solved once (it does not depend on epsilon) and reused, so every repeat
/// starts from an identical warm start and the only variable is the solver's own
/// search.
///
/// - Improvement found every time -> the value is recommendable per class.
/// - Found sometimes -> it is a lottery, and no single-sweep epsilon recommendation
///   is sound. Phase 2's contribution would then be inherently intermittent.
///
///   bash scripts/run_planner_experiment.sh bench_epsilon_repeatability 2
#[test]
#[ignore = "R-97: 2 phase-1 + 10 phase-2 solves, run with --ignored --nocapture"]
fn bench_epsilon_repeatability() {
    use crate::entities::asset_params::BatteryParams;
    const REPEATS: usize = 5;
    const P2_BUDGET_S: f64 = 30.0;
    println!("\n── R-97: repeatability of a working epsilon ──");
    println!("   (gap 0.06, 288 slots, {REPEATS} repeats, phase-2 budget {P2_BUDGET_S:.0} s)\n");
    for (class, battery, epsilon) in [
        ("heater only", false, 1.00),
        ("heater + battery", true, 0.75),
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        profile.planner.mip_gap_target = 0.06;
        profile.planner.phase2_epsilon_eur = epsilon;
        let thermal_mass = 200.0 * 4.186 / 3600.0;
        for a in profile.assets.iter_mut() {
            if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                h.thermal_mass_kwh_per_c = thermal_mass;
                h.temp_min_c = 45.0;
                h.temp_max_c = 60.0;
                h.temp_safety_max_c = 60.0;
                h.temp_initial_c = 47.82;
            }
        }
        if battery {
            profile
                .assets
                .push(crate::entities::asset_params::AssetParams::Battery(
                    BatteryParams {
                        id: "battery".into(),
                        capacity_kwh: 11.0,
                        max_charge_kw: 5.5,
                        max_discharge_kw: 5.5,
                        initial_soc: 0.50,
                        round_trip_efficiency: 0.92,
                        min_soc: 0.10,
                        c_terminal_eur_kwh: None,
                    },
                ));
        }
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, 47.82);
        set_heater_power(&mut sim, 6.0);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let p2w = build_phase2_weights(&inputs, &profile.planner);
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let p1_sw = (1..inputs.n)
            .filter(|&i| (p1.y_heat[i] - p1.y_heat[i - 1]).abs() > 1e-6)
            .count();

        println!("  == {class} at epsilon {epsilon:.2} (phase 1: {p1_sw} heater switches)");
        let mut improved = 0usize;
        for r in 1..=REPEATS {
            let t = Instant::now();
            let res = solve_phase2(
                &inputs,
                &p1w,
                &p2w,
                p1.objective_eur,
                epsilon,
                &p1,
                &ctxs,
                P2_BUDGET_S,
            );
            let secs = t.elapsed().as_secs_f64();
            match res {
                Ok((p2, friction)) => {
                    let moved = (0..inputs.n)
                        .filter(|&i| (p1.y_heat[i] - p2.y_heat[i]).abs() > 1e-6)
                        .count();
                    let sw = (1..inputs.n)
                        .filter(|&i| (p2.y_heat[i] - p2.y_heat[i - 1]).abs() > 1e-6)
                        .count();
                    if moved > 0 {
                        improved += 1;
                    }
                    println!(
                        "     run {r}: {secs:>6.2} s  moved {moved:>3}  switches {sw:>3}  friction {friction:>8.4}  {:?}",
                        p2.status
                    );
                    emit_result(&format!(
                        r#"{{"params":{{"class":"{class}","battery":{battery},"epsilon":{epsilon},"run":{r},"mip_gap":0.06,"p2_budget_s":{P2_BUDGET_S},"slots":{}}},"results":{{"phase2_s":{secs:.3},"heat_slots_moved":{moved},"heat_switches":{sw},"p1_heat_switches":{p1_sw},"friction_eur":{friction:.4},"p2_status":"{:?}"}}}}"#,
                        inputs.n, p2.status
                    ));
                }
                Err(e) => println!("     run {r}: {secs:>6.2} s  Err: {e}"),
            }
        }
        println!("     -> improvement found in {improved}/{REPEATS} runs\n");
    }
    println!(
        "  {REPEATS}/{REPEATS} means the value is recommendable for that class; anything less\n  \
         means phase 2's contribution is intermittent and no epsilon can be tuned from\n  \
         a single sweep.\n"
    );
}

/// R-97: does a working epsilon survive a change of instance?
///
/// Phase 2 is deterministic per configuration, so a measured epsilon holds — for
/// *that* instance. Live VENs re-solve every 300 s against a different tank
/// temperature, SoC and price window, so the practical question is whether one value
/// keeps working as state moves.
///
/// This sweeps epsilon against several start conditions drawn from the band a real
/// tank occupies (45-60 C) at both heater stages. If one epsilon improves on every
/// instance, per-VEN tuning is viable in production. If the working value jumps
/// between instances, then no fixed epsilon can be relied on and phase 2's
/// contribution is intermittent in practice even though each solve is deterministic.
///
///   bash scripts/run_planner_experiment.sh bench_epsilon_across_instances 2
#[test]
#[ignore = "R-97: 4 phase-1 + 24 phase-2 solves (~15 min), run with --ignored --nocapture"]
fn bench_epsilon_across_instances() {
    const P2_BUDGET_S: f64 = 15.0; // production budget
    println!("\n── R-97: does one epsilon work across instances? ──");
    println!(
        "   (heater only, gap 0.06, 288 slots, phase-2 budget {P2_BUDGET_S:.0} s = production)\n"
    );
    println!(
        "  {:>28} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "instance", "e=0.3", "e=0.5", "e=0.75", "e=1.0", "e=1.5", "e=2.0"
    );
    for (label, temp_c, initial_kw) in [
        ("cool tank, full power", 47.82, 6.0),
        ("near T_min, off", 46.0, 0.0),
        ("mid-band, mid stage", 52.0, 3.0),
        ("warm tank, off", 57.0, 0.0),
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        profile.planner.mip_gap_target = 0.06;
        let thermal_mass = 200.0 * 4.186 / 3600.0;
        for a in profile.assets.iter_mut() {
            if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                h.thermal_mass_kwh_per_c = thermal_mass;
                h.temp_min_c = 45.0;
                h.temp_max_c = 60.0;
                h.temp_safety_max_c = 60.0;
                h.temp_initial_c = temp_c;
            }
        }
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, temp_c);
        set_heater_power(&mut sim, initial_kw);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let p1_sw = (1..inputs.n)
            .filter(|&i| (p1.y_heat[i] - p1.y_heat[i - 1]).abs() > 1e-6)
            .count();

        let mut cells = Vec::new();
        for epsilon in [0.3, 0.5, 0.75, 1.0, 1.5, 2.0] {
            let mut pr = profile.clone();
            pr.planner.phase2_epsilon_eur = epsilon;
            let p2w = build_phase2_weights(&inputs, &pr.planner);
            let r = solve_phase2(
                &inputs,
                &p1w,
                &p2w,
                p1.objective_eur,
                epsilon,
                &p1,
                &ctxs,
                P2_BUDGET_S,
            );
            let cell = match r {
                Ok((p2, friction)) => {
                    let sw = (1..inputs.n)
                        .filter(|&i| (p2.y_heat[i] - p2.y_heat[i - 1]).abs() > 1e-6)
                        .count();
                    emit_result(&format!(
                        r#"{{"params":{{"instance":"{label}","temp_c":{temp_c},"initial_kw":{initial_kw},"epsilon":{epsilon},"mip_gap":0.06,"p2_budget_s":{P2_BUDGET_S},"slots":{}}},"results":{{"p1_heat_switches":{p1_sw},"heat_switches":{sw},"friction_eur":{friction:.4},"p2_status":"{:?}"}}}}"#,
                        inputs.n, p2.status
                    ));
                    // Switches after phase 2 vs after phase 1: lower is better.
                    if sw < p1_sw {
                        format!("{sw}<{p1_sw}")
                    } else {
                        format!("={p1_sw}")
                    }
                }
                Err(_) => "Err".to_string(),
            };
            cells.push(cell);
        }
        println!(
            "  {label:>28} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            cells[0], cells[1], cells[2], cells[3], cells[4], cells[5]
        );
    }
    println!(
        "\n  Cells show heater switches after phase 2 against after phase 1. '=N' means\n  \
         phase 2 changed nothing. A column that improves on every row is an epsilon\n  \
         that can be set per VEN; if the improving column moves between rows, no fixed\n  \
         value is dependable as live state changes.\n"
    );
}

/// R-97: when phase 2 *does* smooth, does any of it reach the relay?
///
/// The cross-instance sweep shows phase 2 roughly halving heater switches when it
/// fires (58 -> 26, 54 -> 24), but only on some instances at some epsilons. Whether
/// that is worth 15 s per cycle hinges on something else entirely: a plan is replaced
/// every `replan_interval_s` (300 s), so only its first slots are ever executed.
///
/// This takes the instances and epsilons where phase 2 demonstrably improves the
/// horizon-wide schedule, and asks how many of those switches fall inside the
/// executed window.
///
/// - Executed window unchanged -> phase 2's smoothing never reaches the hardware,
///   and `phase2_epsilon_eur = 0.0` saves 15 s/cycle fleet-wide at no behavioural
///   cost. That would be the largest clean win available.
/// - Executed window improves -> the 15 s buys real relay life and the intermittency
///   is a genuine cost to live with.
///
///   bash scripts/run_planner_experiment.sh bench_phase2_smoothing_reaches_relay 2
#[test]
#[ignore = "R-97: 6 two-phase solves, run with --ignored --nocapture"]
fn bench_phase2_smoothing_reaches_relay() {
    const P2_BUDGET_S: f64 = 15.0;
    println!("\n── R-97: does phase 2's smoothing reach the executed window? ──");
    println!("   (replan every 300 s, so ~25 min is already five cycles ahead)\n");
    println!(
        "  {:>26} {:>7} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "instance / phase", "eps", "sw 25min", "sw 1h", "sw 4h", "sw 8h", "sw 48h"
    );
    // The two (instance, epsilon) pairs the cross-instance sweep showed improving.
    for (label, temp_c, initial_kw, epsilon, battery) in [
        ("cool tank, full", 47.82, 6.0, 1.00, false),
        ("near T_min, off", 46.0, 0.0, 1.00, false),
        ("mid-band, mid", 52.0, 3.0, 0.75, false),
        ("warm tank, off", 57.0, 0.0, 1.00, false),
        ("cool+battery", 47.82, 6.0, 0.75, true),
        ("mid-band+battery", 52.0, 3.0, 0.75, true),
    ] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        if battery {
            profile
                .assets
                .push(crate::entities::asset_params::AssetParams::Battery(
                    crate::entities::asset_params::BatteryParams {
                        id: "battery".into(),
                        capacity_kwh: 11.0,
                        max_charge_kw: 5.5,
                        max_discharge_kw: 5.5,
                        initial_soc: 0.50,
                        round_trip_efficiency: 0.92,
                        min_soc: 0.10,
                        c_terminal_eur_kwh: None,
                    },
                ));
        }
        profile.planner.mip_gap_target = 0.06;
        profile.planner.phase2_epsilon_eur = epsilon;
        let thermal_mass = 200.0 * 4.186 / 3600.0;
        for a in profile.assets.iter_mut() {
            if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                h.thermal_mass_kwh_per_c = thermal_mass;
                h.temp_min_c = 45.0;
                h.temp_max_c = 60.0;
                h.temp_safety_max_c = 60.0;
                h.temp_initial_c = temp_c;
            }
        }
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, temp_c);
        set_heater_power(&mut sim, initial_kw);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let p2w = build_phase2_weights(&inputs, &profile.planner);
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let (p2, _friction) = solve_phase2(
            &inputs,
            &p1w,
            &p2w,
            p1.objective_eur,
            epsilon,
            &p1,
            &ctxs,
            P2_BUDGET_S,
        )
        .expect("phase 2 feasible");

        let sw_within = |y: &[f64], hours: f64| -> usize {
            let mut n = 0;
            let mut elapsed = 0.0;
            for t in 1..inputs.n {
                if elapsed >= hours {
                    break;
                }
                if (y[t] - y[t - 1]).abs() > 1e-6 {
                    n += 1;
                }
                elapsed += inputs.dt_h[t];
            }
            n
        };
        for (phase, y) in [("phase 1", &p1.y_heat), ("phase 2", &p2.y_heat)] {
            println!(
                "  {:>26} {epsilon:>7.2} {:>9} {:>9} {:>9} {:>9} {:>9}",
                format!("{label} / {phase}"),
                sw_within(y, 25.0 / 60.0),
                sw_within(y, 1.0),
                sw_within(y, 4.0),
                sw_within(y, 8.0),
                sw_within(y, 48.0),
            );
            emit_result(&format!(
                r#"{{"params":{{"instance":"{label}","phase":"{phase}","epsilon":{epsilon},"temp_c":{temp_c},"initial_kw":{initial_kw},"battery":{battery},"mip_gap":0.06,"p2_budget_s":{P2_BUDGET_S},"slots":{}}},"results":{{"sw_25min":{},"sw_1h":{},"sw_4h":{},"sw_8h":{},"sw_48h":{}}}}}"#,
                inputs.n,
                sw_within(y, 25.0 / 60.0),
                sw_within(y, 1.0),
                sw_within(y, 4.0),
                sw_within(y, 8.0),
                sw_within(y, 48.0),
            ));
        }
    }
    println!(
        "\n  If the 25min and 1h columns match between phase 1 and phase 2, the smoothing\n  \
         never reaches the relay and the 15 s buys nothing the hardware feels.\n"
    );
}

/// R-97: can a looser gap get heater+battery phase 1 off the ceiling?
///
/// Post-warm-start-fix production timings show ven-5's phase 1 at 36-60 s, hitting
/// the 60 s limit on most cycles, while phase 2 is bounded at 15 s. So phase 1 on
/// the heater+battery class is now the dominant cost in the fleet, and the bench
/// agrees (57.8 s against 3.83 s for heater-only).
///
/// GB-40 measured the `mip_gap_target` lever across ten instances — 10 % puts phase
/// 1 on GapLimit for nine of them at +2.05 % mean objective cost — but never on this
/// asset mix, which did not exist as a bench case until now. These three VENs
/// (ven-5, ven-14, ven-17) already run 0.06.
///
/// The objective column is the price: a looser gap accepts a worse plan. Reported so
/// the trade is visible rather than assumed, and the gap only changes the optimality
/// tolerance — not asset behaviour.
///
///   bash scripts/run_planner_experiment.sh bench_heater_battery_gap_sweep
#[test]
#[ignore = "R-97: 5 heater+battery phase-1 solves, run with --ignored --nocapture"]
fn bench_heater_battery_gap_sweep() {
    use crate::entities::asset_params::BatteryParams;
    println!("\n── R-97: heater+battery phase 1 vs mip_gap_target ──");
    println!("   (ven-5 shape: 200 L/15 K tank + 11 kWh battery, 288 slots, 48 h)\n");
    println!(
        "  {:>9} {:>11} {:>15} {:>13}  status",
        "mip_gap", "phase1 s", "objective EUR", "vs 0.06"
    );
    let mut baseline: Option<f64> = None;
    for gap in [0.06, 0.10, 0.15, 0.20, 0.30] {
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        profile.planner.mip_gap_target = gap;
        let thermal_mass = 200.0 * 4.186 / 3600.0;
        for a in profile.assets.iter_mut() {
            if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                h.thermal_mass_kwh_per_c = thermal_mass;
                h.temp_min_c = 45.0;
                h.temp_max_c = 60.0;
                h.temp_safety_max_c = 60.0;
                h.temp_initial_c = 47.82;
            }
        }
        profile
            .assets
            .push(crate::entities::asset_params::AssetParams::Battery(
                BatteryParams {
                    id: "battery".into(),
                    capacity_kwh: 11.0,
                    max_charge_kw: 5.5,
                    max_discharge_kw: 5.5,
                    initial_soc: 0.50,
                    round_trip_efficiency: 0.92,
                    min_soc: 0.10,
                    c_terminal_eur_kwh: None,
                },
            ));
        let mut sim = make_snap_from_profile(&profile);
        set_heater_temp(&mut sim, 47.82);
        set_heater_power(&mut sim, 6.0);
        let tariffs = make_diurnal_tariffs(50);
        let cap = no_capacity();
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let t = Instant::now();
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let secs = t.elapsed().as_secs_f64();
        if baseline.is_none() {
            baseline = Some(p1.objective_eur);
        }
        // Objective is a cost being minimised, so a higher value is a worse plan.
        let delta = baseline
            .map(|b| {
                if b.abs() > 1e-9 {
                    format!("{:+.2}%", 100.0 * (p1.objective_eur - b) / b.abs())
                } else {
                    "n/a".to_string()
                }
            })
            .unwrap_or_default();
        println!(
            "  {gap:>9.2} {secs:>11.2} {:>15.4} {delta:>13}  {:?}",
            p1.objective_eur, p1.status
        );
        emit_result(&format!(
            r#"{{"params":{{"class":"heater + battery","mip_gap":{gap},"slots":{},"volume_l":200,"band_k":15}},"results":{{"phase1_s":{secs:.3},"objective_eur":{:.4},"status":"{:?}"}}}}"#,
            inputs.n, p1.objective_eur, p1.status
        ));
    }
    println!(
        "\n  A gap that takes phase 1 off TimeLimit without a large objective penalty is\n  \
         a real saving for ven-5/ven-14/ven-17, whose phase 1 is 36-60 s in production.\n"
    );
}

/// R-97: does a looser gap cost anything in the EXECUTED window?
///
/// `bench_heater_battery_gap_sweep` shows gap 0.30 taking heater+battery phase 1 from
/// 58 s (TimeLimit) to 12.5 s (GapLimit) — a 4.6x saving for ven-5/ven-14/ven-17 —
/// at a 6.37 % worse horizon-wide objective.
///
/// That objective covers 48 h, but a plan is replaced every 300 s, so only its first
/// slots are ever executed. Every comparable question in this investigation has come
/// out the same way: the horizon-wide number overstated the executed impact (the 48 h
/// horizon looked harmful and was neutral; phase 2's smoothing looked valuable and
/// never reached the relay). So the horizon objective is the wrong yardstick here too.
///
/// This prices the first 25 min / 1 h / 8 h of each plan under identical tariffs.
///
///   bash scripts/run_planner_experiment.sh bench_gap_executed_cost
#[test]
#[ignore = "R-97: 2 heater+battery phase-1 solves, run with --ignored --nocapture"]
fn bench_gap_executed_cost() {
    use crate::entities::asset_params::BatteryParams;
    println!("\n── R-97: executed-window cost of a looser gap (heater+battery) ──");
    println!("   (replan every 300 s, so the first slots are what actually run)\n");
    println!(
        "  {:>16} {:>7} {:>9} {:>11} {:>11} {:>11} {:>13}  status",
        "instance", "mip_gap", "phase1 s", "cost 25min", "cost 1h", "cost 8h", "objective"
    );
    let mut rows = Vec::new();
    // Several start conditions, because a gap that works on one instance need not
    // work on another — the epsilon sweeps in this file show exactly that failure.
    for (inst, temp_c, initial_kw) in [
        ("cool, full", 47.82, 6.0),
        ("near T_min, off", 46.0, 0.0),
        ("mid-band, mid", 52.0, 3.0),
        ("warm, off", 57.0, 0.0),
    ] {
        for gap in [0.06, 0.30] {
            let now = fixed_now();
            let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
            profile.planner.mip_gap_target = gap;
            let thermal_mass = 200.0 * 4.186 / 3600.0;
            for a in profile.assets.iter_mut() {
                if let crate::entities::asset_params::AssetParams::Heater(h) = a {
                    h.thermal_mass_kwh_per_c = thermal_mass;
                    h.temp_min_c = 45.0;
                    h.temp_max_c = 60.0;
                    h.temp_safety_max_c = 60.0;
                    h.temp_initial_c = temp_c;
                }
            }
            profile
                .assets
                .push(crate::entities::asset_params::AssetParams::Battery(
                    BatteryParams {
                        id: "battery".into(),
                        capacity_kwh: 11.0,
                        max_charge_kw: 5.5,
                        max_discharge_kw: 5.5,
                        initial_soc: 0.50,
                        round_trip_efficiency: 0.92,
                        min_soc: 0.10,
                        c_terminal_eur_kwh: None,
                    },
                ));
            let mut sim = make_snap_from_profile(&profile);
            set_heater_temp(&mut sim, temp_c);
            set_heater_power(&mut sim, initial_kw);
            let tariffs = make_diurnal_tariffs(50);
            let cap = no_capacity();
            let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
            let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
            let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
            let t = Instant::now();
            let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
            let secs = t.elapsed().as_secs_f64();

            // Net grid cost over the first `hours`, priced identically in both rows.
            let cost_within = |hours: f64| -> f64 {
                let mut c = 0.0;
                let mut elapsed = 0.0;
                for i in 0..inputs.n {
                    if elapsed >= hours {
                        break;
                    }
                    c += (inputs.c_imp_eur_kwh[i] * p1.p_imp_kw[i]
                        - inputs.c_exp_eur_kwh[i] * p1.p_exp_kw[i])
                        * inputs.dt_h[i];
                    elapsed += inputs.dt_h[i];
                }
                c
            };
            let (c25, c1, c8) = (cost_within(25.0 / 60.0), cost_within(1.0), cost_within(8.0));
            println!(
            "  {inst:>16} {gap:>7.2} {secs:>9.2} {c25:>11.4} {c1:>11.4} {c8:>11.4} {:>13.4}  {:?}",
            p1.objective_eur, p1.status
        );
            emit_result(&format!(
                r#"{{"params":{{"class":"heater + battery","instance":"{inst}","temp_c":{temp_c},"initial_kw":{initial_kw},"mip_gap":{gap},"slots":{},"volume_l":200,"band_k":15}},"results":{{"phase1_s":{secs:.3},"cost_25min_eur":{c25:.4},"cost_1h_eur":{c1:.4},"cost_8h_eur":{c8:.4},"objective_eur":{:.4},"status":"{:?}"}}}}"#,
                inputs.n, p1.objective_eur, p1.status
            ));
            rows.push((gap, c25, c1, c8));
        }
    }
    if rows.len() == 2 {
        let (_, a25, a1, a8) = rows[0];
        let (_, b25, b1, b8) = rows[1];
        let pct = |a: f64, b: f64| {
            if a.abs() > 1e-9 {
                format!("{:+.2}%", 100.0 * (b - a) / a.abs())
            } else {
                "n/a".to_string()
            }
        };
        println!(
            "\n  0.30 vs 0.06 in the executed window: 25min {}, 1h {}, 8h {}",
            pct(a25, b25),
            pct(a1, b1),
            pct(a8, b8)
        );
    }
    println!(
        "\n  If the 25min and 1h costs are unchanged, the 6.37 % horizon penalty is paid\n  \
         entirely in slots that never run, and gap 0.30 is a 4.6x saving for free.\n"
    );
}

/// R-97: the two questions the fleet phase survey left open, in one run.
///
/// The survey (docs/reference/R97_PLANNER_BENCHMARKS.md) found phase 2 hitting its
/// 15 s timeout on **every** battery+EV VEN and converging on every other class —
/// 4 VENs x 15 s per cycle, the fleet's dominant solver cost. Two things are unknown:
///
/// 1. **Does that 15 s reach the relay?** The executed-window check was run on
///    heater instances only (2 of 6 changed in the first 25 min). Battery+EV is a
///    different model and must be measured separately before anything is concluded
///    about trimming the budget.
/// 2. **Why is ven-19 a 10x outlier inside its own class** (12.6 s vs 0.78-2.4 s)?
///    Its only unique parameters are the fleet's largest battery (16 kWh / 7 kW) and
///    the only `round_trip_efficiency` of 0.93 rather than 0.92. Round-trip loss sets
///    the price spread at which arbitrage breaks even; an efficiency landing
///    break-even near the actual tariff spread leaves many near-optimal schedules and
///    a weak LP bound. Sweeping only the efficiency, with everything else fixed,
///    separates that from the size.
///
///   wsl cargo test -p ven-app --release bench_battery_ev_phase2_executed_window -- --ignored --nocapture
#[test]
#[ignore = "R-97: 2 solves per efficiency, run with --ignored --nocapture"]
fn bench_battery_ev_phase2_executed_window() {
    use crate::entities::asset_params::BatteryParams;

    let now = fixed_now();
    let tariffs = make_diurnal_tariffs(50);
    let cap = no_capacity();

    // Energy moved inside the first `hours` of a dispatch [kWh] — what the relay
    // actually sees before the next replan replaces the plan.
    let energy_within = |p: &[f64], dt_h: &[f64], n: usize, hours: f64| -> f64 {
        let mut kwh = 0.0;
        let mut elapsed = 0.0;
        for t in 0..n {
            if elapsed >= hours {
                break;
            }
            kwh += p[t] * dt_h[t];
            elapsed += dt_h[t];
        }
        kwh
    };

    println!("\n── R-97: battery+EV, does phase 2's 15 s reach the relay? ──");
    println!("   (ven-19's battery 16 kWh / 7 kW; EV 11 kW; no heater; 288 slots, 48 h)\n");
    println!(
        "  {:>5} {:>8} {:>8} {:>11} {:>11} {:>10} {:>10} {:>10}",
        "eff", "p1 s", "p2 s", "p2 status", "friction", "ev 25m", "bat 25m", "bat 48h"
    );

    for eff in [0.92_f64, 0.93, 0.96] {
        let mut profile = ev_bench_profile(true, 20.0);
        profile.planner.mip_gap_target = 0.06;
        profile.planner.phase2_epsilon_eur = 0.02; // the fleet default
        profile
            .assets
            .push(crate::entities::asset_params::AssetParams::Battery(
                BatteryParams {
                    id: "battery".into(),
                    capacity_kwh: 16.0,
                    max_charge_kw: 7.0,
                    max_discharge_kw: 7.0,
                    initial_soc: 0.50,
                    round_trip_efficiency: eff,
                    min_soc: 0.10,
                    c_terminal_eur_kwh: None,
                },
            ));

        let sim = make_snap_from_profile(&profile);
        let ctxs = build_asset_contexts(&profile, &sim, now, None, None, &tariffs);
        let inputs = build_milp_inputs(&ctxs, &tariffs, &cap, &profile, now, &[], None);
        let p1w = build_phase1_weights(&profile, PlannerObjective::MinCost);
        let p2w = build_phase2_weights(&inputs, &profile.planner);

        let t0 = std::time::Instant::now();
        let p1 = solve_phase1(&inputs, &p1w, &ctxs, 60.0).expect("phase 1 feasible");
        let p1_s = t0.elapsed().as_secs_f64();

        let t1 = std::time::Instant::now();
        let (p2, friction) = solve_phase2(
            &inputs,
            &p1w,
            &p2w,
            p1.objective_eur,
            profile.planner.phase2_epsilon_eur,
            &p1,
            &ctxs,
            15.0,
        )
        .expect("phase 2 feasible");
        let p2_s = t1.elapsed().as_secs_f64();

        let bat_net = |s: &crate::controller::milp_planner::types::MilpSolution, h: f64| -> f64 {
            energy_within(&s.p_bat_ch_kw, &inputs.dt_h, inputs.n, h)
                - energy_within(&s.p_bat_dis_kw, &inputs.dt_h, inputs.n, h)
        };

        for (tag, s, p2s, fr) in [
            ("p1", &p1, f64::NAN, f64::NAN),
            ("p2", &p2, p2_s, friction),
        ] {
            println!(
                "  {:>5} {:>8.2} {:>8.2} {:>11} {:>11.4} {:>10.3} {:>10.3} {:>10.3}",
                format!("{eff:.2}{tag}"),
                if tag == "p1" { p1_s } else { f64::NAN },
                p2s,
                format!("{:?}", s.status),
                fr,
                energy_within(&s.p_ev_kw, &inputs.dt_h, inputs.n, 25.0 / 60.0),
                bat_net(s, 25.0 / 60.0),
                bat_net(s, 48.0),
            );
        }

        emit_result(&format!(
            "{{\"bench\":\"bench_battery_ev_phase2_executed_window\",\"class\":1,\
             \"params\":{{\"class\":\"battery + EV\",\"round_trip_efficiency\":{eff},\
             \"capacity_kwh\":16.0,\"max_charge_kw\":7.0,\"mip_gap\":0.06,\
             \"phase2_epsilon_eur\":0.02,\"slots\":288}},\
             \"results\":{{\"phase1_s\":{:.3},\"phase2_s\":{:.3},\"phase1_status\":\"{:?}\",\
             \"phase2_status\":\"{:?}\",\"friction_eur\":{:.5},\
             \"p1_ev_25min_kwh\":{:.4},\"p2_ev_25min_kwh\":{:.4},\
             \"p1_bat_net_25min_kwh\":{:.4},\"p2_bat_net_25min_kwh\":{:.4},\
             \"p1_bat_net_48h_kwh\":{:.4},\"p2_bat_net_48h_kwh\":{:.4}}}}}",
            p1_s,
            p2_s,
            p1.status,
            p2.status,
            friction,
            energy_within(&p1.p_ev_kw, &inputs.dt_h, inputs.n, 25.0 / 60.0),
            energy_within(&p2.p_ev_kw, &inputs.dt_h, inputs.n, 25.0 / 60.0),
            bat_net(&p1, 25.0 / 60.0),
            bat_net(&p2, 25.0 / 60.0),
            bat_net(&p1, 48.0),
            bat_net(&p2, 48.0),
        ));
    }

    println!(
        "\n  If p1 and p2 agree on `ev 25m` and `bat 25m`, the 15 s buys nothing the\n\
         \x20          relay feels on this class. If phase 1 time climbs with efficiency, ven-19's\n\
         \x20          outlier is the efficiency, not the battery size.\n"
    );
}
