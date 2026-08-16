use serde::{Deserialize, Serialize};

/// Asset type classification (§1.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetType {
    Pv,              // photovoltaic producer
    Battery,         // bidirectional storage
    Ev,              // electric vehicle (consumer, storage-like)
    Heater,          // thermal consumer with storage characteristics
    HeatPump,        // thermal consumer with storage characteristics
    WashingMachine,  // batch consumer
    CookingStove,    // heuristic/uncontrollable consumer
    SiteResidual,    // virtual asset: unmodeled site consumption
    GenericConsumer, // fallback
    GenericProducer, // fallback
}

/// How adjustable an asset's power consumption/generation is (§1.2). Reported
/// per-asset by `assets::AssetCapability.adjustability` (BL-27) — moved here
/// from `entities/design_vocabulary.rs`'s dead-code quarantine once it became
/// live, referenced code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PowerAdjustability {
    None,           // observe only (e.g. cooking stove, fixed load)
    Recommendation, // VEN can suggest but not enforce (e.g. washing machine)
    OnOff,          // binary switching — equivalent to Stepped with [0, MaxPower]
    Stepped,        // discrete power levels (e.g. 0/3/6 kW pump, step-controlled charger)
    Stepless,       // continuously adjustable within [min_kw, max_kw]
    Croppable,      // can be curtailed downward only (e.g. PV — can't exceed natural output)
}

/// How to handle completion when the last DeadlineTier expires (§1.10).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompletionPolicy {
    /// Terminate immediately → PARTIAL_COMPLETED if FillPercentage < 1.0.
    Stop,
    /// Keep going, bidding at PostDeadlineComfortBid for priority.
    Continue,
}

/// What triggered a plan recomputation (§1.5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanTrigger {
    Periodic,         // regular planning cycle (every PlanTimeStep)
    RateChange,       // new PRICE/GHG/EXPORT_PRICE event from VTN
    CapacityChange,   // new capacity limit/reservation from VTN
    Alert,            // emergency/flex alert from VTN
    UserRequest,      // new or modified device session / user request
    AssetStateChange, // device connected/disconnected/failed
    /// The deviation arbiter's accumulated absorbed-kWh (per SoC-coupled
    /// asset) crossed its capacity-fraction threshold since the last plan
    /// adoption — an accumulator/hysteresis signal, deliberately not a
    /// raw-per-tick-deviation trigger (see `docs/reference/KEY_LEARNINGS.md`'s Deviation
    /// Absorber section on why the removed feature 017's raw-deviation trigger caused
    /// spurious replans). Rate-limited by a cooldown — see
    /// `AppState::last_residual_trigger_at`.
    ResidualThreshold,
}

/// One point on the comfort/value curve (§2.7).
/// MaxMarginalPrice is a priority bid, not the actual price paid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfortRate {
    pub fill: f64,               // 0.0..1.0 task completion fraction
    pub max_marginal_price: f64, // max €/kWh the user bids — determines priority
    pub max_marginal_co2: f64,   // max gCO2/kWh user accepts at this fill level
}

impl ComfortRate {
    /// Interpolate an arbitrary `ComfortRate` field (selected by `extract`) at a fill level.
    /// `rates` must be sorted non-decreasing by `fill` (guaranteed by
    /// `services/comfort.rs::validate_curve` for any persisted curve) and non-empty. Exact
    /// breakpoint queries return the stored value; mid-curve queries interpolate linearly
    /// between the two bracketing points; queries outside the stored range clamp to the
    /// nearest boundary breakpoint.
    fn interpolate_at_fill(rates: &[ComfortRate], fill: f64, extract: impl Fn(&ComfortRate) -> f64) -> f64 {
        if fill <= rates[0].fill {
            return extract(&rates[0]);
        }
        let last = rates.len() - 1;
        if fill >= rates[last].fill {
            return extract(&rates[last]);
        }
        let hi = rates.iter().position(|r| r.fill >= fill).unwrap();
        let lo = hi - 1;
        let (r_lo, r_hi) = (&rates[lo], &rates[hi]);
        if r_hi.fill == r_lo.fill {
            return extract(r_hi);
        }
        let t = (fill - r_lo.fill) / (r_hi.fill - r_lo.fill);
        extract(r_lo) + t * (extract(r_hi) - extract(r_lo))
    }

