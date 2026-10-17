//! A comfort curve as cumulative spend: "60 % for €2.00, 80 % for €2.50" in, the marginal bids the
//! planner reads out, and a stored curve read back the same way.
//!
//! The stored form never changes: it stays the marginal curve `entities::comfort::validate_curve`
//! guards and `assets::ev_comfort` prices. This module only converts at the boundary. It reads the
//! asset's own fill span (`ComfortFillSpan`, declared by the asset) and the asset's own priced bands
//! for the way back; it never integrates a curve itself and never decides what an asset can hold.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::asset::ComfortRate;
use super::comfort::{validate_curve, EnergyBand};

/// How far below the next point a step's price is held, as a fill fraction. The stored curve is
/// piecewise-linear, so a constant-price interval is written as a point at its start and one this
/// close to its end, and the price falls across the gap. At 60 kWh and a 0.10 €/kWh step the gap
/// misprices about €0.003 per knot (pinned by `a_cumulative_curve_reads_back_within_a_cent`).
const STEP_GAP_FILL: f64 = 0.001;

/// Narrowest interval a spend point may describe [fill]: wide enough that the step's two points
/// never meet.
const MIN_INTERVAL_FILL: f64 = 0.01;

/// What an asset declares about the energy its curve prices: where the priced energy starts and
/// stops, and what a unit of fill is worth in kWh. Measured now, in absolute fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComfortFillSpan {
    /// Current fill: the first priced kWh is the one that leaves it.
    pub from_fill: f64,
    /// The asset's limit: no kWh above it is priced.
    pub limit_fill: f64,
    /// Energy of a full range of fill [kWh].
    pub capacity_kwh: f64,
}

/// One cumulative-spend point: the most you will pay in total to reach `fill`, counted from the
/// asset's current charge. `max_marginal_co2` carries the CO2 bid of the interval ending here,
/// unchanged by this conversion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ComfortSpendPoint {
    pub fill: f64,
    pub cumulative_max_cost_eur: f64,
    #[serde(default)]
    pub max_marginal_co2: f64,
}

/// The marginal curve a cumulative-spend curve means. Each interval's bid is its added spend over
/// its energy, held constant across the interval. `Err` names the offending point in spend terms;
/// the result also passes `validate_curve`, the one rule for what a stored curve may look like.
pub fn rates_from_cumulative_spend(
    points: &[ComfortSpendPoint],
    span: &ComfortFillSpan,
) -> Result<Vec<ComfortRate>, String> {
    if points.is_empty() {
        return Err("spend curve must contain at least one point".into());
    }
    if !(span.capacity_kwh.is_finite() && span.capacity_kwh > 0.0) {
        return Err("this asset declares no energy per fill".into());
    }
    let (mut lo_fill, mut prev_spend_eur, mut prev_bid): (f64, f64, Option<f64>) =
        (span.from_fill, 0.0, None);
    let mut rates: Vec<ComfortRate> = Vec::with_capacity(points.len() * 2);
    for (i, p) in points.iter().enumerate() {
        if !(p.fill.is_finite() && p.cumulative_max_cost_eur.is_finite()) {
            return Err(format!("point {i}: values must be finite"));
        }
        if p.fill > 1.0 {
            return Err(format!("point {i}: fill {} above 1", p.fill));
        }
        if p.fill < lo_fill + MIN_INTERVAL_FILL {
            return Err(format!(
                "point {i}: fill {} must be at least {} above the previous point (the current charge for the first)",
                p.fill, MIN_INTERVAL_FILL
            ));
        }
        if p.cumulative_max_cost_eur < prev_spend_eur {
            return Err(format!(
                "point {i}: total spend €{} is below the €{prev_spend_eur} of the point before",
                p.cumulative_max_cost_eur
            ));
        }
        let kwh = (p.fill - lo_fill) * span.capacity_kwh;
        let bid = (p.cumulative_max_cost_eur - prev_spend_eur) / kwh;
        if let Some(prev) = prev_bid {
            if bid > prev * (1.0 + 1e-9) + 1e-12 {
                return Err(format!(
                    "point {i}: adds €{bid:.3} per kWh between {:.0} % and {:.0} %, more than the \
                     €{prev:.3} per kWh of the interval before — what you will pay per kWh cannot rise with charge",
                    lo_fill * 100.0,
                    p.fill * 100.0
                ));
            }
        }
        let bid = prev_bid.map_or(bid, |prev| bid.min(prev));
        for fill in [lo_fill, p.fill - STEP_GAP_FILL] {
            rates.push(ComfortRate {
                fill,
                max_marginal_price_eur_kwh: bid,
                max_marginal_co2: p.max_marginal_co2,
            });
        }
        (lo_fill, prev_spend_eur, prev_bid) = (p.fill, p.cumulative_max_cost_eur, Some(bid));
    }
    // Close the last step at its own end, so the stored curve has a point where the user's last
    // point is (the others close one gap early, and the next step opens on the point itself).
    if let (Some(last), Some(bid)) = (points.last(), prev_bid) {
        rates.push(ComfortRate {
            fill: last.fill,
            max_marginal_price_eur_kwh: bid,
            max_marginal_co2: last.max_marginal_co2,
        });
    }
    validate_curve(&rates)?;
    Ok(rates)
}

