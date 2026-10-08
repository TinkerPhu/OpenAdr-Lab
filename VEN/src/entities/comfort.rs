//! Comfort curves: what a curve must look like to be accepted, and which curve planning uses.
//!
//! A user may replace an asset's built-in `default_comfort_rates()` with their own bid curve
//! (WP4.2, BL-19; the overrides live in `AppState` and persist through `SettingsPort`, see
//! `services::comfort`). The two rules here are pure domain logic, so they live with the type
//! they act on: the infrastructure that builds the planner's asset contexts
//! (`simulator::plan_context`) must be able to ask "which curve applies?" without reaching up
//! into the application ring (R-108).

use std::collections::HashMap;

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
            && r.max_marginal_price.is_finite()
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
        if !(0.0..=MAX_BID_EUR_KWH).contains(&r.max_marginal_price) {
            return Err(format!(
                "point {i}: max_marginal_price {} outside [0, {MAX_BID_EUR_KWH}] €/kWh",
                r.max_marginal_price
            ));
        }
        if r.max_marginal_co2 < 0.0 {
            return Err(format!("point {i}: max_marginal_co2 must be ≥ 0"));
        }
        if let Some(prev) = prev_price {
            if r.max_marginal_price > prev {
                return Err(format!(
                    "point {i}: max_marginal_price {} rises above the previous {prev} —                      bids must not increase with fill",
                    r.max_marginal_price
                ));
            }
        }
        prev_fill = r.fill;
        prev_price = Some(r.max_marginal_price);
    }
    Ok(())
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
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
        max_marginal_price: bid,
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
            (eff[0].max_marginal_price - 0.50).abs() < 1e-9,
            "override wins"
        );

        let eff = effective_comfort_rates(&overrides, "heater", default_rates);
        assert!(
            (eff[0].max_marginal_price - 0.30).abs() < 1e-9,
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
}
