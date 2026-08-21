//! `CapacityCurve` — a closed-form, direction-specific power/duration/energy
//! forecast: "if the site committed now to sustained max import (or export),
//! how does the achievable power step down over elapsed time, and how much
//! energy is behind it." Distinct from `SiteFlexibilityForecastSlot`, whose
//! per-slot `up_kw`/`down_kw` are independent point-in-time counterfactuals
//! and must never be integrated over time — see
//! `controller::capacity_forecast` for the computation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which sustained commitment this curve models. Not mirror images of each
/// other — the contributing asset set and bounds differ per direction (see
/// `controller::capacity_forecast`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitmentDirection {
    /// Sustained maximum import (site draws more from the grid).
    Import,
    /// Sustained maximum export (site sends more to the grid).
    Export,
}

/// One point where the achievable power changes (an asset saturates, a
/// shiftable load is placed). `power_kw` holds from this step's `elapsed_s`
/// until the next step (or the horizon end) — a step function, not an
/// interpolated line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapacityCurveStep {
    /// Seconds elapsed since `CapacityCurve::start`.
    pub elapsed_s: i64,
    /// Achievable net grid power at and after this elapsed time (kW).
    pub power_kw: f64,
}

/// A closed-form power/duration/energy capacity forecast for one commitment
/// direction, anchored at `start`. Not cached or treated as binding across
/// ticks — recompute fresh whenever a fresh answer is needed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapacityCurve {
    pub direction: CommitmentDirection,
    pub start: DateTime<Utc>,
    /// Ordered by `elapsed_s`, ascending, starting at 0.
    pub steps: Vec<CapacityCurveStep>,
}

impl CapacityCurve {
    /// Cumulative energy (kWh) across the whole curve — trapezoidal-free
    /// since each step holds constant power until the next step (or the
    /// curve's last step, which contributes no further energy: there is no
    /// "next" elapsed time to bound its duration).
    pub fn energy_kwh_total(&self) -> f64 {
        self.steps
            .windows(2)
            .map(|w| {
                let dt_h = (w[1].elapsed_s - w[0].elapsed_s) as f64 / 3600.0;
                w[0].power_kw.abs() * dt_h
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_curve(steps: Vec<(i64, f64)>) -> CapacityCurve {
        CapacityCurve {
            direction: CommitmentDirection::Export,
            start: Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap(),
            steps: steps
                .into_iter()
                .map(|(elapsed_s, power_kw)| CapacityCurveStep {
                    elapsed_s,
                    power_kw,
                })
                .collect(),
        }
    }

    #[test]
    fn energy_kwh_total_integrates_step_function() {
        // 5 kW held for the first 1800s (0.5h), then 2 kW held until 3600s (0.5h more).
        let curve = make_curve(vec![(0, 5.0), (1800, 2.0), (3600, 0.0)]);
        let energy = curve.energy_kwh_total();
        assert!(
            (energy - 3.5).abs() < 1e-9,
            "expected 5.0*0.5 + 2.0*0.5 = 3.5 kWh, got {energy}"
        );
    }

    #[test]
    fn energy_kwh_total_zero_for_single_step() {
        // No "next" elapsed time to bound the last step's duration.
        let curve = make_curve(vec![(0, 5.0)]);
        assert_eq!(curve.energy_kwh_total(), 0.0);
    }

    #[test]
    fn energy_kwh_total_zero_for_empty_curve() {
        let curve = make_curve(vec![]);
        assert_eq!(curve.energy_kwh_total(), 0.0);
    }

    #[test]
    fn energy_kwh_total_uses_magnitude_for_import_direction() {
        // Import-direction power is expressed as positive kW by convention,
        // but the formula must not silently flip sign for either direction.
        let curve = CapacityCurve {
            direction: CommitmentDirection::Import,
            start: Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap(),
            steps: vec![
                CapacityCurveStep {
                    elapsed_s: 0,
                    power_kw: 4.0,
                },
                CapacityCurveStep {
                    elapsed_s: 900,
                    power_kw: 0.0,
                },
            ],
        };
        assert!((curve.energy_kwh_total() - 1.0).abs() < 1e-9);
    }
}
