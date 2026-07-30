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

/// Device health and communication status (§1.3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceResponsiveness {
    Responsive,   // device confirms setpoints within expected delay
    Degraded,     // device responds but outside expected parameters
    Unresponsive, // device not confirming setpoint changes
    Offline,      // device not communicating at all
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
    /// Interpolate `max_marginal_price` at an arbitrary fill level. `rates` must be sorted
    /// non-decreasing by `fill` (guaranteed by `services/comfort.rs::validate_curve` for any
    /// persisted curve) and non-empty. Exact breakpoint queries return the stored price;
    /// mid-curve queries interpolate linearly between the two bracketing points; queries
    /// outside the stored range clamp to the nearest boundary breakpoint.
    pub fn value_at_fill(rates: &[ComfortRate], fill: f64) -> f64 {
        if fill <= rates[0].fill {
            return rates[0].max_marginal_price;
        }
        let last = rates.len() - 1;
        if fill >= rates[last].fill {
            return rates[last].max_marginal_price;
        }
        let hi = rates.iter().position(|r| r.fill >= fill).unwrap();
        let lo = hi - 1;
        let (r_lo, r_hi) = (&rates[lo], &rates[hi]);
        if r_hi.fill == r_lo.fill {
            return r_hi.max_marginal_price;
        }
        let t = (fill - r_lo.fill) / (r_hi.fill - r_lo.fill);
        r_lo.max_marginal_price + t * (r_hi.max_marginal_price - r_lo.max_marginal_price)
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
                max_marginal_co2: 0.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 0.0,
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
}
