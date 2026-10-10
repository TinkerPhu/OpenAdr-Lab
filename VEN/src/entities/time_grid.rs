//! The planning time grid: how many slots a plan has, when each starts, and how long each is.
//!
//! Owns the one rule for turning the profile's plan zones into slots, and the one rule for
//! "which slot holds this instant". Reads only `PlanZone`s; knows nothing about tariffs, assets
//! or the solver. The planner's input builder, the plan-cycle inputs and the assets' MILP
//! contexts all use it, so they cannot disagree about where a slot begins.

use super::plan::PlanZone;
use super::units::dt_h_from_s;

/// Slot layout of one plan: `n` slots, slot `t` spanning `cum_s[t]..cum_s[t + 1]` seconds after
/// the plan's start and lasting `dt_h[t]` hours.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeGrid {
    pub n: usize,
    /// Slot start offsets in seconds, plus the end of the last slot: `n + 1` entries, first 0.
    pub cum_s: Vec<i64>,
    pub dt_h: Vec<f64>,
}

impl TimeGrid {
    pub fn from_zones(zones: &[PlanZone]) -> Self {
        let n = slot_count(zones);
        let mut cum_s = Vec::with_capacity(n + 1);
        let mut dt_h = Vec::with_capacity(n);
        let mut end_s = 0i64;
        cum_s.push(end_s);
        for zone in zones {
            let step_h = dt_h_from_s(zone.step_s as f64);
            for _ in 0..zone.slots {
                dt_h.push(step_h);
                end_s += zone.step_s as i64;
                cum_s.push(end_s);
            }
        }
        Self { n, cum_s, dt_h }
    }
}

/// Number of slots the zones add up to.
pub fn slot_count(zones: &[PlanZone]) -> usize {
    zones.iter().map(|z| z.slots).sum()
}

/// The slot that holds the instant `offset_s` seconds after the plan's start: the latest slot
/// whose start is at or before it. An instant before the start is slot 0; one at or past the end
/// is the last slot.
pub fn slot_at(cum_s: &[i64], n: usize, offset_s: i64) -> usize {
    cum_s
        .partition_point(|&start_s| start_s <= offset_s)
        .saturating_sub(1)
        .min(n.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zones() -> Vec<PlanZone> {
        vec![
            PlanZone {
                step_s: 300,
                slots: 3,
            },
            PlanZone {
                step_s: 1800,
                slots: 2,
            },
        ]
    }

    #[test]
    fn from_zones_sums_slots_and_accumulates_their_starts() {
        let grid = TimeGrid::from_zones(&zones());
        assert_eq!(grid.n, 5);
        assert_eq!(grid.cum_s, vec![0, 300, 600, 900, 2700, 4500]);
        assert_eq!(
            grid.dt_h,
            vec![300.0 / 3600.0, 300.0 / 3600.0, 300.0 / 3600.0, 0.5, 0.5]
        );
    }

    #[test]
    fn from_zones_of_nothing_is_an_empty_grid_starting_at_zero() {
        let grid = TimeGrid::from_zones(&[]);
        assert_eq!((grid.n, grid.cum_s, grid.dt_h), (0, vec![0], vec![]));
    }

    #[test]
    fn slot_at_is_the_latest_slot_starting_at_or_before_the_instant() {
        let grid = TimeGrid::from_zones(&zones());
        let at = |offset_s| slot_at(&grid.cum_s, grid.n, offset_s);
        assert_eq!(at(0), 0);
        assert_eq!(at(299), 0);
        assert_eq!(at(300), 1, "a slot owns its own start");
        assert_eq!(at(899), 2);
        assert_eq!(at(900), 3);
        assert_eq!(at(2700), 4);
    }

    #[test]
    fn slot_at_clamps_to_the_first_and_last_slot() {
        let grid = TimeGrid::from_zones(&zones());
        assert_eq!(
            slot_at(&grid.cum_s, grid.n, -60),
            0,
            "before the plan starts"
        );
        assert_eq!(
            slot_at(&grid.cum_s, grid.n, 4500),
            4,
            "the end belongs to the last slot"
        );
        assert_eq!(slot_at(&grid.cum_s, grid.n, 99_999), 4);
        assert_eq!(slot_at(&[0], 0, 10), 0, "an empty grid has no slot to miss");
    }
}
