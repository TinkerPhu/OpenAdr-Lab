//! R-93: the EV's state of charge is part of the solved plan.
//!
//! These assert the properties the deleted post-solve integrator
//! (`asset_port::ev_soc_trajectory`) used to be unit-tested for — monotonic rise
//! under charging, the pack ceiling, a predicted trip's drop landing at its own
//! slot, and the floor absorbing an oversized drop. They are now properties of
//! the plan the solver produces, so each one runs a real solve and reads
//! `SolveOutput::soc_ev` rather than calling a helper that no longer exists.

use super::solver::{make_phase1_weights, make_solver_inputs};
use super::*;
use crate::controller::milp_planner::asset_port::ExogenousSocDrops;

/// An EV that is present throughout, cheap to charge, and obliged to reach a
/// target — the shape every test here varies.
fn ev_inputs(n: usize, battery_kwh: f64, soc_init: f64) -> MilpInputs {
    let mut inputs = make_solver_inputs(n, 0.0);
    inputs.a_ev = vec![true; n];
    inputs.ev_mode = MilpLoadMode::MustRun;
    inputs.ev_battery_kwh = battery_kwh;
    inputs.soc_ev_init = Some(soc_init);
    inputs.p_ev_max_kw = 7.2;
    inputs.p_ev_min_kw = 0.0;
    inputs
}

fn solve(inputs: &MilpInputs) -> SolveOutput {
    solve_phase1(
        inputs,
        &make_phase1_weights(),
        &contexts_from_inputs(inputs),
        60.0,
    )
    .expect("solve must succeed")
}

#[test]
fn soc_rises_by_the_energy_charged() {
    // 10 kWh pack, a firm 5 kWh by the last slot: the curve must end half a pack
    // above where it started, and never step down while charging.
    let n = 5;
    let mut inputs = ev_inputs(n, 10.0, 0.0);
    inputs.ev_obligations = ev_firm_kwh(10.0, 0.0, 5.0, n - 1);

    let sol = solve(&inputs);

    assert_eq!(sol.soc_ev.len(), n + 1, "one SoC value per step boundary");
    assert!(
        (sol.soc_ev[0] - 0.0).abs() < 1e-6,
        "must start at the live reading, got {}",
        sol.soc_ev[0]
    );
    for t in 1..=n {
        assert!(
            sol.soc_ev[t] >= sol.soc_ev[t - 1] - 1e-6,
            "SoC must not fall while only charging: {:?}",
            sol.soc_ev
        );
    }
    // The obligation sits on slot n-1, so it binds the boundary after it.
    assert!(
        (sol.soc_ev[n] - 0.5).abs() < 1e-3,
        "5 kWh into a 10 kWh pack is half a pack by the deadline, got {}",
        sol.soc_ev[n]
    );
}

#[test]
fn soc_never_exceeds_a_full_pack() {
    // Far more charging opportunity than the pack can hold, and a per-kWh reward
    // that would happily buy more: the ceiling is what stops it.
    let n = 10;
    let mut inputs = ev_inputs(n, 5.0, 0.5);
    inputs.e_ev_extra_max_kwh = 100.0;
    inputs.v_ev_extra_eur_kwh = 10.0;

    let sol = solve(&inputs);

    for (t, &soc) in sol.soc_ev.iter().enumerate() {
        assert!(
            soc <= 1.0 + 1e-6,
            "SoC must never exceed a full pack, got {soc} at step {t}"
        );
    }
}

#[test]
fn a_predicted_drop_lands_at_its_own_slot() {
    // No obligation and no reward, so the only movement in the curve is the trip.
    let n = 5;
    let mut inputs = ev_inputs(n, 10.0, 0.80);
    let mut drop_frac_per_slot = vec![0.0; n];
    drop_frac_per_slot[2] = 0.30;
    inputs.ev_soc_drops = Some(ExogenousSocDrops {
        drop_frac_per_slot,
        floor_frac: 0.05,
    });

    let sol = solve(&inputs);

    assert!(
        (sol.soc_ev[2] - 0.80).abs() < 1e-3,
        "unchanged before the drop slot, got {}",
        sol.soc_ev[2]
    );
    assert!(
        (sol.soc_ev[3] - 0.50).abs() < 1e-3,
        "slot 2's drop must show at the following boundary, got {}",
        sol.soc_ev[3]
    );
    assert!(
        (sol.soc_ev[4] - 0.50).abs() < 1e-3,
        "and then hold, got {}",
        sol.soc_ev[4]
    );
}

