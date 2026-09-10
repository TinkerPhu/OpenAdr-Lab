use serde::{Deserialize, Serialize};

/// Tracks the user-induced irradiance perturbation between ticks.
///
/// While the user drags the irradiance slider, the offset is set to
/// `slider_position − natural_irradiance`. After release the offset decays
/// exponentially toward zero with time constant `tau_s`, at which point the
/// simulation resumes tracking the sin model with no lag.
///
/// `asset-competence-assurance` (`unified-capacity-envelope-engine`'s follow-up,
/// `pv-competence-consolidation`): this used to be parameterized as a
/// `(pv_alpha, reference_step_s)` pair — a decay-per-step fraction plus a
/// separately-sourced reference step duration — which is mathematically the
/// same curve as `A·e^(-t/tau)` (`tau = -reference_step_s / ln(1 - pv_alpha)`),
/// just spread across two numbers instead of the one that actually matters.
/// That redundant second parameter is exactly what let a real, previously-latent
/// bug exist: this module's own live decay hardcoded its reference step at
/// `300.0`, while `entities::solar::pv_ceiling_kw`'s forward-projected version
/// read a *different*, planner-config-sourced `zone_a_step_s` for the same role
/// — the two only coincided because `zone_a_step_s`'s fallback default (300)
/// happened to match. Storing `tau_s` directly removes the redundant degree of
/// freedom structurally, not just at that one call site.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PvSmoothingState {
    /// Current perturbation above (or below) the natural sin model. Zero = no override.
    pub irradiance_offset: f64,
}

impl PvSmoothingState {
    /// Apply this tick's forced value (if any) or decay the offset, returning the
    /// resolved irradiance. `forced` is one-shot (auto-clears one tick after being
    /// posted, see `SimInjectState`), so the offset itself — not just `forced` — is
    /// what tracks "a manual perturbation is still in effect."
    pub fn update(
        &mut self,
        forced: Option<f64>,
        natural_irradiance: f64,
        dt_s: f64,
        tau_s: f64,
    ) -> f64 {
        self.irradiance_offset = self.next_offset(forced, natural_irradiance, dt_s, tau_s);
        (natural_irradiance + self.irradiance_offset).clamp(0.0, 1.0)
    }

    /// Pure counterpart of `update`: what the offset *would* become this tick,
    /// without writing it back. `SimState::peek_pv_kw` previews a tick without
    /// advancing state, so it needs this arithmetic while `update` needs the
    /// same arithmetic plus the write — sharing it here is what keeps the live
    /// tick and the preview from drifting.
    pub fn next_offset(
        &self,
        forced: Option<f64>,
        natural_irradiance: f64,
        dt_s: f64,
        tau_s: f64,
    ) -> f64 {
        if let Some(forced) = forced {
            return forced - natural_irradiance;
        }
        let decayed = self.decayed_offset_after(dt_s, tau_s);
        if decayed.abs() < 0.005 {
            0.0
        } else {
            decayed
        }
    }

    /// The one function computing this decay, used both by the live per-tick
    /// update above (`elapsed_s = dt_s`, one tick) and by
    /// `assets::pv::PvInverter::max_effort_schedule`/`forecast()` (`elapsed_s`
    /// = an arbitrary future offset) — so a forward projection can never use a
    /// different reference time base than the live tick that produces the
    /// offset being projected. Standard single-pole exponential decay,
    /// `A·e^(-t/tau)`; no floor/snap-to-zero here (callers needing that, like
    /// `next_offset`, apply it themselves) since a forward-projecting caller
    /// may legitimately want the raw, un-snapped value at a specific instant.
    pub fn decayed_offset_after(&self, elapsed_s: f64, tau_s: f64) -> f64 {
        decayed_offset(self.irradiance_offset, elapsed_s, tau_s)
    }
}

