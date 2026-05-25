use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Asset, AssetCapability, AssetState, ControlDescriptor, ControlKind};
use crate::common::{Interpolation, TimeSeries};
use crate::entities::asset_params::PvParams;

/// PV Inverter config. Generates power (export = negative).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvInverter {
    pub rated_kw: f64,
    /// Active export limit in kW (≤ 0); None = no curtailment limit.
    pub export_limit_kw: Option<f64>,
    /// [0.0, 1.0]; set each tick by sim (natural + offset, clamped). NOT from YAML.
    pub irradiance: f64,
    /// Current perturbation offset above/below the natural sin model. Decays toward zero
    /// each tick at rate `pv_alpha`. Set each tick from PvSmoothingState. NOT from YAML.
    pub irradiance_offset: f64,
    /// Per-tick decay factor for irradiance_offset (0–1). Set from pv_irradiance_alpha inject.
    /// NOT from YAML.
    pub pv_alpha: f64,
}

/// PV mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvState {
    /// Actual power last tick. Always ≤ 0 (PV only exports). Unit: kW.
    pub actual_power_kw: f64,
}

impl PvInverter {
    pub fn from_params(cfg: &PvParams) -> Self {
        Self {
            rated_kw: cfg.rated_kw,
            export_limit_kw: None,
            irradiance: 0.0,
            irradiance_offset: 0.0,
            pv_alpha: 0.1,
        }
    }

    pub fn initial_state(_cfg: &PvParams) -> PvState {
        PvState {
            actual_power_kw: 0.0,
        }
    }

    /// Pure physics step. Ignores setpoint (non-curtailable in Phase A).
    /// Reads `self.irradiance` (set by sim loop each tick before calling).
    pub fn step_inner(&self, _state: &PvState, _setpoint_kw: f64, _dt: Duration) -> (PvState, f64) {
        let raw_kw = -(self.rated_kw * self.irradiance); // negative = export
        let actual_kw = self
            .export_limit_kw
            .map(|lim| raw_kw.max(lim)) // lim ≤ 0; max() clamps to less export
            .unwrap_or(raw_kw);
        (
            PvState {
                actual_power_kw: actual_kw,
            },
            actual_kw,
        )
    }

    /// Non-curtailable point-range capability.
    pub fn capability_inner(&self, state: &PvState) -> AssetCapability {
        AssetCapability {
            max_export_kw: state.actual_power_kw, // e.g. -2.0
            max_import_kw: state.actual_power_kw, // same — is_fixed() = true
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        f64::MAX // no export limit by default
    }

    pub fn state_values(&self, _state: &PvState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("irradiance".into(), self.irradiance);
        m.insert("rated_kw".into(), self.rated_kw);
        m.insert("irradiance_offset".into(), self.irradiance_offset);
        m.insert("pv_alpha".into(), self.pv_alpha);
        if let Some(lim) = self.export_limit_kw {
            m.insert("export_limit_kw".into(), lim);
        }
        m
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![
            ControlDescriptor {
                key: "pv_irradiance".into(),
                label: "Irradiance Override".into(),
                kind: ControlKind::Slider,
                min: Some(0.0),
                max: Some(1.0),
                unit: "%".into(),
                display_scale: Some(100.0),
            },
            ControlDescriptor {
                key: "pv_irradiance_alpha".into(),
                label: "Blend-back Speed".into(),
                kind: ControlKind::Slider,
                min: Some(0.01),
                max: Some(1.0),
                unit: "".into(),
                display_scale: None,
            },
        ]
    }

    pub fn reset(&self, _state: &mut PvState, _values: HashMap<String, f64>) {}

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("rated_kw") {
            self.rated_kw = v.max(0.0);
        }
    }

