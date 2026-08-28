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

/// One decomposed solve at a given gap: phase 1 and phase 2 run separately so
/// each phase's own termination reason is visible.
///
/// `solve_milp_two_phase` returns the *winning* solution, which is phase 2's
/// whenever phase 2 succeeds — so a `Plan`'s `solve_status` reports the friction
/// phase only. That collapses the two, and the first version of this benchmark
/// presented it as if it characterised the whole solve. It does not: phase 1 is
/// where cost optimality is decided, so it is the phase whose gap behaviour
/// actually matters.
struct PhaseRun {
    p1_s: f64,
    p1_status: good_lp::solvers::SolutionStatus,
    p2_s: f64,
    p2_status: Option<good_lp::solvers::SolutionStatus>,
    objective_eur: f64,
}

fn solve_phases_at_gap(mip_gap_target: f64) -> PhaseRun {
    let now = fixed_now();
    let mut profile = bench_profile(true);
    profile.planner.mip_gap_target = mip_gap_target;
    let mut sim = make_snap_from_profile(&profile);
    set_heater_power(&mut sim, 6.0);
    let tariffs = make_tariffs(0.25, 0.08, 300.0);
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

    let (p2_status, objective_eur) = match &p2 {
        Ok((sol, _friction)) => (Some(sol.status), sol.objective_eur),
        Err(_) => (None, p1.objective_eur),
    };

    PhaseRun {
        p1_s,
        p1_status: p1.status,
        p2_s,
        p2_status,
        objective_eur,
    }
}

/// GB-40: where does the optimality gap actually start to bind, and what does it cost?
///
/// The first sweep (2/5/10%) found no time saved and every solve terminating on
/// the clock. Two follow-ups, both prompted by the right question — could the
/// improvement sit at a value we skipped?
///
/// 1. **Sweep much looser.** If 10% never binds, the achieved gap exceeds 10%,
///    and a *tighter* target is strictly harder on both counts (smaller gap to
///    close, less pruning to close it with). So intermediate values cannot help.
///    The informative direction is the other one: keep loosening until the gap
///    binds, and the crossover brackets the achieved gap — which is otherwise
///    unobservable through `good_lp` (R-65 in `docs/reference/TECHNICAL_DEBTS.md`).
///    That converts a qualitative "the relaxation is weak" into a number.
/// 2. **Repeat each point.** HiGHS should be deterministic for identical input,
///    so identical objectives across repeats would establish that the objective
///    differences between gaps are a real consequence of the setting rather than
///    search noise — a distinction the first sweep could not make.
#[test]
#[ignore = "GB-40 benchmark: production-sized solves, run explicitly with --ignored --nocapture"]
fn bench_mip_gap_sweep() {
    const REPEATS: usize = 3;
    let gaps = [0.02_f64, 0.05, 0.10, 0.20, 0.35, 0.50];

    let _ = solve_phases_at_gap(0.02); // warm-up, discarded

    println!(
        "\n=== GB-40: MIP gap sweep (heater case, solver_timeout_s=60, {REPEATS} repeats) ==="
    );
    println!(
        "  {:>5}  {:>9} {:>10}  {:>9} {:>10}  {:>8}  {:>13}  {:>9}",
        "gap",
        "phase1 s",
        "p1 status",
        "phase2 s",
        "p2 status",
        "total s",
        "objective_eur",
        "repeats"
    );

    let mut baseline: Option<f64> = None;
    for &gap in &gaps {
        let runs: Vec<PhaseRun> = (0..REPEATS).map(|_| solve_phases_at_gap(gap)).collect();

        // Median wall time across repeats; objectives are checked for agreement
        // rather than averaged — if they differ, that is itself the finding.
        let mut p1_times: Vec<f64> = runs.iter().map(|r| r.p1_s).collect();
        let mut p2_times: Vec<f64> = runs.iter().map(|r| r.p2_s).collect();
        p1_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        p2_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p1_med = p1_times[REPEATS / 2];
        let p2_med = p2_times[REPEATS / 2];

        let obj = runs[0].objective_eur;
        let identical = runs.iter().all(|r| {
            (r.objective_eur - obj).abs() < 1e-9
                && format!("{:?}", r.p1_status) == format!("{:?}", runs[0].p1_status)
        });
        let spread = if identical {
            "identical".to_string()
        } else {
            let lo = runs
                .iter()
                .map(|r| r.objective_eur)
                .fold(f64::MAX, f64::min);
            let hi = runs
                .iter()
                .map(|r| r.objective_eur)
                .fold(f64::MIN, f64::max);
            format!("VARIES {lo:.3}-{hi:.3}")
        };

        if baseline.is_none() {
            baseline = Some(obj);
        }
        let delta = baseline
            .filter(|b| b.abs() > 1e-9)
            .map(|b| format!("{:+.2}%", (obj - b) / b.abs() * 100.0))
            .unwrap_or_else(|| "n/a".into());

        println!(
            "  {:>4.0}%  {:>9.2} {:>10}  {:>9.2} {:>10}  {:>8.2}  {:>13.4}  {:>9}   {}",
            gap * 100.0,
            p1_med,
            format!("{:?}", runs[0].p1_status),
            p2_med,
            runs[0]
                .p2_status
                .map(|s| format!("{s:?}"))
                .unwrap_or_else(|| "Err".into()),
            p1_med + p2_med,
            obj,
            spread,
            delta
        );
    }
    println!(
        "\n  A phase reporting GapLimit stopped on the gap; TimeLimit means it ran out of clock.\n\
           The loosest gap that still reports TimeLimit is a lower bound on the achieved gap.\n"
    );

    // Measurement, not a threshold: any assertion on times or objectives would
    // fail on a slower machine for reasons unrelated to the formulation.
    assert_eq!(gaps.len(), 6);
}
