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
    soc_max: f64,
    battery_kwh: f64,
    v_ev_core_eur_kwh: f64,
    v_ev_extra_eur_kwh: f64,
    w_ghg_eur_kg: f64,
) -> Vec<EvEnergySegment> {
    let start = soc_init.clamp(0.0, 1.0);
    // The vehicle's charge limit, not a full pack, is where the bands stop.
    // `EvCharger::capability_inner` reports zero import capability at or above it, so
    // a band beyond it prices energy the charger will refuse — a plan promising charge
    // that never arrives. ven-2 was planned to 0.998 against a 0.85 limit while a week
    // of measurements never once exceeded 0.850.
    let ceiling = soc_max.clamp(0.0, 1.0);
    if battery_kwh <= 0.0 || start >= ceiling {
        return Vec::new();
    }

    // Fill levels that bound the bands: the range ends, plus every curve
    // breakpoint inside it (or the target, when no curve was expressed).
    //
    // The target stays a *breakpoint* rather than becoming the bound: a session may ask
    // for less than the vehicle can hold, and the energy between the two is real,
    // reachable, and worth less than the requested charge — which is exactly what the
    // two prices below express.
    let mut bounds: Vec<f64> = vec![start, ceiling];
    if rates.is_empty() {
        let target = soc_target.clamp(0.0, 1.0);
        if target > start && target < ceiling {
            bounds.push(target);
        }
    } else {
        bounds.extend(
            rates
                .iter()
                .map(|r| r.fill)
                .filter(|&f| f > start && f < ceiling),
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
    fine.push(*bounds.last().unwrap_or(&ceiling));
    let bounds = fine;

    let fine_bands = bounds.windows(2).filter_map(|w| {
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
    });

    // Merge adjacent bands that carry the same bid. Two bands priced alike are
    // one decision split in two — the solver gains nothing from being allowed to
    // fill them separately, and pays for the extra variables.
    //
    // This is not a micro-optimisation. The subdivision above is unconditional,
    // but a *flat* stretch of a curve has nothing to resolve, and the fleet's own
    // sessions express no curve at all (`engage_charge_planning` sets a target,
    // not a bid), so their price is a two-step function with one breakpoint —
    // 14 bands carrying 2 distinct prices. Measured on ven-11 (EV + base load,
    // no heater, 288 slots): median solve 114 ms before this change shipped,
    // 6426 ms with the unmerged bands, i.e. 56x for information the model did
    // not contain. See the R-97 note in `docs/reference/TECHNICAL_DEBTS.md`.
    let mut merged: Vec<EvEnergySegment> = Vec::new();
    for band in fine_bands {
        match merged.last_mut() {
            Some(prev) if (prev.eur_per_kwh - band.eur_per_kwh).abs() < 1e-9 => {
                prev.kwh += band.kwh;
            }
            _ => merged.push(band),
        }
    }
    merged
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

    /// A vehicle with no charge limit below full, so the existing expectations below
    /// are unchanged: the limit is a new dimension, not a new meaning for the old ones.
    fn segments(rates: &[ComfortRate], soc_init: f64, soc_target: f64) -> Vec<EvEnergySegment> {
        ev_energy_segments(
            rates,
            soc_init,
            soc_target,
            1.0,
            BATTERY_KWH,
            1.0,
            0.10,
            0.5,
        )
    }

    fn segments_limited(
        rates: &[ComfortRate],
        soc_init: f64,
        soc_target: f64,
        soc_max: f64,
    ) -> Vec<EvEnergySegment> {
        ev_energy_segments(
            rates,
            soc_init,
            soc_target,
            soc_max,
            BATTERY_KWH,
            1.0,
            0.10,
            0.5,
        )
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

    /// R-97: a band is a decision, and two bands at the same bid are one
    /// decision split in two. Every extra band is a variable the solver must
    /// branch over, so a price that does not vary must not produce more than one.
    #[test]
    fn bands_at_the_same_bid_are_one_band() {
        // The fleet's own case: `engage_charge_planning` sets a target, never a
        // curve, so the price is the two-step default. Unmerged, the 5 %-SoC
        // subdivision turns that into 14 bands carrying 2 distinct prices — and
        // measured 56x the solve time on ven-11.
        let segs = segments(&[], 0.30, 0.80);
        assert_eq!(
            segs.len(),
            2,
            "a two-step price is two decisions, however finely the range is cut: {segs:?}"
        );

        // A flat curve is one price over the whole range, so one band.
        let flat = segments(&[pt(0.0, 0.25, 0.0), pt(1.0, 0.25, 0.0)], 0.10, 0.80);
        assert_eq!(flat.len(), 1, "a flat curve is one decision: {flat:?}");

        // A ramp genuinely varies, so it keeps its resolution.
        let ramp = segments(&[pt(0.0, 0.40, 0.0), pt(1.0, 0.00, 0.0)], 0.10, 0.80);
        assert!(
            ramp.len() > 5,
            "a sloping curve must keep its shape, got {} bands",
            ramp.len()
        );
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

    // ── The vehicle's charge limit bounds the bands ───────────────────────────
    //
    // `EvCharger::capability_inner` reports zero import capability at or above
    // `soc_target`, so energy above it cannot be delivered. Pricing a band there made
    // the planner promise charge the charger then refused: ven-2 was planned to 0.998
    // against a 0.85 limit, and a week of 1-minute history (10,078 samples) never once
    // measured above 0.850.

    #[test]
    fn no_band_prices_energy_above_the_charge_limit() {
        // Limit and goal coincide, which is the fleet's own configuration.
        let segs = segments_limited(&[], 0.64, 0.85, 0.85);
        let total: f64 = segs.iter().map(|s| s.kwh).sum();
        assert!(
            (total - BATTERY_KWH * 0.21).abs() < 1e-9,
            "expected only 0.64 -> 0.85, got {total} kWh: {segs:?}"
        );
        assert!(
            segs.iter().all(|s| (s.eur_per_kwh - 1.0).abs() < 1e-9),
            "with nothing above the goal there is no cheaper tier to buy: {segs:?}"
        );
    }

    #[test]
    fn a_goal_below_the_limit_keeps_its_cheaper_tier() {
        // The two-tier structure is not the bug and is not removed: a session may ask
        // for less than the vehicle can hold, and that gap is real and reachable.
        let segs = segments_limited(&[], 0.30, 0.60, 0.85);
        let core: f64 = segs
            .iter()
            .filter(|s| (s.eur_per_kwh - 1.0).abs() < 1e-9)
            .map(|s| s.kwh)
            .sum();
        let extra: f64 = segs
            .iter()
            .filter(|s| (s.eur_per_kwh - 0.10).abs() < 1e-9)
            .map(|s| s.kwh)
            .sum();
        assert!(
            (core - BATTERY_KWH * 0.30).abs() < 1e-9,
            "to the goal: {segs:?}"
        );
        assert!(
            (extra - BATTERY_KWH * 0.25).abs() < 1e-9,
            "goal to the limit, never past it: {segs:?}"
        );
    }

    #[test]
    fn a_curve_is_clipped_at_the_limit_too() {
        // A user's curve may express bids up to a full pack; the vehicle still will not
        // accept them, so those breakpoints must not create bands.
        let segs = segments_limited(
            &[pt(0.0, 0.50, 0.0), pt(0.9, 0.20, 0.0), pt(1.0, 0.05, 0.0)],
            0.40,
            0.80,
            0.80,
        );
        let total: f64 = segs.iter().map(|s| s.kwh).sum();
        assert!((total - BATTERY_KWH * 0.40).abs() < 1e-9, "{segs:?}");
    }

    #[test]
    fn a_vehicle_already_at_its_limit_has_nothing_to_value() {
        assert!(segments_limited(&[], 0.85, 0.85, 0.85).is_empty());
        // And above it — a limit lowered under a fuller pack — is not negative energy.
        assert!(segments_limited(&[], 0.92, 0.85, 0.85).is_empty());
    }
}