#[test]
fn an_oversized_drop_settles_at_the_floor_rather_than_going_infeasible() {
    // A trip that would consume more than the pack holds. The old integrator
    // clamped this after the fact; the model now absorbs it with the penalised
    // `drop_unmet` slack, so the solve must still succeed and settle at the floor.
    let n = 3;
    let mut inputs = ev_inputs(n, 10.0, 0.20);
    inputs.ev_soc_drops = Some(ExogenousSocDrops {
        drop_frac_per_slot: vec![0.0, 0.90, 0.0],
        floor_frac: 0.05,
    });

    let sol = solve(&inputs);

    assert!(
        (sol.soc_ev[2] - 0.05).abs() < 1e-3,
        "must settle at the configured floor, got {}",
        sol.soc_ev[2]
    );
}

#[test]
fn a_drop_the_pack_can_absorb_is_applied_in_full() {
    // The guard on the slack above: where the floor is not binding, the whole
    // drop must land. Otherwise `drop_unmet` would be a way to dodge a trip.
    let n = 3;
    let mut inputs = ev_inputs(n, 10.0, 0.90);
    inputs.ev_soc_drops = Some(ExogenousSocDrops {
        drop_frac_per_slot: vec![0.0, 0.40, 0.0],
        floor_frac: 0.05,
    });

    let sol = solve(&inputs);

    assert!(
        (sol.soc_ev[2] - 0.50).abs() < 1e-3,
        "the full 40 % must be consumed, got {}",
        sol.soc_ev[2]
    );
}

#[test]
fn without_declared_drops_the_curve_only_reflects_charging() {
    let n = 4;
    let mut inputs = ev_inputs(n, 10.0, 0.30);
    inputs.ev_soc_drops = None;
    inputs.ev_obligations = ev_firm_kwh(10.0, 0.30, 2.0, n - 1);

    let sol = solve(&inputs);

    assert!(
        (sol.soc_ev[n] - 0.50).abs() < 1e-3,
        "0.30 plus 2 kWh of a 10 kWh pack, got {}",
        sol.soc_ev[n]
    );
}

// ── Obligations bind at their own deadline (R-92) ────────────────────────────

#[test]
fn a_firm_obligation_is_met_at_its_deadline_step() {
    let n = 6;
    let mut inputs = ev_inputs(n, 20.0, 0.0);
    inputs.ev_obligations = ev_firm_kwh(20.0, 0.0, 10.0, 3);

    let sol = solve(&inputs);

    assert!(
        sol.soc_ev[4] >= 0.5 - 1e-3,
        "must hold half the pack by the end of slot 3, got {}",
        sol.soc_ev[4]
    );
    assert!(
        sol.ev_shortfall_kwh.iter().all(|&kwh| kwh < 1e-3),
        "a reachable target must never be traded for shortfall slack: {:?}",
        sol.ev_shortfall_kwh
    );
}

#[test]
fn two_obligations_are_each_met_at_their_own_step() {
    // The capability R-92 was opened for: two departures inside one horizon, each
    // with its own target, both binding.
    let n = 8;
    let mut inputs = ev_inputs(n, 20.0, 0.0);
    inputs.ev_obligations = vec![
        crate::controller::milp_planner::asset_port::EvObligation {
            deadline_step: 2,
            target_soc: 0.25,
            session_id: None,
        },
        crate::controller::milp_planner::asset_port::EvObligation {
            deadline_step: 6,
            target_soc: 0.75,
            session_id: None,
        },
    ];

    let sol = solve(&inputs);

    assert!(
        sol.soc_ev[3] >= 0.25 - 1e-3,
        "first target must bind by the end of slot 2, got {}",
        sol.soc_ev[3]
    );
    assert!(
        sol.soc_ev[7] >= 0.75 - 1e-3,
        "second target must bind by the end of slot 6, got {}",
        sol.soc_ev[7]
    );
}

#[test]
fn an_unreachable_obligation_reports_the_gap_instead_of_failing_the_solve() {
    // 7.2 kW for two slots cannot fill a 100 kWh pack. The old model pre-clamped
    // the requirement so the gap was invisible; the slack now carries it.
    let n = 2;
    let mut inputs = ev_inputs(n, 100.0, 0.0);
    inputs.ev_obligations = ev_firm_kwh(100.0, 0.0, 90.0, 1);

    let sol = solve(&inputs);

    let short: f64 = sol.ev_shortfall_kwh.iter().copied().fold(0.0, f64::max);
    assert!(
        short > 1.0,
        "the unreachable part must be reported as shortfall, got {:?}",
        sol.ev_shortfall_kwh
    );
    // What the window *can* deliver must still be charged.
    assert!(
        sol.p_ev_kw.iter().sum::<f64>() > 0.0,
        "the plan must still charge everything the window allows"
    );
}

