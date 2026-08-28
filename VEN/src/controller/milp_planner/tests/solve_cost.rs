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
