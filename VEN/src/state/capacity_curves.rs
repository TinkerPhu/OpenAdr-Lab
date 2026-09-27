//! Sustained-commitment capacity curves (import, export), re-derived fresh
//! every dispatcher tick — same "plain replace-on-tick cache" shape as
//! `site_headroom_forecast.rs`, and for the same reason: each tick already
//! produces the whole curve, not one more sample to retain.

use crate::entities::capacity_curve::CapacityCurve;

use super::AppState;

impl AppState {
    /// `(import_curve, export_curve)`. `None` before the first tick.
    pub async fn capacity_curves(&self) -> Option<(CapacityCurve, CapacityCurve)> {
        self.hems.read().await.capacity_curves.clone()
    }

    pub async fn set_capacity_curves(&self, curves: (CapacityCurve, CapacityCurve)) {
        self.hems.write().await.capacity_curves = Some(curves);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::capacity_curve::{CapacityCurveStep, CommitmentDirection};
    use chrono::Utc;

    fn curve(direction: CommitmentDirection, power_kw: f64) -> CapacityCurve {
        CapacityCurve {
            direction,
            start: Utc::now(),
            steps: vec![CapacityCurveStep {
                elapsed_s: 0,
                power_kw,
            }],
        }
    }

    #[tokio::test]
    async fn none_before_the_first_tick() {
        let state = AppState::new();
        assert!(state.capacity_curves().await.is_none());
    }

    #[tokio::test]
    async fn set_replaces_the_whole_pair() {
        let state = AppState::new();
        state
            .set_capacity_curves((
                curve(CommitmentDirection::Import, 1.0),
                curve(CommitmentDirection::Export, 2.0),
            ))
            .await;
        state
            .set_capacity_curves((
                curve(CommitmentDirection::Import, 3.0),
                curve(CommitmentDirection::Export, 4.0),
            ))
            .await;
        let (import_curve, export_curve) = state.capacity_curves().await.unwrap();
        assert_eq!(import_curve.steps[0].power_kw, 3.0);
        assert_eq!(export_curve.steps[0].power_kw, 4.0);
    }
}
