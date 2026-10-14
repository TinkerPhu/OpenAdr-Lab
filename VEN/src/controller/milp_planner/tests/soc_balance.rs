//! R-93: the EV's state of charge is part of the solved plan.
//!
//! These assert the properties the deleted post-solve integrator
//! (`asset_port::ev_soc_trajectory`) used to be unit-tested for — monotonic rise
//! under charging, the pack ceiling, a predicted trip's drop landing at its own
//! slot, and the floor absorbing an oversized drop. They are now properties of
//! the plan the solver produces, so each one runs a real solve and reads
//! `SolveOutput::soc_ev` rather than calling a helper that no longer exists.

use super::*;
use crate::controller::milp_planner::asset_port::{EvObligation, ExogenousSocDrops};

/// An EV that is present throughout, cheap to charge, and obliged to reach a
/// target — the shape every test here varies.
fn ev_inputs(n: usize, battery_kwh: f64, soc_init_frac: f64) -> SyntheticSite {
    let mut site = make_solver_site(n, 0.0);
    site.ev.a_ev = vec![true; n];
    site.ev.mode = EvMilpMode::MustRun;
    site.ev.battery_kwh = battery_kwh;
    site.ev.soc_init_frac = soc_init_frac;
    site.ev.p_max_kw = 7.2;
    site.ev.p_min_kw = 0.0;
    site
}

fn solve(site: &SyntheticSite) -> SolveOutput {
    site.solve_phase1(&make_phase1_weights())
        .expect("solve must succeed")
}

#[test]
fn soc_rises_by_the_energy_charged() {
    // 10 kWh pack, a firm 5 kWh by the last slot: the curve must end half a pack
    // above where it started, and never step down while charging.
    let n = 5;
    let mut site = ev_inputs(n, 10.0, 0.0);
    site.ev.obligations = ev_firm_kwh(10.0, 0.0, 5.0, n - 1);

    let sol = solve(&site);

    assert_eq!(sol.ev().soc.len(), n + 1, "one SoC value per step boundary");
    assert!(
        (sol.ev().soc[0] - 0.0).abs() < 1e-6,
        "must start at the live reading, got {}",
        sol.ev().soc[0]
    );
    for t in 1..=n {
        assert!(
            sol.ev().soc[t] >= sol.ev().soc[t - 1] - 1e-6,
            "SoC must not fall while only charging: {:?}",
            sol.ev().soc
        );
    }
    // The obligation sits on slot n-1, so it binds the boundary after it.
    assert!(
        (sol.ev().soc[n] - 0.5).abs() < 1e-3,
        "5 kWh into a 10 kWh pack is half a pack by the deadline, got {}",
        sol.ev().soc[n]
    );
}

#[test]
fn soc_never_exceeds_a_full_pack() {
    // Far more charging opportunity than the pack can hold, and a per-kWh reward
    // that would happily buy more: the ceiling is what stops it.
    let n = 10;
    let mut site = ev_inputs(n, 5.0, 0.5);
    site.ev.e_extra_max_kwh = 100.0;
    site.ev.v_extra_eur_kwh = 10.0;

    let sol = solve(&site);

    for (t, &soc_frac) in sol.ev().soc.iter().enumerate() {
        assert!(
            soc_frac <= 1.0 + 1e-6,
            "SoC must never exceed a full pack, got {soc_frac} at step {t}"
        );
    }
}

#[test]
fn a_predicted_drop_lands_at_its_own_slot() {
    // No obligation and no reward, so the only movement in the curve is the trip.
    let n = 5;
    let mut site = ev_inputs(n, 10.0, 0.80);
    let mut drop_frac_per_slot = vec![0.0; n];
    drop_frac_per_slot[2] = 0.30;
    site.ev.soc_drops = Some(ExogenousSocDrops {
        drop_frac_per_slot,
        floor_frac: 0.05,
    });

    let sol = solve(&site);

    assert!(
        (sol.ev().soc[2] - 0.80).abs() < 1e-3,
        "unchanged before the drop slot, got {}",
        sol.ev().soc[2]
    );
    assert!(
        (sol.ev().soc[3] - 0.50).abs() < 1e-3,
        "slot 2's drop must show at the following boundary, got {}",
        sol.ev().soc[3]
    );
    assert!(
        (sol.ev().soc[4] - 0.50).abs() < 1e-3,
        "and then hold, got {}",
        sol.ev().soc[4]
    );
}

#[test]
fn an_oversized_drop_settles_at_the_floor_rather_than_going_infeasible() {
    // A trip that would consume more than the pack holds. The old integrator
    // clamped this after the fact; the model now absorbs it with the penalised
    // `drop_unmet` slack, so the solve must still succeed and settle at the floor.
    let n = 3;
    let mut site = ev_inputs(n, 10.0, 0.20);
    site.ev.soc_drops = Some(ExogenousSocDrops {
        drop_frac_per_slot: vec![0.0, 0.90, 0.0],
        floor_frac: 0.05,
    });

    let sol = solve(&site);

    assert!(
        (sol.ev().soc[2] - 0.05).abs() < 1e-3,
        "must settle at the configured floor, got {}",
        sol.ev().soc[2]
    );
}