    /// Interpolate `max_marginal_price` at an arbitrary fill level. See `interpolate_at_fill`.
    pub fn value_at_fill(rates: &[ComfortRate], fill: f64) -> f64 {
        Self::interpolate_at_fill(rates, fill, |r| r.max_marginal_price)
    }

    /// Interpolate `max_marginal_co2` at an arbitrary fill level. See `interpolate_at_fill`.
    pub fn co2_value_at_fill(rates: &[ComfortRate], fill: f64) -> f64 {
        Self::interpolate_at_fill(rates, fill, |r| r.max_marginal_co2)
    }
}

#[cfg(test)]
mod comfort_rate_tests {
    use super::ComfortRate;

    fn curve() -> Vec<ComfortRate> {
        vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.30,
                max_marginal_co2: 300.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 50.0,
            },
        ]
    }

    #[test]
    fn value_at_fill_exact_breakpoint_returns_stored_price() {
        let rates = curve();
        assert_eq!(ComfortRate::value_at_fill(&rates, 0.0), 0.30);
        assert_eq!(ComfortRate::value_at_fill(&rates, 1.0), 0.10);
    }

    #[test]
    fn value_at_fill_mid_curve_interpolates_linearly() {
        let rates = curve();
        assert!((ComfortRate::value_at_fill(&rates, 0.5) - 0.20).abs() < 1e-9);
        assert!((ComfortRate::value_at_fill(&rates, 0.25) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn value_at_fill_out_of_range_clamps_to_nearest_breakpoint() {
        let rates = curve();
        assert_eq!(ComfortRate::value_at_fill(&rates, -0.5), 0.30);
        assert_eq!(ComfortRate::value_at_fill(&rates, 1.5), 0.10);
    }

    #[test]
    fn value_at_fill_three_point_curve_interpolates_within_bracket() {
        let rates = vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.30,
                max_marginal_co2: 0.0,
            },
            ComfortRate {
                fill: 0.5,
                max_marginal_price: 0.20,
                max_marginal_co2: 0.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 0.0,
            },
        ];
        assert!((ComfortRate::value_at_fill(&rates, 0.75) - 0.15).abs() < 1e-9);
    }

    // ── co2_value_at_fill: same interpolation, independent axis (BL-17 comfort bidding) ──

    #[test]
    fn co2_value_at_fill_exact_breakpoint_returns_stored_co2() {
        let rates = curve();
        assert_eq!(ComfortRate::co2_value_at_fill(&rates, 0.0), 300.0);
        assert_eq!(ComfortRate::co2_value_at_fill(&rates, 1.0), 50.0);
    }

    #[test]
    fn co2_value_at_fill_mid_curve_interpolates_linearly() {
        let rates = curve();
        assert!((ComfortRate::co2_value_at_fill(&rates, 0.5) - 175.0).abs() < 1e-9);
        assert!((ComfortRate::co2_value_at_fill(&rates, 0.25) - 237.5).abs() < 1e-9);
    }

    #[test]
    fn co2_value_at_fill_out_of_range_clamps_to_nearest_breakpoint() {
        let rates = curve();
        assert_eq!(ComfortRate::co2_value_at_fill(&rates, -0.5), 300.0);
        assert_eq!(ComfortRate::co2_value_at_fill(&rates, 1.5), 50.0);
    }

    #[test]
    fn co2_value_at_fill_three_point_curve_interpolates_within_bracket() {
        let rates = vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.30,
                max_marginal_co2: 300.0,
            },
            ComfortRate {
                fill: 0.5,
                max_marginal_price: 0.20,
                max_marginal_co2: 200.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 50.0,
            },
        ];
        assert!((ComfortRate::co2_value_at_fill(&rates, 0.75) - 125.0).abs() < 1e-9);
    }

    /// Price and CO2 axes are independent — a curve where they move in *opposite*
    /// directions across fill must interpolate each correctly on its own, proving
    /// `co2_value_at_fill` isn't accidentally reading the price field (or vice versa).
    #[test]
    fn price_and_co2_axes_interpolate_independently_on_a_diverging_curve() {
        let rates = vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.10, // price rises with fill...
                max_marginal_co2: 300.0,  // ...while CO2 bid falls with fill
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.50,
                max_marginal_co2: 20.0,
            },
        ];
        assert!((ComfortRate::value_at_fill(&rates, 0.5) - 0.30).abs() < 1e-9);
        assert!((ComfortRate::co2_value_at_fill(&rates, 0.5) - 160.0).abs() < 1e-9);
    }
}
