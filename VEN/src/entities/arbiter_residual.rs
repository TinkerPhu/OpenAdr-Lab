//! Per-asset residual-escalation state for the deviation arbiter (see
//! `docs/architecture/VEN_ARCHITECTURE.md`'s Deviation Arbiter section).
//!
//! Deliberately a fresh, per-asset type rather than a repurposing of the
//! (confirmed dead) `entities::site_meter::DispatchState` — that type is
//! whole-site scalar, not per-asset, and doesn't fit the SoC-coupling
//! resource this tracks (battery/EV capacity, not aggregate site power).

use serde::{Deserialize, Serialize};

/// Tracks how much of an SoC-coupled asset's (battery/EV) capacity the
/// arbiter has consumed reactively since the last plan adoption, against how
/// much was available at that adoption. Used to decide when accumulated
/// greedy corrections risk undermining a later, more valuable use of the same
/// resource — the failure mode §5.5 describes pure greedy-by-marginal-cost as
/// unable to catch on its own.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub struct AssetResidual {
    /// kWh absorbed by the arbiter (either direction, magnitude only) since
    /// the last plan adoption.
    pub absorbed_kwh: f64,
    /// The asset's available charge/discharge capacity (kWh) as of that
    /// adoption — the baseline the fraction in `breach_fraction` is measured
    /// against.
    pub capacity_kwh_at_last_plan: f64,
}

impl AssetResidual {
    /// Fraction of the capacity baseline consumed so far. `0.0` if the
    /// baseline itself is zero or negative (nothing to protect).
    pub fn breach_fraction(&self) -> f64 {
        if self.capacity_kwh_at_last_plan <= 0.0 {
            0.0
        } else {
            self.absorbed_kwh / self.capacity_kwh_at_last_plan
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breach_fraction_zero_when_nothing_absorbed() {
        let r = AssetResidual {
            absorbed_kwh: 0.0,
            capacity_kwh_at_last_plan: 5.0,
        };
        assert_eq!(r.breach_fraction(), 0.0);
    }

    #[test]
    fn breach_fraction_scales_with_capacity() {
        let r = AssetResidual {
            absorbed_kwh: 1.0,
            capacity_kwh_at_last_plan: 5.0,
        };
        assert!((r.breach_fraction() - 0.2).abs() < 1e-9);
    }

    #[test]
    fn breach_fraction_zero_when_baseline_is_zero() {
        let r = AssetResidual {
            absorbed_kwh: 1.0,
            capacity_kwh_at_last_plan: 0.0,
        };
        assert_eq!(r.breach_fraction(), 0.0);
    }
}
