//! Comfort curves: what a curve must look like to be accepted, and which curve planning uses.
//!
//! A user may replace an asset's built-in `default_comfort_rates()` with their own bid curve
//! (WP4.2, BL-19; the overrides live in `AppState` and persist through `SettingsPort`, see
//! `services::comfort`). The two rules here are pure domain logic, so they live with the type
//! they act on: the infrastructure that builds the planner's asset contexts
//! (`simulator::plan_context`) must be able to ask "which curve applies?" without reaching up
//! into the application ring (R-108).

use std::collections::HashMap;
use ts_rs::TS;

use super::asset::ComfortRate;

/// Bid-price sanity bound [€/kWh] — an order of magnitude above any real tariff.
const MAX_BID_EUR_KWH: f64 = 10.0;

/// Validate a user-provided curve: non-empty, all values finite, fills in
/// [0, 1] and strictly increasing, bids bounded and **non-increasing**.
///
/// The non-increasing rule is load-bearing, not cosmetic: the EV prices energy
/// by walking this curve with one continuous variable per segment
/// (`ev-comfort-piecewise-core`), which is exactly solvable without binaries
/// only while the marginal bid never rises. A rising bid would also read oddly —
/// paying more for the next kWh the fuller the battery gets.
pub fn validate_curve(rates: &[ComfortRate]) -> Result<(), String> {
    if rates.is_empty() {
        return Err("comfort curve must contain at least one point".into());
    }
    let mut prev_fill = -1.0_f64;
    let mut prev_price: Option<f64> = None;
    for (i, r) in rates.iter().enumerate() {
        if !(r.fill.is_finite()
            && r.max_marginal_price_eur_kwh.is_finite()
            && r.max_marginal_co2.is_finite())
        {
            return Err(format!("point {i}: values must be finite"));
        }
        if !(0.0..=1.0).contains(&r.fill) {
            return Err(format!("point {i}: fill {} outside [0, 1]", r.fill));
        }
        if r.fill <= prev_fill {
            return Err(format!(
                "point {i}: fill {} not strictly increasing (previous {prev_fill})",
                r.fill
            ));
        }
        if !(0.0..=MAX_BID_EUR_KWH).contains(&r.max_marginal_price_eur_kwh) {
            return Err(format!(
                "point {i}: max_marginal_price {} outside [0, {MAX_BID_EUR_KWH}] €/kWh",
                r.max_marginal_price_eur_kwh
            ));
        }
        if r.max_marginal_co2 < 0.0 {
            return Err(format!("point {i}: max_marginal_co2 must be ≥ 0"));
        }
        if let Some(prev) = prev_price {
            if r.max_marginal_price_eur_kwh > prev {
                return Err(format!(
                    "point {i}: max_marginal_price {} rises above the previous {prev} — \
                     bids must not increase with fill",
                    r.max_marginal_price_eur_kwh
                ));
            }
        }
        prev_fill = r.fill;
        prev_price = Some(r.max_marginal_price_eur_kwh);
    }
    Ok(())
}

/// One band of energy priced at a single marginal bid: what a comfort curve says the next
/// `kwh` are worth, in €/kWh. Walking a curve from the asset's current charge to its limit
/// produces these (`assets::ev_comfort`); the planner buys the bands whose bid beats their cost
/// (`ev-comfort-piecewise-core`), and the comfort-curve card sums them (`max_cost_eur`).
#[derive(Debug, Clone, PartialEq)]
pub struct EnergyBand {
    /// Energy in this band [kWh].
    pub kwh: f64,
    /// What the user bids for each kWh in it [€/kWh].
    pub eur_per_kwh: f64,
}

/// The most a curve can cost: the sum of kWh × bid over its bands, i.e. the area under the
/// curve. Every kWh the planner buys has a tariff no higher than its bid, so a curve's spend is
/// bounded by this without a constraint.
pub fn max_cost_eur(bands: &[EnergyBand]) -> f64 {
    bands.iter().map(|b| b.kwh * b.eur_per_kwh).sum()
}

/// The energy a set of bands covers [kWh].
pub fn bands_energy_kwh(bands: &[EnergyBand]) -> f64 {
    bands.iter().map(|b| b.kwh).sum()
}

