use serde::{Deserialize, Serialize};

/// Tracks the user-induced irradiance perturbation between ticks.
///
/// While the user drags the irradiance slider, the offset is set to
/// `slider_position − natural_irradiance`. After release the offset decays
/// exponentially (EMA with factor `pv_alpha`) until it reaches zero, at which
/// point the simulation resumes tracking the sin model with no lag.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PvSmoothingState {
    /// Current perturbation above (or below) the natural sin model. Zero = no override.
    pub irradiance_offset: f64,
}

impl PvSmoothingState {
    const PLAN_STEP_S: f64 = 300.0;

    /// Apply this tick's forced value (if any) or decay the offset, returning the
    /// resolved irradiance. `forced` is one-shot (auto-clears one tick after being
    /// posted, see `SimInjectState`), so the offset itself — not just `forced` — is
    /// what tracks "a manual perturbation is still in effect."
    pub fn update(
        &mut self,
        forced: Option<f64>,
        natural_irradiance: f64,
        dt_s: f64,
        pv_alpha: f64,
    ) -> f64 {
        if let Some(forced) = forced {
            self.irradiance_offset = forced - natural_irradiance;
        } else {
            let per_tick_factor = (1.0 - pv_alpha).powf(dt_s / Self::PLAN_STEP_S);
            self.irradiance_offset *= per_tick_factor;
            if self.irradiance_offset.abs() < 0.005 {
                self.irradiance_offset = 0.0;
            }
        }
        (natural_irradiance + self.irradiance_offset).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_locks_irradiance_to_forced_value() {
        let mut s = PvSmoothingState::default();
        let irradiance = s.update(Some(0.0), 0.8, 1.0, 0.1);
        assert_eq!(irradiance, 0.0);
    }

    #[test]
    fn update_decays_offset_toward_zero_when_released() {
        let mut s = PvSmoothingState {
            irradiance_offset: -0.8,
        };
        let irradiance = s.update(None, 0.8, 1.0, 0.5);
        assert!(irradiance > 0.0, "irradiance should have moved off zero");
        assert!(
            s.irradiance_offset.abs() < 0.8,
            "offset should have decayed"
        );
    }

    #[test]
    fn update_snaps_tiny_offset_to_exactly_zero() {
        let mut s = PvSmoothingState {
            irradiance_offset: 0.001,
        };
        s.update(None, 0.5, 1.0, 0.1);
        assert_eq!(s.irradiance_offset, 0.0);
    }
}
