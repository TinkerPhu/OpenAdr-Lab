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
        "  {:>34} {:>7} {:>7} {:>10} {:>14}  status",
        "grid", "slots", "hours", "phase1 s", "objective EUR"
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
        let now = fixed_now();
        let mut profile = ev_bench_profile_with_heater(true, 20.0, true);
        let n: usize = zones.iter().map(|x| x.slots).sum();
        let hours: f64 = zones
            .iter()
            .map(|x| x.slots as f64 * x.step_s as f64 / 3600.0)
            .sum();
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
        println!(
            "  {label:>34} {n:>7} {hours:>7.0} {:>10.2} {:>14.4}  {:?}",
            t.elapsed().as_secs_f64(),
            p1.objective_eur,
            p1.status
        );
    }
    println!("\n  (all four fully priced, so pricing is held constant)\n");
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
    }
    println!(
        "\n  If phase-1 time falls as slack rises, heater difficulty is a property of the\n  \
         installation, not of the formulation — and GB-40's per-VEN variance is explained.\n"
    );
}