/// Cumulative spend at `fill` over bands that start at `span.from_fill`: the kWh up to there,
/// each at its band's bid, the last band prorated.
fn cumulative_spend_at(bands: &[EnergyBand], span: &ComfortFillSpan, fill: f64) -> f64 {
    let mut remaining_kwh = ((fill - span.from_fill) * span.capacity_kwh).max(0.0);
    let mut spend_eur = 0.0;
    for band in bands {
        let taken_kwh = band.kwh.min(remaining_kwh);
        spend_eur += taken_kwh * band.eur_per_kwh;
        remaining_kwh -= taken_kwh;
        if remaining_kwh <= 1e-12 {
            break;
        }
    }
    spend_eur
}

/// A stored curve read back as cumulative spend: one point at each stored fill inside the priced
/// range, thinned where two sit within a step's gap, and one at the asset's limit. `bands` are the
/// asset's own bands for `rates` (`Asset::comfort_bands`), so the view is the area the planner
/// prices, never a second integral.
pub fn cumulative_spend_view(
    rates: &[ComfortRate],
    bands: &[EnergyBand],
    span: &ComfortFillSpan,
) -> Vec<ComfortSpendPoint> {
    let mut fills: Vec<(f64, f64)> = rates
        .iter()
        .map(|r| (r.fill, r.max_marginal_co2))
        .filter(|&(f, _)| f > span.from_fill + 1e-9 && f < span.limit_fill - 1e-9)
        .collect();
    fills.push((
        span.limit_fill,
        rates.last().map_or(0.0, |r| r.max_marginal_co2),
    ));
    let mut kept: Vec<(f64, f64)> = Vec::with_capacity(fills.len());
    for (i, &(fill, co2)) in fills.iter().enumerate() {
        let next = fills.get(i + 1).map(|&(f, _)| f);
        if next.is_some_and(|n| n - fill < 1.5 * STEP_GAP_FILL) {
            continue;
        }
        kept.push((fill, co2));
    }
    kept.into_iter()
        .map(|(fill, max_marginal_co2)| ComfortSpendPoint {
            fill,
            cumulative_max_cost_eur: cumulative_spend_at(bands, span, fill),
            max_marginal_co2,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::comfort::max_cost_eur;

    /// 50 kWh pack at 20 % with the vehicle's limit at 100 %, as the worked example in the spec.
    fn span() -> ComfortFillSpan {
        ComfortFillSpan {
            from_fill: 0.20,
            limit_fill: 1.0,
            capacity_kwh: 50.0,
        }
    }

    fn sp(fill: f64, eur: f64) -> ComfortSpendPoint {
        ComfortSpendPoint {
            fill,
            cumulative_max_cost_eur: eur,
            max_marginal_co2: 0.0,
        }
    }

    /// The bands the planner would price for `rates` over this span: the same walk
    /// `assets::ev_comfort` does, restated at test resolution (1 % bands, midpoint priced).
    fn bands_of(rates: &[ComfortRate], span: &ComfortFillSpan) -> Vec<EnergyBand> {
        let steps = ((span.limit_fill - span.from_fill) / 0.001).round() as usize;
        (0..steps)
            .map(|k| {
                let lo = span.from_fill + k as f64 * 0.001;
                EnergyBand {
                    kwh: 0.001 * span.capacity_kwh,
                    eur_per_kwh: ComfortRate::value_at_fill(rates, lo + 0.0005),
                }
            })
            .collect()
    }

    #[test]
    fn rates_from_cumulative_spend_differentiates_each_interval() {
        // 20 % -> 60 % is 20 kWh for EUR 2.00 (0.10/kWh); 60 % -> 80 % is 10 kWh for EUR 0.50 (0.05/kWh).
        let rates =
            rates_from_cumulative_spend(&[sp(0.60, 2.00), sp(0.80, 2.50)], &span()).unwrap();
        let at = |f: f64| ComfortRate::value_at_fill(&rates, f);
        assert!((at(0.30) - 0.10).abs() < 1e-9, "{rates:?}");
        assert!((at(0.58) - 0.10).abs() < 1e-9);
        assert!((at(0.65) - 0.05).abs() < 1e-9);
        assert!((at(0.79) - 0.05).abs() < 1e-9);
    }

    #[test]
    fn a_cumulative_curve_reads_back_within_a_cent() {
        let span = span();
        let rates = rates_from_cumulative_spend(&[sp(0.60, 2.00), sp(0.80, 2.50)], &span).unwrap();
        let view = cumulative_spend_view(&rates, &bands_of(&rates, &span), &span);
        let spend_at = |fill: f64| {
            view.iter()
                .find(|p| (p.fill - fill).abs() < 1e-6)
                .unwrap_or_else(|| panic!("no point at {fill}: {view:?}"))
                .cumulative_max_cost_eur
        };
        assert!((spend_at(0.60) - 2.00).abs() < 0.01, "{view:?}");
        assert!((spend_at(0.80) - 2.50).abs() < 0.01, "{view:?}");
    }

    #[test]
    fn the_view_lists_the_limit_and_thins_a_steps_gap() {
        let span = span();
        let rates = rates_from_cumulative_spend(&[sp(0.60, 2.00), sp(0.80, 2.50)], &span).unwrap();
        let fills: Vec<f64> = cumulative_spend_view(&rates, &bands_of(&rates, &span), &span)
            .iter()
            .map(|p| p.fill)
            .collect();
        assert_eq!(
            fills.len(),
            3,
            "60 %, 80 % and the limit, not the gap points: {fills:?}"
        );
        assert!((fills[2] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_maximum_of_a_spend_curve_is_the_last_total_within_a_cent() {
        let span = ComfortFillSpan {
            limit_fill: 0.80,
            ..span()
        };
        let rates = rates_from_cumulative_spend(&[sp(0.60, 2.00), sp(0.80, 2.50)], &span).unwrap();
        assert!((max_cost_eur(&bands_of(&rates, &span)) - 2.50).abs() < 0.01);
    }

    #[test]
    fn a_rising_interval_is_refused_in_spend_terms() {
        // 20 -> 40 %: EUR 1.00 over 10 kWh = 0.10/kWh; 40 -> 60 %: EUR 2.00 over 10 kWh = 0.20/kWh.
        let err = rates_from_cumulative_spend(&[sp(0.40, 1.00), sp(0.60, 3.00)], &span())
            .expect_err("spend that is not concave must be refused");
        assert!(err.contains("point 1"), "{err}");
        assert!(
            err.contains("per kWh") && err.contains("0.200") && err.contains("0.100"),
            "{err}"
        );
    }

    #[test]
    fn spend_that_falls_is_refused() {
        let err = rates_from_cumulative_spend(&[sp(0.40, 2.00), sp(0.60, 1.50)], &span())
            .expect_err("a total below the previous one is meaningless");
        assert!(err.contains("point 1") && err.contains("below"), "{err}");
    }

    #[test]
    fn points_at_or_below_the_current_charge_or_too_close_are_refused() {
        assert!(rates_from_cumulative_spend(&[sp(0.20, 1.0)], &span()).is_err());
        assert!(rates_from_cumulative_spend(&[sp(0.40, 1.0), sp(0.405, 1.1)], &span()).is_err());
        assert!(rates_from_cumulative_spend(&[], &span()).is_err());
    }

    #[test]
    fn an_asset_without_energy_per_fill_is_refused() {
        let none = ComfortFillSpan {
            capacity_kwh: 0.0,
            ..span()
        };
        let err = rates_from_cumulative_spend(&[sp(0.5, 1.0)], &none).expect_err("no capacity");
        assert!(err.contains("energy per fill"), "{err}");
    }

    #[test]
    fn the_derived_curve_obeys_the_one_validator() {
        let rates =
            rates_from_cumulative_spend(&[sp(0.40, 2.00), sp(0.60, 3.00), sp(1.0, 3.50)], &span())
                .unwrap();
        assert!(validate_curve(&rates).is_ok());
        // A flat curve is one valid case: equal bids interval to interval.
        let flat = rates_from_cumulative_spend(&[sp(0.40, 2.0), sp(0.60, 4.0)], &span()).unwrap();
        assert!(validate_curve(&flat).is_ok());
    }

    #[test]
    fn the_co2_bid_of_a_point_is_carried_unchanged() {
        let p = ComfortSpendPoint {
            max_marginal_co2: 300.0,
            ..sp(0.60, 2.00)
        };
        let rates = rates_from_cumulative_spend(&[p], &span()).unwrap();
        assert!(rates
            .iter()
            .all(|r| (r.max_marginal_co2 - 300.0).abs() < 1e-12));
    }
}