#[test]
fn charging_while_away_is_never_scheduled() {
    let n = 6;
    let mut inputs = ev_inputs(n, 20.0, 0.0);
    inputs.a_ev = vec![true, true, false, false, true, true];
    inputs.ev_obligations = ev_firm_kwh(20.0, 0.0, 5.0, n - 1);

    let sol = solve(&inputs);

    for t in [2usize, 3] {
        assert!(
            sol.p_ev_kw[t].abs() < 1e-6,
            "no charging while the car is away, got {} at slot {t}",
            sol.p_ev_kw[t]
        );
    }
}

/// The fleet regression R-93 caused: comfort bands are priced from the current state
/// of charge all the way to a FULL pack, so once the band-accounting sum became
/// whole-horizon the solver bought that beyond-target energy everywhere - charging a
/// VEN to 100 % against an 80 % target and refilling the predicted trip's drop in the
/// very slot it happened, leaving the planned curve with no dip at all.
///
/// Observed on VEN-1 before the fix: 0.673 -> 1.000, 41 charging slots, zero
/// decreases across 288 slots despite a predicted 22 % drop.
///
/// The discriminator is deliberately the *window*, not the total: beyond-target
/// buying before the deadline is pre-R-93 behaviour and still allowed. Charge power
/// is therefore capped so that the pre-deadline window can absorb only a fraction of
/// what the bands offer - without the bound the remaining 10 kWh is bought across the
/// rest of the horizon, with it the purchase stops at the deadline.
#[test]
fn beyond_target_bands_are_not_bought_after_the_last_deadline() {
    let n = 12;
    let mut inputs = ev_inputs(n, 20.0, 0.50);
    inputs.p_ev_max_kw = 2.0; // 1 h slots -> 2 kWh per slot
                              // Firm obligation by slot 1: 2 kWh. The window 0..=1 can absorb at most 4 kWh.
    inputs.ev_obligations = ev_firm_kwh(20.0, 0.50, 2.0, 1);
    // Bands that would sell 10 kWh - half the pack - cheaply, exactly as
    // `ev_energy_segments` builds them all the way to a full pack.
    inputs.ev_segments = vec![
        crate::controller::milp_planner::asset_port::EvEnergySegment {
            kwh: 10.0,
            eur_per_kwh: 5.0,
        },
    ];

    let sol = solve(&inputs);
    let total_kwh: f64 = sol.p_ev_kw.iter().sum(); // 1 h slots

    assert!(
        sol.p_ev_kw.iter().skip(2).sum::<f64>() < 1e-6,
        "no charging may be bought after the last deadline, got {:?}",
        sol.p_ev_kw
    );
    assert!(
        total_kwh <= 4.0 + 1e-6,
        "bands may only be bought inside the deadline window (<=4 kWh), got {total_kwh}"
    );
}

/// Design Decision 9's worked example, which is the whole reason a session states a
/// trip distance: two stated sessions with a trip between them.
///
/// Without the stated drop the planner believes the car returns exactly as it left,
/// so it charges for the first deadline and schedules NOTHING for the second - the
/// car sits well under target on the second morning, with the cheap overnight slots
/// unused.
///
/// The assertion is on energy, not on the shape of the curve: given the chance, the
/// solver legitimately pre-charges before the trip rather than after it, so the
/// state of charge need never visibly dip. What distinguishes "the trip was
/// accounted for" from "the trip was ignored" is that ~30 % of the pack gets bought
/// at all, and that the second deadline is still met afterwards.
#[test]
fn a_stated_trip_between_two_sessions_is_charged_back_for() {
    let n = 12;
    let mut inputs = ev_inputs(n, 20.0, 0.80);
    inputs.ev_obligations = vec![
        crate::controller::milp_planner::asset_port::EvObligation {
            deadline_step: 1,
            target_soc: 0.80,
            session_id: None,
        },
        crate::controller::milp_planner::asset_port::EvObligation {
            deadline_step: 10,
            target_soc: 0.80,
            session_id: None,
        },
    ];
    // The trip between them costs 30 % of a 20 kWh pack = 6 kWh.
    let mut drop_frac_per_slot = vec![0.0; n];
    drop_frac_per_slot[4] = 0.30;
    inputs.ev_soc_drops = Some(ExogenousSocDrops {
        drop_frac_per_slot,
        floor_frac: 0.05,
    });

    let sol = solve(&inputs);
    let charged_kwh: f64 = sol.p_ev_kw.iter().sum(); // 1 h slots

    assert!(
        charged_kwh > 5.0,
        "the trip's 6 kWh must be charged back for, got {charged_kwh:.2} kWh from {:?}",
        sol.p_ev_kw
    );
    assert!(
        sol.soc_ev[11] >= 0.80 - 1e-3,
        "the second session's target must still be met, got {}",
        sol.soc_ev[11]
    );
    assert!(
        sol.ev_shortfall_kwh.iter().all(|kwh| *kwh < 1e-3),
        "nothing should be given up as shortfall here, got {:?}",
        sol.ev_shortfall_kwh
    );
}