#[test]
fn a_drop_the_pack_can_absorb_is_applied_in_full() {
    // The guard on the slack above: where the floor is not binding, the whole
    // drop must land. Otherwise `drop_unmet` would be a way to dodge a trip.
    let n = 3;
    let mut site = ev_inputs(n, 10.0, 0.90);
    site.ev.soc_drops = Some(ExogenousSocDrops {
        drop_frac_per_slot: vec![0.0, 0.40, 0.0],
        floor_frac: 0.05,
    });

    let sol = solve(&site);

    assert!(
        (sol.ev().soc[2] - 0.50).abs() < 1e-3,
        "the full 40 % must be consumed, got {}",
        sol.ev().soc[2]
    );
}

#[test]
fn without_declared_drops_the_curve_only_reflects_charging() {
    let n = 4;
    let mut site = ev_inputs(n, 10.0, 0.30);
    site.ev.soc_drops = None;
    site.ev.obligations = ev_firm_kwh(10.0, 0.30, 2.0, n - 1);

    let sol = solve(&site);

    assert!(
        (sol.ev().soc[n] - 0.50).abs() < 1e-3,
        "0.30 plus 2 kWh of a 10 kWh pack, got {}",
        sol.ev().soc[n]
    );
}

// ── Obligations bind at their own deadline (R-92) ────────────────────────────

#[test]
fn a_firm_obligation_is_met_at_its_deadline_step() {
    let n = 6;
    let mut site = ev_inputs(n, 20.0, 0.0);
    site.ev.obligations = ev_firm_kwh(20.0, 0.0, 10.0, 3);

    let sol = solve(&site);

    assert!(
        sol.ev().soc[4] >= 0.5 - 1e-3,
        "must hold half the pack by the end of slot 3, got {}",
        sol.ev().soc[4]
    );
    assert!(
        sol.ev().shortfall_kwh.iter().all(|&kwh| kwh < 1e-3),
        "a reachable target must never be traded for shortfall slack: {:?}",
        sol.ev().shortfall_kwh
    );
}

#[test]
fn two_obligations_are_each_met_at_their_own_step() {
    // The capability R-92 was opened for: two departures inside one horizon, each
    // with its own target, both binding.
    let n = 8;
    let mut site = ev_inputs(n, 20.0, 0.0);
    site.ev.obligations = vec![
        crate::controller::milp_planner::asset_port::EvObligation {
            deadline_step: 2,
            target_soc_frac: 0.25,
            session_id: None,
        },
        crate::controller::milp_planner::asset_port::EvObligation {
            deadline_step: 6,
            target_soc_frac: 0.75,
            session_id: None,
        },
    ];

    let sol = solve(&site);

    assert!(
        sol.ev().soc[3] >= 0.25 - 1e-3,
        "first target must bind by the end of slot 2, got {}",
        sol.ev().soc[3]
    );
    assert!(
        sol.ev().soc[7] >= 0.75 - 1e-3,
        "second target must bind by the end of slot 6, got {}",
        sol.ev().soc[7]
    );
}

#[test]
fn an_unreachable_obligation_reports_the_gap_instead_of_failing_the_solve() {
    // 7.2 kW for two slots cannot fill a 100 kWh pack. The old model pre-clamped
    // the requirement so the gap was invisible; the slack now carries it.
    let n = 2;
    let mut site = ev_inputs(n, 100.0, 0.0);
    site.ev.obligations = ev_firm_kwh(100.0, 0.0, 90.0, 1);

    let sol = solve(&site);

    let short: f64 = sol.ev().shortfall_kwh.iter().copied().fold(0.0, f64::max);
    assert!(
        short > 1.0,
        "the unreachable part must be reported as shortfall, got {:?}",
        sol.ev().shortfall_kwh
    );
    // What the window *can* deliver must still be charged.
    assert!(
        sol.ev().power_kw.iter().sum::<f64>() > 0.0,
        "the plan must still charge everything the window allows"
    );
}