/// Free-function form of the same formula — lets a caller that only has the raw
/// `offset` value (not a whole `PvSmoothingState`) reuse it without constructing
/// one just to call a method (`assets::pv::PvInverter::uncurtailed_power_kw_at`).
/// `PvSmoothingState::decayed_offset_after` is defined in terms of this, so the
/// two can never independently diverge — same pattern `asset_max_power`/
/// `asset_max_power_series` already established.
pub fn decayed_offset(offset: f64, elapsed_s: f64, tau_s: f64) -> f64 {
    offset * (-elapsed_s / tau_s).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values for tests migrated from the old (pv_alpha, T=300s) encoding,
    // converted via tau = -T / ln(1 - pv_alpha) (pv-competence-consolidation D7) so the
    // migrated tests exercise roughly the same real-world decay speed as before -- these
    // three are qualitative-only tests (direction/snap-to-zero), so an approximate
    // conversion is fine; the precision check below computes tau exactly instead.
    const TAU_FROM_ALPHA_0_1_APPROX: f64 = 2847.4; // pv_alpha=0.1, T=300
    const TAU_FROM_ALPHA_0_5_APPROX: f64 = 432.8; // pv_alpha=0.5, T=300

    #[test]
    fn update_locks_irradiance_to_forced_value() {
        let mut s = PvSmoothingState::default();
        let irradiance = s.update(Some(0.0), 0.8, 1.0, TAU_FROM_ALPHA_0_1_APPROX);
        assert_eq!(irradiance, 0.0);
    }

    #[test]
    fn update_decays_offset_toward_zero_when_released() {
        let mut s = PvSmoothingState {
            irradiance_offset: -0.8,
        };
        let irradiance = s.update(None, 0.8, 1.0, TAU_FROM_ALPHA_0_5_APPROX);
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
        s.update(None, 0.5, 1.0, TAU_FROM_ALPHA_0_1_APPROX);
        assert_eq!(s.irradiance_offset, 0.0);
    }

    // ── decayed_offset_after: the one shared decay function (pv-competence-consolidation D7) ──

    #[test]
    fn decayed_offset_after_matches_the_old_two_parameter_formula_at_one_tick() {
        // Numeric-equivalence check: decayed_offset_after(dt_s, tau) must reproduce
        // exactly what the old `(1 - pv_alpha).powf(dt_s / 300.0)` formula gave for
        // pv_alpha=0.1, dt_s=1.0 — confirming the reparametrization is mathematically
        // identical, not just structurally similar.
        let s = PvSmoothingState {
            irradiance_offset: 1.0,
        };
        let old_formula = (1.0_f64 - 0.1).powf(1.0 / 300.0);
        let exact_tau = -300.0_f64 / (1.0_f64 - 0.1).ln();
        let new_formula = s.decayed_offset_after(1.0, exact_tau);
        assert!(
            (old_formula - new_formula).abs() < 1e-9,
            "old={old_formula}, new={new_formula}"
        );
    }

    #[test]
    fn decayed_offset_after_at_elapsed_zero_returns_the_full_offset() {
        let s = PvSmoothingState {
            irradiance_offset: 0.42,
        };
        assert!((s.decayed_offset_after(0.0, 2848.5) - 0.42).abs() < 1e-12);
    }

    #[test]
    fn decayed_offset_after_at_one_tau_decays_to_1_over_e() {
        let s = PvSmoothingState {
            irradiance_offset: 1.0,
        };
        let result = s.decayed_offset_after(2848.5, 2848.5);
        assert!(
            (result - std::f64::consts::E.recip()).abs() < 1e-9,
            "expected 1/e, got {result}"
        );
    }

    #[test]
    fn decayed_offset_after_projects_further_than_one_tick() {
        // The whole point of this function: it must correctly answer for an
        // elapsed_s far larger than any single live tick's dt_s (e.g. a
        // sustained-commitment sweep asking about 30 minutes from now).
        let s = PvSmoothingState {
            irradiance_offset: 1.0,
        };
        let after_5_min = s.decayed_offset_after(300.0, 2848.5);
        let after_30_min = s.decayed_offset_after(1800.0, 2848.5);
        assert!(
            after_30_min < after_5_min,
            "further in the future must have decayed more: {after_30_min} vs {after_5_min}"
        );
        assert!(after_30_min > 0.0, "must not overshoot past zero");
    }
}
