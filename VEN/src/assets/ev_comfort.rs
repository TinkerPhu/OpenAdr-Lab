//! BL-34 / BL-17 / `ev-comfort-piecewise-core`: turns an EV session's comfort curve
//! (price + CO2 bid) into the priced energy bands `EvMilpContext` buys from.
//!
//! The curve is a **marginal-value curve over state of charge**: at each fill level,
//! the most the user will pay for the next kWh. Walking it from the EV's current SoC
//! to full produces `EvEnergySegment`s — one per curve interval — which the planner
//! treats as independent continuous quantities. That is what replaced the old
//! all-or-nothing "core" block (see GB-41): a bid that covers part of the energy now
//! buys that part.
//!
//! Split out of `ev_milp.rs` to keep it under the VEN/src/ 500-production-line cap
//! (`ven-architecture` rule, `.claude/CLAUDE.md`).

use crate::controller::milp_planner::asset_port::EvEnergySegment;
use crate::entities::asset::ComfortRate;

/// Price the energy from `soc_init` to full as bands, one per comfort-curve
/// interval, each carrying the bid the user expressed there.
///
/// Each band is priced at its **midpoint** fill rather than its start: the curve
/// is piecewise-linear, so the midpoint value is the interval's exact average
/// marginal value, which makes the total reward equal the area under the user's
/// own curve (pinned by `segment_value_equals_the_area_under_the_users_curve`).
///
/// With **no curve expressed** (the legacy `/ev-session` route, a VTN-commanded
/// session, or any request that never set one) the caller's profile defaults stand
/// in as a two-step curve: `v_ev_core_eur_kwh` up to `soc_target`, then
/// `v_ev_extra_eur_kwh` beyond it — exactly the economics those sessions had before
/// this change.
///
/// CO2 bids are monetized here (`g/kWh ÷ 1000 × w_ghg_eur_kg`) and added to the
/// price bid, so the objective has no unit conversion left to do.
pub(super) fn ev_energy_segments(
    rates: &[ComfortRate],
    soc_init: f64,
    soc_target: f64,
    battery_kwh: f64,
    v_ev_core_eur_kwh: f64,
    v_ev_extra_eur_kwh: f64,
    w_ghg_eur_kg: f64,
) -> Vec<EvEnergySegment> {
    let start = soc_init.clamp(0.0, 1.0);
    if battery_kwh <= 0.0 || start >= 1.0 {
        return Vec::new();
    }

    // Fill levels that bound the bands: the range ends, plus every curve
    // breakpoint inside it (or the target, when no curve was expressed).
    let mut bounds: Vec<f64> = vec![start, 1.0];
    if rates.is_empty() {
        let target = soc_target.clamp(0.0, 1.0);
        if target > start && target < 1.0 {
            bounds.push(target);
        }
    } else {
        bounds.extend(
            rates
                .iter()
                .map(|r| r.fill)
                .filter(|&f| f > start && f < 1.0),
        );
    }
    bounds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    bounds.dedup_by(|a, b| (*a - *b).abs() < 1e-12);

    // Subdivide wide intervals. A curve's breakpoints alone are not enough
    // resolution: a two-point ramp (0.35 €/kWh at empty, 0.05 at full) has no
    // interior breakpoint, so a single band would be priced at the ramp's
    // average and the declining shape — the whole point of a curve — would be
    // lost. Splitting into ≤5 %-SoC bands keeps the shape while staying
    // continuous, which is what lets the solver buy the valuable part of a ramp
    // and leave the rest.
    const MAX_BAND_FILL: f64 = 0.05;
    let mut fine: Vec<f64> = Vec::with_capacity(bounds.len() * 4);
    for w in bounds.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let steps = (((hi - lo) / MAX_BAND_FILL).ceil() as usize).max(1);
        for k in 0..steps {
            fine.push(lo + (hi - lo) * (k as f64) / (steps as f64));
        }
    }
    fine.push(*bounds.last().unwrap_or(&1.0));
    let bounds = fine;

    bounds
        .windows(2)
        .filter_map(|w| {
            let (lo, hi) = (w[0], w[1]);
            let kwh = (hi - lo) * battery_kwh;
            if kwh <= 1e-12 {
                return None;
            }
            let mid = (lo + hi) / 2.0;
            let eur_per_kwh = if rates.is_empty() {
                // No bid expressed: the profile's own two-step default.
                if mid <= soc_target {
                    v_ev_core_eur_kwh
                } else {
                    v_ev_extra_eur_kwh
                }
            } else {
                ComfortRate::value_at_fill(rates, mid)
                    + (ComfortRate::co2_value_at_fill(rates, mid) / 1000.0) * w_ghg_eur_kg
            };
            Some(EvEnergySegment { kwh, eur_per_kwh })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset::ComfortRate;

    // The two tests that lived here asserted the old two-point reading
    // (`value_at_fill(0.0)` for the whole core, `(1.0)` for everything beyond
    // target). `ev-comfort-piecewise-core` removes that reading on purpose, so
    // they are replaced by the segment tests below rather than kept green
    // against a model that no longer exists.

    fn pt(fill: f64, bid: f64, co2: f64) -> ComfortRate {
        ComfortRate {
            fill,
            max_marginal_price: bid,
            max_marginal_co2: co2,
        }
    }

    const BATTERY_KWH: f64 = 50.0;

    fn segments(rates: &[ComfortRate], soc_init: f64, soc_target: f64) -> Vec<EvEnergySegment> {
        ev_energy_segments(rates, soc_init, soc_target, BATTERY_KWH, 1.0, 0.10, 0.5)
    }

    #[test]
    fn segments_span_the_energy_from_here_to_full() {
        let segs = segments(&[pt(0.0, 0.40, 0.0), pt(1.0, 0.10, 0.0)], 0.30, 0.80);
        let total: f64 = segs.iter().map(|s| s.kwh).sum();
        assert!(
            (total - BATTERY_KWH * 0.70).abs() < 1e-9,
            "expected 35 kWh from 30 % to full, got {total}"
        );
    }

    #[test]
    fn segment_bids_never_rise() {
        let segs = segments(
            &[pt(0.0, 0.50, 0.0), pt(0.4, 0.30, 0.0), pt(1.0, 0.05, 0.0)],
            0.20,
            0.80,
        );
        for w in segs.windows(2) {
            assert!(
                w[1].eur_per_kwh <= w[0].eur_per_kwh + 1e-12,
                "bids must not rise across segments: {:?}",
                segs
            );
        }
    }

    #[test]
    fn a_single_point_curve_values_every_kwh_the_same() {
        let segs = segments(&[pt(0.3, 0.22, 0.0)], 0.40, 0.80);
        assert!(!segs.is_empty());
        for s in &segs {
            assert!((s.eur_per_kwh - 0.22).abs() < 1e-9, "{segs:?}");
        }
    }

    #[test]
    fn an_empty_curve_falls_back_to_the_profile_defaults_split_at_the_target() {
        // No curve expressed: behave exactly as before this change — the
        // profile's core rate up to the target, its extra rate beyond it.
        let segs = segments(&[], 0.30, 0.80);
        let below: f64 = segs
            .iter()
            .filter(|s| (s.eur_per_kwh - 1.0).abs() < 1e-9)
            .map(|s| s.kwh)
            .sum();
        let above: f64 = segs
            .iter()
            .filter(|s| (s.eur_per_kwh - 0.10).abs() < 1e-9)
            .map(|s| s.kwh)
            .sum();
        assert!(
            (below - 25.0).abs() < 1e-9,
            "25 kWh to the target: {segs:?}"
        );
        assert!((above - 10.0).abs() < 1e-9, "10 kWh beyond it: {segs:?}");
    }

    #[test]
    fn the_co2_bid_is_monetized_into_the_same_price() {
        // w_ghg = 0.5 EUR/kgCO2, so a flat 300 g/kWh bid adds 0.15 EUR/kWh.
        let with_co2 = segments(&[pt(0.0, 0.20, 300.0), pt(1.0, 0.20, 300.0)], 0.50, 0.80);
        let without = segments(&[pt(0.0, 0.20, 0.0), pt(1.0, 0.20, 0.0)], 0.50, 0.80);
        assert!((with_co2[0].eur_per_kwh - without[0].eur_per_kwh - 0.15).abs() < 1e-9);
    }

    #[test]
    fn a_full_battery_has_nothing_left_to_value() {
        assert!(segments(&[pt(0.0, 0.40, 0.0), pt(1.0, 0.10, 0.0)], 1.0, 0.80).is_empty());
    }

    #[test]
    fn segment_value_equals_the_area_under_the_users_curve() {
        // A straight 0.40 -> 0.00 ramp over the whole battery: the user's total
        // willingness to pay is the triangle, 0.20 EUR/kWh x 50 kWh = 10 EUR.
        // Pricing each segment at its midpoint makes the sum exact, which is
        // why midpoints are used rather than segment starts.
        let segs = segments(&[pt(0.0, 0.40, 0.0), pt(1.0, 0.0, 0.0)], 0.0, 0.80);
        let total_eur: f64 = segs.iter().map(|s| s.kwh * s.eur_per_kwh).sum();
        assert!(
            (total_eur - 10.0).abs() < 1e-6,
            "expected the area under the curve (10 EUR), got {total_eur}"
        );
    }
}