    pub fn forecast(&self, _state: &PvState, timespan: Duration) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Linear);
        }
        let now = Utc::now();
        let end = now + timespan;
        let mut samples: Vec<(DateTime<Utc>, f64)> = Vec::new();

        let mut t = now;
        while t < end {
            samples.push((t, self.irradiance_at(t)));
            t += Duration::seconds(60);
        }
        samples.push((end, self.irradiance_at(end)));

        if samples.len() >= 2 {
            let n = samples.len();
            if (samples[n - 2].0 - samples[n - 1].0).num_seconds().abs() < 1 {
                samples.truncate(n - 1);
                samples.push((end, self.irradiance_at(end)));
            }
        }

        TimeSeries {
            samples,
            interpolation: Interpolation::Linear,
        }
    }

    /// Forecast generation magnitude (kW, positive) at future time `t`.
    ///
    /// `seconds_ahead` — real seconds into the future.
    /// `tau_s`         — continuous time constant (seconds) for offset decay,
    ///                   derived by the caller as `-step_s / ln(1 − pv_alpha)`.
    ///                   `f64::INFINITY` = no decay; `0.0` = instant decay.
    ///
    /// When offset == 0: pure sin model.
    /// When offset != 0: sin + `irradiance_offset × exp(−seconds_ahead / tau_s)`.
    pub fn forecast_kw_at(&self, t: DateTime<Utc>, seconds_ahead: f64, tau_s: f64) -> f64 {
        let natural = Self::natural_irradiance_at(t);
        let decay_factor = if tau_s > 0.0 {
            (-seconds_ahead / tau_s).exp()
        } else {
            // instant decay: full offset at t=0, zero after
            if seconds_ahead <= 0.0 {
                1.0
            } else {
                0.0
            }
        };
        let decayed = self.irradiance_offset * decay_factor;
        (natural + decayed).clamp(0.0, 1.0) * self.rated_kw
    }

    /// Natural sin-model irradiance [0,1] at time `ts`, without any user offset.
    pub fn natural_irradiance_at(ts: DateTime<Utc>) -> f64 {
        use chrono::Timelike;
        let hour = ts.hour() as f64 + ts.minute() as f64 / 60.0;
        if (6.0..=18.0).contains(&hour) {
            let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
            angle.sin().max(0.0)
        } else {
            0.0
        }
    }

    /// Power output from the sin model at `ts` (kW, negative = export).
    /// Used by `forecast()`. Does NOT include the live irradiance_offset.
    fn irradiance_at(&self, ts: DateTime<Utc>) -> f64 {
        let natural_kw = self.rated_kw * Self::natural_irradiance_at(ts);
        let limited_kw = match self.export_limit_kw {
            Some(limit) => natural_kw.min(limit.abs()),
            None => natural_kw,
        };
        -limited_kw
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        vec![
            crate::entities::asset::ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.0,
                max_marginal_co2: 0.0,
            },
            crate::entities::asset::ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.0,
                max_marginal_co2: 0.0,
            },
        ]
    }

    pub fn default_completion_policy(&self) -> crate::entities::asset::CompletionPolicy {
        crate::entities::asset::CompletionPolicy::Stop
    }

    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        None
    }
}

impl Asset for PvInverter {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::Pv(s) = state else {
            unreachable!("PvInverter/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::Pv(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::Pv(s) = state else {
            unreachable!()
        };
        self.capability_inner(s)
    }

    /// Forecast trajectory for the planner: sin model + decaying perturbation offset.
    ///
    /// For each future slot at `now + i×resolution`:
    ///   irradiance = clamp(natural_sin(t) + offset×(1−α)^seconds_ahead, 0, 1)
    ///   power_kw   = −(irradiance × rated_kw)
    ///
    /// When offset = 0 (no active inject): pure sin model.
    /// While user drags slider: offset is non-zero → sin curve shifted by perturbation.
    /// After release: offset decays with α in the live tick; by the time it reaches the
    /// planner (every 20 s) the perturbation has already decayed in proportion.
    fn capability_trajectory(
        &self,
        _initial: &AssetState,
        duration: Duration,
        resolution: Duration,
    ) -> Vec<(DateTime<Utc>, AssetCapability)> {
        let now = Utc::now();
        let n = (duration.num_seconds() / resolution.num_seconds().max(1)) as usize;
        let res_s = resolution.num_seconds() as f64;
        // pv_alpha is "fraction removed per plan step (300 s)".
        const PLAN_STEP_S: f64 = 300.0;
        let mut result = Vec::with_capacity(n);
        for i in 1..=n {
            let t = now + resolution * i as i32;
            let seconds_ahead = res_s * i as f64;
            let natural = Self::natural_irradiance_at(t);
            let decayed_offset =
                self.irradiance_offset * (1.0 - self.pv_alpha).powf(seconds_ahead / PLAN_STEP_S);
            let irradiance = (natural + decayed_offset).clamp(0.0, 1.0);
            let power_kw = -(irradiance * self.rated_kw);
            result.push((
                t,
                AssetCapability {
                    max_export_kw: power_kw,
                    max_import_kw: power_kw,
                },
            ));
        }
        result
    }
}

// ── PV MILP plugin types ──────────────────────────────────────────────────────