#[test]
fn charging_while_away_is_never_scheduled() {
    let n = 6;
    let mut site = ev_inputs(n, 20.0, 0.0);
    site.ev.a_ev = vec![true, true, false, false, true, true];
    site.ev.obligations = ev_firm_kwh(20.0, 0.0, 5.0, n - 1);

    let sol = solve(&site);

    for t in [2usize, 3] {
        assert!(
            sol.ev().power_kw[t].abs() < 1e-6,
            "no charging while the car is away, got {} at slot {t}",
            sol.ev().power_kw[t]
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
    let mut site = ev_inputs(n, 20.0, 0.50);
    site.ev.p_max_kw = 2.0; // 1 h slots -> 2 kWh per slot
                            // Firm obligation by slot 1: 2 kWh. The window 0..=1 can absorb at most 4 kWh.
    site.ev.obligations = ev_firm_kwh(20.0, 0.50, 2.0, 1);
    // Bands that would sell 10 kWh - half the pack - cheaply, exactly as
    // `ev_energy_segments` builds them all the way to a full pack.
    site.ev.segments = vec![
        crate::controller::milp_planner::asset_port::EvEnergySegment {
            kwh: 10.0,
            eur_per_kwh: 5.0,
        },
    ];

    let sol = solve(&site);
    let total_kwh: f64 = sol.ev().power_kw.iter().sum(); // 1 h slots

    assert!(
        sol.ev().power_kw.iter().skip(2).sum::<f64>() < 1e-6,
        "no charging may be bought after the last deadline, got {:?}",
        sol.ev().power_kw
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
    let mut site = ev_inputs(n, 20.0, 0.80);
    site.ev.obligations = vec![
        crate::controller::milp_planner::asset_port::EvObligation {
            deadline_step: 1,
            target_soc_frac: 0.80,
            session_id: None,
        },
        crate::controller::milp_planner::asset_port::EvObligation {
            deadline_step: 10,
            target_soc_frac: 0.80,
            session_id: None,
        },
    ];
    // The trip between them costs 30 % of a 20 kWh pack = 6 kWh.
    let mut drop_frac_per_slot = vec![0.0; n];
    drop_frac_per_slot[4] = 0.30;
    site.ev.soc_drops = Some(ExogenousSocDrops {
        drop_frac_per_slot,
        floor_frac: 0.05,
    });

    let sol = solve(&site);
    let charged_kwh: f64 = sol.ev().power_kw.iter().sum(); // 1 h slots

    assert!(
        charged_kwh > 5.0,
        "the trip's 6 kWh must be charged back for, got {charged_kwh:.2} kWh from {:?}",
        sol.ev().power_kw
    );
    assert!(
        sol.ev().soc[11] >= 0.80 - 1e-3,
        "the second session's target must still be met, got {}",
        sol.ev().soc[11]
    );
    assert!(
        sol.ev().shortfall_kwh.iter().all(|kwh| *kwh < 1e-3),
        "nothing should be given up as shortfall here, got {:?}",
        sol.ev().shortfall_kwh
    );
}

/// R-134: the firm energy is the largest demand above the live SoC among the obligations, and
/// an obligation already met demands nothing.
#[test]
fn ev_firm_kwh_is_the_largest_demand_above_the_live_soc() {
    let mut site = ev_inputs(4, 50.0, 0.40);
    assert_eq!(site.ev.firm_required_kwh(), 0.0, "no obligation");

    site.ev.obligations = [0.30, 0.60, 0.80]
        .iter()
        .enumerate()
        .map(|(deadline_step, &target_soc_frac)| EvObligation {
            deadline_step,
            target_soc_frac,
            session_id: None,
        })
        .collect();
    let per_obligation: Vec<f64> = site
        .ev
        .obligations
        .iter()
        .map(|o| site.ev.required_kwh(o))
        .collect();
    assert_eq!(per_obligation[0], 0.0, "already above 30 %");
    assert!((per_obligation[1] - 10.0).abs() < 1e-9);
    assert!((site.ev.firm_required_kwh() - 20.0).abs() < 1e-9);
}

/// R-140: the energy an obligation needs is measured against what the car can hold (its charge
/// limit) and includes what a predicted trip uses before the deadline. The solve, the shortfall
/// warning and the session flexibility all read this one rule.
#[test]
fn an_obligations_energy_stops_at_the_charge_limit_and_includes_the_trip() {
    let mut site = ev_inputs(4, 60.0, 0.20);
    site.ev.soc_max_frac = 0.90;
    site.ev.obligations = vec![EvObligation {
        deadline_step: 2,
        target_soc_frac: 0.80,
        session_id: None,
    }];
    // A trip uses 5 % in slot 1, before the deadline: (0.80 - 0.20 + 0.05) x 60 = 39 kWh.
    site.ev.soc_drops = Some(ExogenousSocDrops {
        drop_frac_per_slot: vec![0.0, 0.05, 0.0, 0.0],
        floor_frac: 0.0,
    });
    let ob = site.ev.obligations[0].clone();
    assert!((site.ev.required_kwh(&ob) - 39.0).abs() < 1e-9);

    // Above the limit: only what the car can hold, (0.90 - 0.20) x 60 = 42 kWh, not 45.
    site.ev.soc_drops = None;
    site.ev.obligations[0].target_soc_frac = 0.95;
    let ob = site.ev.obligations[0].clone();
    assert!((site.ev.required_kwh(&ob) - 42.0).abs() < 1e-9);
    assert!((site.ev.firm_required_kwh() - 42.0).abs() < 1e-9);
}