/// What a curve can cost and what the plan spends on its asset, for the comfort-curve card.
/// `max_cost_eur` and `energy_kwh` are `None` for an asset that does not price energy by a
/// curve; `planned_cost_eur` is `0.0` when the plan allocates nothing to the asset.
#[derive(Debug, Clone, PartialEq, serde::Serialize, TS)]
pub struct ComfortCurveCost {
    pub max_cost_eur: Option<f64>,
    pub energy_kwh: Option<f64>,
    pub planned_cost_eur: f64,
}

/// The card's two numbers from the asset's own `bands` and the active plan: the one place
/// that pairs a curve's maximum with the plan's spend.
pub fn comfort_curve_cost(
    bands: Option<&[EnergyBand]>,
    plan: Option<&super::plan::Plan>,
    asset_id: &str,
) -> ComfortCurveCost {
    let planned_cost_eur = plan
        .and_then(|p| {
            let until = p.slots.last()?.end;
            super::plan_cost::asset_planned_cost(&p.slots, asset_id, until)
        })
        .map_or(0.0, |(cost_eur, _co2_g)| cost_eur);
    ComfortCurveCost {
        max_cost_eur: bands.map(max_cost_eur),
        energy_kwh: bands.map(bands_energy_kwh),
        planned_cost_eur,
    }
}

/// The curve planning should use: the user override when present, the
/// asset's built-in default otherwise (BL-19 verify clause).
pub fn effective_comfort_rates(
    overrides: &HashMap<String, Vec<ComfortRate>>,
    asset_id: &str,
    default_rates: Vec<ComfortRate>,
) -> Vec<ComfortRate> {
    overrides.get(asset_id).cloned().unwrap_or(default_rates)
}

/// Where the curve in force for an asset comes from. Serialised lowercase: it is the `source`
/// field of `GET /assets/:id/comfort_curve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ComfortCurveSource {
    /// The user replaced the asset's built-in curve.
    Override,
    /// The asset's own built-in curve.
    Default,
}

/// Which source `effective_comfort_rates` takes the curve from: the one place that knows the
/// override-versus-default rule, so a caller that reports it never re-derives it.
pub fn comfort_curve_source(
    overrides: &HashMap<String, Vec<ComfortRate>>,
    asset_id: &str,
) -> ComfortCurveSource {
    if overrides.contains_key(asset_id) {
        ComfortCurveSource::Override
    } else {
        ComfortCurveSource::Default
    }
}

