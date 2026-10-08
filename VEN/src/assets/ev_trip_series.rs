// ── A series of expected vehicle uses becomes the planner's EV inputs ─────────
// The MILP consumes exactly three EV quantities: a per-slot availability mask, the
// exogenous state-of-charge drops it must project but cannot decide, and the list of
// charging obligations. Before this module, two producers derived all three
// independently — the stated-session queue and the EV's own usage forecast — and the
// six derivations had drifted in the two ways that were the defects:
//
//   * the forecast produced ONE obligation (the next departure) where the session
//     path produced one per session, so every later trip in a 48 h horizon got a
//     truthful mask and a projected drop but nothing asking the plan to recharge;
//   * the session path timed a trip's consumption from the NEXT session's window
//     start, so a lone session's stated distance did nothing at all.
//
// Both producers now state only what they know — a series of expected uses — and this
// is the single place that turns a series into planner inputs
// (`one-concept-one-function`).

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::controller::milp_planner::asset_port::EvObligation;

/// One expected use of the vehicle: when it may charge, when it must be ready, and
/// what the trip that follows is expected to cost — when that is known at all.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpectedVehicleUse {
    /// Charging may begin here.
    pub window_start: DateTime<Utc>,
    /// The vehicle must hold `target_soc` by here.
    pub departure_at: DateTime<Utc>,
    pub target_soc_frac: f64,
    /// A firm departure is a guarantee and binds an obligation. A soft one states a
    /// preference priced per kWh by the comfort curve and binds none
    /// (`ev-comfort-piecewise-core`), but still contributes its window and its
    /// consumption: availability and what a trip costs are facts, independent of
    /// whether the goal is promised.
    pub firm: bool,
    /// `None` when nobody said what the trip costs or when it ends. The plan then
    /// holds the state of charge flat until a real return is measured, rather than
    /// projecting a drop nobody described.
    pub consumption: Option<ExpectedTripConsumption>,
    /// The stated session behind this, when there is one, so a shortfall can name it.
    pub session_id: Option<Uuid>,
}

/// What a trip is expected to cost, and when the vehicle is back to pay for it.
///
/// A pair, not two independent `Option`s: a distance with no return time is energy
/// with no instant to apply it to, and a return time with no distance is an instant
/// with no energy. Keeping them together makes the half-stated case unrepresentable
/// here, so it has to be rejected once, at the boundary that parses a submission.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpectedTripConsumption {
    pub return_at: DateTime<Utc>,
    /// Fraction of the pack the trip consumes (0..1). The EV performs the conversion
    /// from whatever the user stated; this only carries the result.
    pub soc_drop_frac: f64,
}

/// The three quantities the EV MILP consumes, derived from one series.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EvPlanInputs {
    /// Per-slot: may the charger draw power at all.
    pub available_per_slot: Vec<bool>,
    /// Per-slot state-of-charge fraction the plan must subtract.
    pub drop_frac_per_slot: Vec<f64>,
    /// One per firm departure inside the horizon.
    pub obligations: Vec<EvObligation>,
}

/// The slot a *deadline* falls in: the one that ENDS at or after `secs_from_now`.
///
/// An instant landing exactly on a slot boundary belongs to the slot that ends there,
/// not the one that starts there. A departure at 3600 s on a 300 s grid is slot 11,
/// which runs [3300, 3600) — slot 12 runs [3600, 3900), entirely after the car has
/// gone, so being "ready by" slot 12 is too late.
fn deadline_slot(cum_s: &[i64], n: usize, secs_from_now: i64) -> usize {
    if secs_from_now <= 0 {
        return 0;
    }
    cum_s
        .partition_point(|&s| s < secs_from_now)
        .saturating_sub(1)
        .min(n.saturating_sub(1))
}

/// The slot a *return* lands in: the first one that STARTS at or after
/// `secs_from_now`.
///
/// The opposite boundary rule to `deadline_slot`, and deliberately so. A deadline asks
/// "which slot must I be ready by", a return asks "from which slot is the charge
/// already gone" — and the vehicle is only back once a slot begins at or after it.
/// Sharing one helper between the two put every boundary-aligned drop exactly one slot
/// early, which is what the drop-placement tests caught. This is also the convention
/// the deleted `ev_schedule::soc_drop_frac_per_slot` used, by asking each slot whether
/// a trip had ended at or before its start.
fn return_slot(cum_s: &[i64], n: usize, secs_from_now: i64) -> usize {
    if secs_from_now <= 0 {
        return 0;
    }
    cum_s
        .partition_point(|&s| s < secs_from_now)
        .min(n.saturating_sub(1))
}

