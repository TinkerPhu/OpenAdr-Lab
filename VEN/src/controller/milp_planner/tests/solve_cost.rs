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
        &sim,
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
    let inputs = build_milp_inputs(&ctxs, &sim, &tariffs, &cap, &profile, now, &[], None);
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