/// Pre-computed PV contribution for one planning cycle.
/// PV has no LP decision variables — it contributes a constant forecast to the
/// power balance at each slot. The planner reads `p_pv_kw[t]` directly.
#[derive(Debug, Clone)]
pub struct PvMilpContext {
    /// Forecast generation [kW] per slot (positive = generating, sign: supply to bus).
    pub p_pv_kw: Vec<f64>,
}

impl PvInverter {
    /// Build the PV MILP context: forecast `n` slots starting at `now`,
    /// each of width `step_s` seconds, using the sin model + decaying offset.
    pub fn build_milp_context(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        n: usize,
        step_s: i64,
    ) -> PvMilpContext {
        let dt_h = step_s as f64 / 3_600.0;
        let _ = dt_h; // retained for caller symmetry; value used indirectly via slot_t
        let p_pv_kw: Vec<f64> = (0..n)
            .map(|t| {
                let slot_t = now + chrono::Duration::seconds(step_s * t as i64);
                let natural = Self::natural_irradiance_at(slot_t);
                let decayed_offset = self.irradiance_offset * (1.0 - self.pv_alpha).powf(t as f64);
                (natural + decayed_offset).clamp(0.0, 1.0) * self.rated_kw
            })
            .collect();
        PvMilpContext { p_pv_kw }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_pv(rated_kw: f64) -> (PvInverter, PvState) {
        (
            PvInverter {
                rated_kw,
                irradiance: 0.0,
                irradiance_offset: 0.0,
                pv_alpha: 0.1,
                export_limit_kw: None,
            },
            PvState {
                actual_power_kw: 0.0,
            },
        )
    }

    #[test]
    fn forecast_zero_timespan_returns_empty() {
        let (pv, state) = make_pv(5.0);
        let series = pv.forecast(&state, Duration::zero());
        assert!(
            series.samples.is_empty(),
            "Zero timespan must return empty series"
        );
    }

    #[test]
    fn forecast_has_boundary_point_at_end() {
        let (pv, state) = make_pv(5.0);
        let timespan = Duration::seconds(300);
        let before = Utc::now();
        let series = pv.forecast(&state, timespan);
        let after = Utc::now();
        assert!(
            !series.samples.is_empty(),
            "Non-zero timespan must produce samples"
        );
        let last_ts = series.samples.last().unwrap().0;
        assert!(
            last_ts >= before + timespan && last_ts <= after + timespan,
            "Boundary point must be at now+timespan"
        );
    }

    #[test]
    fn forecast_samples_ascending() {
        let (pv, state) = make_pv(5.0);
        let series = pv.forecast(&state, Duration::seconds(120));
        let timestamps: Vec<_> = series.samples.iter().map(|(t, _)| t).collect();
        for i in 1..timestamps.len() {
            assert!(
                timestamps[i] > timestamps[i - 1],
                "Timestamps must be strictly ascending"
            );
        }
    }

    #[test]
    fn forecast_rated_zero_returns_all_zero() {
        let (pv, state) = make_pv(0.0);
        let series = pv.forecast(&state, Duration::seconds(300));
        for (_, v) in &series.samples {
            assert_eq!(*v, 0.0, "Zero-rated PV must produce all-zero series");
        }
    }

    #[test]
    fn pv_params_forecast_kw_noon() {
        let ts = Utc.with_ymd_and_hms(2026, 4, 11, 12, 0, 0).unwrap();
        assert!(PvParams::default().forecast_kw(ts) > 0.0);
    }

    #[test]
    fn pv_params_forecast_kw_midnight() {
        let ts = Utc.with_ymd_and_hms(2026, 4, 11, 0, 0, 0).unwrap();
        assert_eq!(PvParams::default().forecast_kw(ts), 0.0);
    }

    #[test]
    fn step_generates_at_noon_irradiance() {
        let (mut pv, state) = make_pv(10.0);
        pv.irradiance = 1.0; // noon
        let (new_state, power) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power + 10.0).abs() < 0.01,
            "Should export ~10 kW at full irradiance"
        );
        assert!((new_state.actual_power_kw + 10.0).abs() < 0.01);
    }

    // ── capability_trajectory tests ──────────────────────────────────────────

    #[test]
    fn capability_trajectory_uses_sin_model_not_flat_irradiance() {
        // self.irradiance = 0.5 (flat), offset = 0 → forecast must follow sin model, not flat.
        let pv = PvInverter {
            rated_kw: 10.0,
            irradiance: 0.5,
            irradiance_offset: 0.0,
            pv_alpha: 0.1,
            export_limit_kw: None,
        };
        let state = AssetState::Pv(PvState {
            actual_power_kw: -5.0,
        });

        let traj = pv.capability_trajectory(&state, Duration::hours(24), Duration::hours(1));
        assert_eq!(traj.len(), 24);

        // All values ≤ 0 (export only).
        for (_, cap) in &traj {
            assert!(
                cap.max_export_kw <= 1e-9,
                "PV trajectory must be non-positive"
            );
        }

        // Spans at least one daytime and one night slot → not flat.
        let non_zero = traj
            .iter()
            .filter(|(_, c)| c.max_export_kw.abs() > 1e-6)
            .count();
        let zero = traj
            .iter()
            .filter(|(_, c)| c.max_export_kw.abs() <= 1e-6)
            .count();
        assert!(
            non_zero > 0,
            "24-h trajectory must include daytime generation"
        );
        assert!(zero > 0, "24-h trajectory must include night zeros");

        // Not all identical to −5.0 (flat at self.irradiance=0.5 × 10 kW).
        let all_flat = traj
            .iter()
            .all(|(_, c)| (c.max_export_kw + 5.0).abs() < 1e-6);
        assert!(
            !all_flat,
            "forecast must follow sin model, not flat self.irradiance"
        );
    }

    #[test]
    fn capability_trajectory_offset_shifts_sin_curve() {
        // With a positive irradiance_offset = +0.2 and slow alpha, the forecast
        // at slot 1 must be visibly higher than the pure sin model.
        let pv = PvInverter {
            rated_kw: 10.0,
            irradiance: 0.0,
            irradiance_offset: 0.3,
            pv_alpha: 0.0, // alpha=0 → offset never decays → full offset at every slot
            export_limit_kw: None,
        };
        let state = AssetState::Pv(PvState {
            actual_power_kw: 0.0,
        });

        // Use a 1-hour resolution so slot timestamps are well into daytime when run near noon.
        let traj = pv.capability_trajectory(&state, Duration::hours(4), Duration::hours(1));
        assert_eq!(traj.len(), 4);

        for (t, cap) in &traj {
            let natural = PvInverter::natural_irradiance_at(*t);
            let expected = -((natural + 0.3).clamp(0.0, 1.0) * 10.0);
            assert!(
                (cap.max_export_kw - expected).abs() < 1e-9,
                "at {t}: expected {expected:.4} (sin+offset), got {:.4}",
                cap.max_export_kw
            );
        }
    }

    #[test]
    fn capability_trajectory_offset_decays_across_slots() {
        // With alpha=1.0, offset halves per second. At res=1s, slot 1 (1s ahead):
        // decayed_offset = 0.4 × (1−1.0)^1 = 0.0 → pure sin model.
        let pv = PvInverter {
            rated_kw: 10.0,
            irradiance: 0.0,
            irradiance_offset: 0.4,
            pv_alpha: 1.0, // full decay after 1 tick
            export_limit_kw: None,
        };
        let state = AssetState::Pv(PvState {
            actual_power_kw: 0.0,
        });
        let traj = pv.capability_trajectory(&state, Duration::seconds(3), Duration::seconds(1));
        // Slot 1 (1 s ahead): decayed_offset = 0.4 × 0^1 = 0 → equals sin model
        for (t, cap) in &traj {
            let natural = PvInverter::natural_irradiance_at(*t);
            let expected = -(natural * 10.0);
            assert!(
                (cap.max_export_kw - expected).abs() < 1e-9,
                "at {t}: with alpha=1.0 offset must be fully decayed, got {:.4}",
                cap.max_export_kw
            );
        }
    }

    #[test]
    fn capability_trajectory_offset_decays_per_step_not_per_second() {
        // Regression guard: with alpha=0.1 and resolution=300s (1 plan step),
        // slot 1 (300s ahead) must use exponent=300/300=1, NOT raw seconds=300.
        // Correct: 0.4 × 0.9^1 = 0.36  →  offset still visible
        // Buggy:   0.4 × 0.9^300 ≈ 0   →  offset gone after slot 0
        let pv = PvInverter {
            rated_kw: 10.0,
            irradiance: 0.0,
            irradiance_offset: 0.4,
            pv_alpha: 0.1,
            export_limit_kw: None,
        };
        let state = AssetState::Pv(PvState {
            actual_power_kw: 0.0,
        });
        let traj = pv.capability_trajectory(
            &state,
            Duration::seconds(900), // 3 plan steps
            Duration::seconds(300), // 1 plan step per slot
        );
        assert_eq!(traj.len(), 3);
        // With correct per-STEP decay: slot 1 exponent = 300/300 = 1  → offset = 0.4 × 0.9 = 0.36
        // With buggy per-second decay: slot 1 exponent = 300          → offset = 0.4 × 0.9^300 ≈ 0
        // Verify the implementation matches the correct formula exactly.
        let (t1, cap1) = &traj[0];
        let natural = PvInverter::natural_irradiance_at(*t1);
        let correct_offset = 0.4_f64 * (1.0_f64 - 0.1_f64).powf(1.0); // per-step exponent = 1
        let correct_kw = -((natural + correct_offset).clamp(0.0, 1.0) * 10.0);
        assert!(
            (cap1.max_export_kw - correct_kw).abs() < 0.01,
            "slot 1 must match per-step decay formula: expected {:.4}, got {:.4}",
            correct_kw,
            cap1.max_export_kw
        );
        // When not saturated (natural + decayed_offset < 1.0, i.e. natural < 0.64),
        // the offset is fully visible and must contribute >1 kW over natural-only.
        // Skip when saturated: the offset is clipped and the visible difference is smaller.
        let natural_only_kw = -(natural * 10.0);
        let decayed_offset_slot1 = 0.4_f64 * (1.0_f64 - 0.1_f64); // exponent=1 per step
        if natural + decayed_offset_slot1 < 1.0 {
            assert!(
                cap1.max_export_kw < natural_only_kw - 1.0,
                "slot 1 must export >1 kW more than natural-only when not saturated: \
                 got {:.4}, natural-only {:.4}",
                cap1.max_export_kw,
                natural_only_kw
            );
        }
    }

    #[test]
    fn capability_trajectory_respects_rated_kw() {
        let pv = PvInverter {
            rated_kw: 8.0,
            irradiance: 0.0,
            irradiance_offset: 0.0,
            pv_alpha: 0.1,
            export_limit_kw: None,
        };
        let state = AssetState::Pv(PvState {
            actual_power_kw: 0.0,
        });
        let traj = pv.capability_trajectory(&state, Duration::hours(24), Duration::hours(1));
        for (_, cap) in &traj {
            assert!(
                cap.max_export_kw >= -8.0 - 1e-9,
                "export must not exceed rated_kw=8.0, got {}",
                cap.max_export_kw
            );
        }
    }

    /// Verifies that irradiance_offset and pv_alpha stored on PvInverter are
    /// picked up by capability_trajectory. When alpha=0 the offset never decays,
    /// so all slots shift by the full offset.
    #[test]
    fn capability_trajectory_reads_offset_from_self() {
        let mut pv = PvInverter {
            rated_kw: 10.0,
            irradiance: 1.0, // flat — must NOT be used
            irradiance_offset: 0.2,
            pv_alpha: 0.0, // no decay → offset constant at 0.2 everywhere
            export_limit_kw: None,
        };
        // Verify offset is read from self.irradiance_offset, not from self.irradiance.
        // With pv_alpha=0 and offset=0.2, each slot must equal sin(t)+0.2 (clamped).
        let state = AssetState::Pv(PvState {
            actual_power_kw: 0.0,
        });
        let traj = pv.capability_trajectory(&state, Duration::hours(4), Duration::hours(1));

        for (t, cap) in &traj {
            let natural = PvInverter::natural_irradiance_at(*t);
            let expected_irr = (natural + 0.2).clamp(0.0, 1.0);
            let expected_kw = -(expected_irr * 10.0);
            assert!(
                (cap.max_export_kw - expected_kw).abs() < 1e-9,
                "at {t}: expected {expected_kw:.4} (sin+0.2), got {:.4}",
                cap.max_export_kw
            );
        }

        // Changing offset on self must change the trajectory output.
        pv.irradiance_offset = -0.5;
        let traj2 = pv.capability_trajectory(&state, Duration::hours(4), Duration::hours(1));
        let same = traj
            .iter()
            .zip(traj2.iter())
            .all(|((_, a), (_, b))| (a.max_export_kw - b.max_export_kw).abs() < 1e-9);
        assert!(
            !same,
            "changing irradiance_offset must change trajectory output"
        );
    }

    #[test]
    fn capability_trajectory_ascending_timestamps() {
        let (pv, state_inner) = make_pv(5.0);
        let state = AssetState::Pv(state_inner);
        let traj = pv.capability_trajectory(&state, Duration::hours(6), Duration::hours(1));
        assert_eq!(traj.len(), 6);
        for i in 1..traj.len() {
            assert!(
                traj[i].0 > traj[i - 1].0,
                "trajectory timestamps must be strictly ascending"
            );
        }
    }
}