#[cfg(test)]
pub(crate) fn curve_point(fill: f64, bid: f64) -> ComfortRate {
    ComfortRate {
        fill,
        max_marginal_price_eur_kwh: bid,
        max_marginal_co2: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(fill: f64, bid: f64) -> ComfortRate {
        curve_point(fill, bid)
    }

    #[test]
    fn validate_curve_rejects_a_bid_that_rises_with_fill() {
        let err = validate_curve(&[pt(0.0, 0.10), pt(1.0, 0.40)])
            .expect_err("a rising bid must be refused");
        assert!(
            err.contains("point 1"),
            "the error must name the offending point: {err}"
        );
        assert!(
            err.contains("0.4") && err.contains("0.1"),
            "the error must show both bids so the user can see the rise: {err}"
        );
    }

    #[test]
    fn validate_curve_accepts_a_falling_or_flat_curve() {
        assert!(validate_curve(&[pt(0.0, 0.50), pt(1.0, 0.05)]).is_ok());
        assert!(validate_curve(&[pt(0.0, 0.20), pt(0.5, 0.20), pt(1.0, 0.20)]).is_ok());
        assert!(validate_curve(&[pt(0.3, 0.15)]).is_ok(), "single point");
    }

    #[test]
    fn validate_curve_rejects_a_rise_anywhere_in_the_curve() {
        // The rise is between the 2nd and 3rd points, not at the ends.
        let err = validate_curve(&[pt(0.0, 0.50), pt(0.4, 0.20), pt(0.7, 0.35), pt(1.0, 0.05)])
            .expect_err("a mid-curve rise must be refused");
        assert!(err.contains("point 2"), "{err}");
    }

    #[test]
    fn test_validate_curve_accepts_monotonic_bounded() {
        assert!(validate_curve(&[pt(0.5, 0.40), pt(0.8, 0.25), pt(1.0, 0.10)]).is_ok());
    }

    #[test]
    fn test_validate_curve_rejects_bad_input() {
        assert!(validate_curve(&[]).is_err(), "empty");
        assert!(
            validate_curve(&[pt(0.8, 0.2), pt(0.5, 0.3)]).is_err(),
            "non-monotonic fill"
        );
        assert!(
            validate_curve(&[pt(0.5, 0.2), pt(0.5, 0.3)]).is_err(),
            "duplicate fill"
        );
        assert!(validate_curve(&[pt(1.2, 0.2)]).is_err(), "fill > 1");
        assert!(validate_curve(&[pt(0.5, -0.1)]).is_err(), "negative bid");
        assert!(validate_curve(&[pt(0.5, 99.0)]).is_err(), "bid unbounded");
        assert!(validate_curve(&[pt(f64::NAN, 0.2)]).is_err(), "NaN");
    }

    #[test]
    fn test_effective_comfort_rates_prefers_override() {
        let mut overrides = HashMap::new();
        overrides.insert("ev".to_string(), vec![pt(0.9, 0.50)]);
        let default_rates = vec![pt(0.8, 0.30)];

        let eff = effective_comfort_rates(&overrides, "ev", default_rates.clone());
        assert!(
            (eff[0].max_marginal_price_eur_kwh - 0.50).abs() < 1e-9,
            "override wins"
        );

        let eff = effective_comfort_rates(&overrides, "heater", default_rates);
        assert!(
            (eff[0].max_marginal_price_eur_kwh - 0.30).abs() < 1e-9,
            "no override → built-in default"
        );
    }

    #[test]
    fn an_empty_override_is_still_the_override() {
        let mut overrides = HashMap::new();
        overrides.insert("ev".to_string(), Vec::new());
        assert!(effective_comfort_rates(&overrides, "ev", vec![pt(0.8, 0.30)]).is_empty());
    }

    #[test]
    fn comfort_curve_source_names_override_or_default() {
        let mut overrides = HashMap::new();
        overrides.insert("ev".to_string(), vec![pt(0.9, 0.50)]);
        assert_eq!(
            comfort_curve_source(&overrides, "ev"),
            ComfortCurveSource::Override
        );
        assert_eq!(
            comfort_curve_source(&overrides, "heater"),
            ComfortCurveSource::Default
        );
    }

    #[test]
    fn comfort_curve_cost_pairs_the_maximum_with_the_plans_spend() {
        use crate::services::test_support::plans::flat_plan;
        use chrono::TimeZone;
        let start = chrono::Utc.with_ymd_and_hms(2026, 4, 11, 6, 0, 0).unwrap();
        let mut plan = flat_plan(3600, 2, start);
        plan.slots[1].import_tariff_eur_kwh = 0.25;
        plan.slots[1]
            .allocations
            .push(crate::entities::plan::AssetAllocation {
                asset_id: "ev".into(),
                power_kw: 4.0,
                surplus_power_kw: 0.0,
                grid_power_kw: 4.0,
                marginal_value: 0.0,
                cost_eur: 0.0,
                co2_g: 0.0,
            });
        let bands = [EnergyBand {
            kwh: 10.0,
            eur_per_kwh: 0.30,
        }];
        let c = comfort_curve_cost(Some(&bands), Some(&plan), "ev");
        assert_eq!(c.max_cost_eur, Some(3.0));
        assert_eq!(c.energy_kwh, Some(10.0));
        assert!((c.planned_cost_eur - 1.0).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn comfort_curve_cost_is_zero_planned_without_an_allocation_and_has_no_maximum_without_bands() {
        let c = comfort_curve_cost(None, None, "heater");
        assert_eq!(
            c,
            ComfortCurveCost {
                max_cost_eur: None,
                energy_kwh: None,
                planned_cost_eur: 0.0
            }
        );
    }
}