/// Start and end instants of slot `t`.
fn slot_bounds(cum_s: &[i64], t: usize, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = cum_s.get(t).copied().unwrap_or(0);
    let end = cum_s
        .get(t + 1)
        .copied()
        .unwrap_or_else(|| cum_s.get(t).copied().unwrap_or(0));
    (now + Duration::seconds(start), now + Duration::seconds(end))
}

/// Turn a series of expected uses into the planner's EV inputs.
///
/// `uses` must be in time order, which both producers give naturally — the queue is
/// ordered by construction and the generator is walked forward.
pub fn plan_inputs(
    uses: &[ExpectedVehicleUse],
    n: usize,
    cum_s: &[i64],
    now: DateTime<Utc>,
) -> EvPlanInputs {
    let horizon_end_s = cum_s
        .get(n)
        .copied()
        .unwrap_or_else(|| cum_s.last().copied().unwrap_or(0));
    let horizon_end = now + Duration::seconds(horizon_end_s);

    // ── Availability: the union of charging windows, plus the tail after a known
    // final return. Without that tail the vehicle would be unchargeable from its
    // last return to the horizon edge even though it is home; with it, a final use
    // whose return is UNKNOWN still contributes no tail, which is the honest answer
    // — nothing said the car came back.
    let mut windows: Vec<(DateTime<Utc>, DateTime<Utc>)> = uses
        .iter()
        .map(|u| (u.window_start, u.departure_at))
        .collect();
    if let Some(last) = uses.last() {
        if let Some(c) = &last.consumption {
            if c.return_at < horizon_end {
                windows.push((c.return_at, horizon_end));
            }
        }
    }
    let available_per_slot = (0..n)
        .map(|t| {
            let (slot_start, slot_end) = slot_bounds(cum_s, t, now);
            windows.iter().any(|(open, close)| {
                // A window must already be open when the slot starts — except in the
                // slot in progress, where a window opening part-way through still
                // counts. Those are two different situations, not one rule with an
                // exception:
                //
                // GB-54 aligns a plan's `now` to the slot grid, so a session created
                // at 15:25 belongs to a slot that began at 15:00. Requiring the window
                // to pre-date the slot start locked the EV out of the whole slot in
                // progress — up to an hour of charging lost, in the one slot dispatch
                // actually acts on.
                //
                // Later slots are a different question. Treating a mid-slot opening as
                // chargeable there would let the plan draw full power through a slot
                // the vehicle is away for most of — promising charge the charger
                // refuses, which is exactly the class of defect the charge-limit fix
                // removed. So from slot 1 on, the vehicle must be present at the
                // slot's start, which is also what `is_away_at` answers and therefore
                // what the live tick will do.
                let opened_in_time = if t == 0 {
                    *open < slot_end
                } else {
                    *open <= slot_start
                };
                opened_in_time && slot_start < *close
            })
        })
        .collect();

    // ── Consumption: each use's own drop, at its own stated return.
    let mut drop_frac_per_slot = vec![0.0; n];
    for u in uses {
        let Some(c) = &u.consumption else { continue };
        if c.soc_drop_frac <= 0.0 {
            continue;
        }
        // A return beyond the horizon is not this plan's business. `return_slot`
        // clamps to the last slot, which would book the charge loss inside the horizon
        // at a time the vehicle has not actually got back - and with a daily trip the
        // next day's return is routinely just past the edge, so the plan would subtract
        // two trips' worth of charge while modelling one. The next cycle, whose horizon
        // reaches further, places it properly.
        let secs = (c.return_at - now).num_seconds();
        if secs >= horizon_end_s {
            continue;
        }
        let at = return_slot(cum_s, n, secs);
        // Slot 0 never carries a drop: a return already in the past is reflected in
        // the live state of charge the plan starts from, and counting it again would
        // charge the trip twice.
        if at > 0 {
            drop_frac_per_slot[at] += c.soc_drop_frac;
        }
    }

    // ── Obligations: one per firm departure inside the horizon.
    let obligations = uses
        .iter()
        .filter(|u| u.firm)
        .filter(|u| (u.departure_at - now).num_seconds() <= horizon_end_s)
        .map(|u| EvObligation {
            deadline_step: deadline_slot(cum_s, n, (u.departure_at - now).num_seconds()),
            target_soc_frac: u.target_soc_frac,
            session_id: u.session_id,
        })
        .collect();

    EvPlanInputs {
        available_per_slot,
        drop_frac_per_slot,
        obligations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 4, 0, 0, 0).unwrap()
    }

    /// 24 one-hour slots. `cum_s` holds n+1 boundaries.
    fn grid(n: usize) -> Vec<i64> {
        (0..=n).map(|t| t as i64 * 3600).collect()
    }

    fn at(h: i64) -> DateTime<Utc> {
        now() + Duration::hours(h)
    }

    fn use_at(open: i64, depart: i64, ret: Option<i64>, drop: f64) -> ExpectedVehicleUse {
        ExpectedVehicleUse {
            window_start: at(open),
            departure_at: at(depart),
            target_soc_frac: 0.8,
            firm: true,
            consumption: ret.map(|r| ExpectedTripConsumption {
                return_at: at(r),
                soc_drop_frac: drop,
            }),
            session_id: None,
        }
    }

    #[test]
    fn a_window_makes_only_its_own_slots_chargeable() {
        let n = 24;
        let out = plan_inputs(&[use_at(2, 5, None, 0.0)], n, &grid(n), now());
        assert!(!out.available_per_slot[1], "before the window opens");
        assert!(out.available_per_slot[2], "first slot of the window");
        assert!(out.available_per_slot[4], "last slot before departure");
        assert!(!out.available_per_slot[5], "the departure slot itself");
        assert!(
            !out.available_per_slot[20],
            "no tail without a known return"
        );
    }

    #[test]
    fn the_slot_in_progress_counts_as_chargeable() {
        // GB-54: an aligned `now` sits before wall-clock now, so a window opening
        // mid-slot must still make that slot usable — testing the slot's start
        // instead would lose up to a whole slot of charging.
        let n = 24;
        let cum = grid(n);
        let mut u = use_at(0, 5, None, 0.0);
        u.window_start = now() + Duration::minutes(25); // inside slot 0
        let out = plan_inputs(&[u], n, &cum, now());
        assert!(
            out.available_per_slot[0],
            "a window opening inside slot 0 makes slot 0 chargeable"
        );
    }

    #[test]
    fn a_known_final_return_opens_the_tail_to_the_horizon() {
        let n = 24;
        let out = plan_inputs(&[use_at(2, 5, Some(9), 0.2)], n, &grid(n), now());
        assert!(!out.available_per_slot[6], "away");
        assert!(!out.available_per_slot[8], "still away");
        assert!(out.available_per_slot[9], "home again, so chargeable");
        assert!(
            out.available_per_slot[23],
            "and stays chargeable to the edge"
        );
    }

    #[test]
    fn an_unknown_final_return_opens_no_tail() {
        let n = 24;
        let out = plan_inputs(&[use_at(2, 5, None, 0.0)], n, &grid(n), now());
        assert!(
            !out.available_per_slot[9..].iter().any(|a| *a),
            "nothing said the car came back, so nothing claims it is chargeable"
        );
    }

    #[test]
    fn a_lone_use_drop_lands_at_its_own_return() {
        // The defect this replaces: the drop used to be placed at the NEXT session's
        // window start, so a single session's stated distance did nothing at all.
        let n = 24;
        let out = plan_inputs(&[use_at(2, 5, Some(9), 0.25)], n, &grid(n), now());
        assert!((out.drop_frac_per_slot[9] - 0.25).abs() < 1e-9);
        assert!(
            (out.drop_frac_per_slot.iter().sum::<f64>() - 0.25).abs() < 1e-9,
            "exactly once, in exactly one slot"
        );
    }

    #[test]
    fn a_return_already_past_is_not_charged_twice() {
        let n = 24;
        let mut u = use_at(0, 5, Some(0), 0.3);
        u.consumption = Some(ExpectedTripConsumption {
            return_at: now() - Duration::hours(2),
            soc_drop_frac: 0.3,
        });
        let out = plan_inputs(&[u], n, &grid(n), now());
        assert_eq!(
            out.drop_frac_per_slot.iter().sum::<f64>(),
            0.0,
            "already reflected in the live state of charge the plan starts from"
        );
    }

    #[test]
    fn every_firm_departure_binds_its_own_obligation() {
        // The forecast path produced one obligation however many trips the horizon
        // held, so every trip after the first had a mask and a drop but no goal.
        let n = 24;
        let uses = vec![use_at(0, 5, Some(9), 0.2), use_at(9, 14, Some(18), 0.25)];
        let out = plan_inputs(&uses, n, &grid(n), now());
        assert_eq!(out.obligations.len(), 2);
        assert_eq!(out.obligations[0].deadline_step, 4);
        assert_eq!(out.obligations[1].deadline_step, 13);
        assert!(out.obligations.iter().all(|o| o.target_soc_frac == 0.8));
    }

    #[test]
    fn a_soft_departure_states_no_obligation_but_keeps_its_window_and_drop() {
        let n = 24;
        let mut u = use_at(2, 5, Some(9), 0.2);
        u.firm = false;
        let out = plan_inputs(&[u], n, &grid(n), now());
        assert!(
            out.obligations.is_empty(),
            "a preference is not a guarantee"
        );
        assert!(out.available_per_slot[3], "but availability is fact");
        assert!(
            (out.drop_frac_per_slot[9] - 0.2).abs() < 1e-9,
            "and so is what the trip costs"
        );
    }

    #[test]
    fn a_departure_beyond_the_horizon_binds_nothing_yet() {
        let n = 6; // 6 h horizon
        let out = plan_inputs(&[use_at(0, 20, Some(22), 0.2)], n, &grid(n), now());
        assert!(
            out.obligations.is_empty(),
            "the next cycle will see it; this one cannot serve it"
        );
    }

    #[test]
    fn a_departure_exactly_at_the_horizon_end_still_binds() {
        // cum_s holds n+1 boundaries, so the horizon ENDS at cum_s[n]. Using
        // cum_s[n-1] silently dropped a departure in the final slot.
        let n = 6;
        let out = plan_inputs(&[use_at(0, 6, None, 0.0)], n, &grid(n), now());
        assert_eq!(out.obligations.len(), 1);
    }

    #[test]
    fn two_returns_in_one_slot_accumulate() {
        // Not reachable from one vehicle, but the derivation must not silently drop
        // one if a coarse far-horizon slot ever holds both.
        let n = 24;
        let a = use_at(0, 2, Some(10), 0.1);
        let b = use_at(10, 12, Some(10), 0.15);
        let out = plan_inputs(&[a, b], n, &grid(n), now());
        assert!((out.drop_frac_per_slot[10] - 0.25).abs() < 1e-9);
    }

    #[test]
    fn an_empty_series_leaves_nothing_chargeable_and_nothing_required() {
        let n = 8;
        let out = plan_inputs(&[], n, &grid(n), now());
        assert_eq!(out.available_per_slot, vec![false; n]);
        assert_eq!(out.drop_frac_per_slot, vec![0.0; n]);
        assert!(out.obligations.is_empty());
    }

    #[test]
    fn a_deadline_and_a_return_on_the_same_boundary_land_in_different_slots() {
        // The two instants need OPPOSITE boundary rules, and sharing one helper between
        // them put every boundary-aligned drop a slot early. A departure at +9 h must be
        // met by the slot that ENDS at 9 h (slot 8); a return at +9 h means the charge is
        // gone from the slot that STARTS at 9 h (slot 9).
        let n = 24;
        let cum = grid(n);
        let depart_then_return = ExpectedVehicleUse {
            window_start: at(0),
            departure_at: at(9),
            target_soc_frac: 0.8,
            firm: true,
            consumption: Some(ExpectedTripConsumption {
                return_at: at(9),
                soc_drop_frac: 0.2,
            }),
            session_id: None,
        };
        let out = plan_inputs(&[depart_then_return], n, &cum, now());
        assert_eq!(
            out.obligations[0].deadline_step, 8,
            "ready by the slot ending at 9 h"
        );
        assert!(
            (out.drop_frac_per_slot[9] - 0.2).abs() < 1e-9,
            "the drop belongs to the slot starting at 9 h, got {:?}",
            out.drop_frac_per_slot
                .iter()
                .enumerate()
                .filter(|(_, d)| **d > 0.0)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_return_beyond_the_horizon_is_not_booked_inside_it() {
        // `return_slot` clamps to the last slot, so without an explicit guard a trip
        // returning after the horizon ends would have its charge loss recorded in the
        // final slot - at a time the vehicle has not actually got back. With a daily
        // trip the next day's return sits just past the edge routinely, so the plan
        // would subtract two trips' worth of charge while modelling one.
        let n = 12; // 12 h horizon
        let cum = grid(n);
        let inside = ExpectedVehicleUse {
            window_start: at(0),
            departure_at: at(2),
            target_soc_frac: 0.8,
            firm: true,
            consumption: Some(ExpectedTripConsumption {
                return_at: at(6),
                soc_drop_frac: 0.20,
            }),
            session_id: None,
        };
        let beyond = ExpectedVehicleUse {
            window_start: at(6),
            departure_at: at(11),
            target_soc_frac: 0.8,
            firm: true,
            consumption: Some(ExpectedTripConsumption {
                return_at: at(30), // long after the horizon ends
                soc_drop_frac: 0.25,
            }),
            session_id: None,
        };
        let out = plan_inputs(&[inside, beyond], n, &cum, now());
        let total: f64 = out.drop_frac_per_slot.iter().sum();
        assert!(
            (total - 0.20).abs() < 1e-9,
            "only the return inside the horizon counts, got {total}: {:?}",
            out.drop_frac_per_slot
        );
        assert!((out.drop_frac_per_slot[6] - 0.20).abs() < 1e-9);
        // The departure itself still binds, because it IS inside the horizon.
        assert_eq!(out.obligations.len(), 2);
    }
}
