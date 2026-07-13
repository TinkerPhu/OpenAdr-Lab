use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Asset, AssetCapability, AssetState, ControlDescriptor, ControlKind};
use crate::common::{Interpolation, TimeSeries};
use crate::entities::asset_params::BaseLoadParams;

/// One named appliance's daily draw: a Gaussian-shaped power bump centered on
/// `center_hour`, with day-to-day jitter in both timing and magnitude so the
/// pattern isn't a perfectly flat, trivially-learnable signal.
struct AppliancePattern {
    /// Distinguishes this appliance's jitter seed from the others' (so their
    /// day-to-day draws don't move in lockstep).
    seed_tag: u64,
    center_hour: f64,
    amplitude_kw: f64,
    sigma_h: f64,
}

/// Coffee (~8h), lunch + dinner cooking (~12h, ~18h), evening TV/lights
/// (~20h, broad plateau). Amplitudes are typical single-household appliance
/// draws, not calibrated to any specific device.
const APPLIANCE_PATTERNS: [AppliancePattern; 4] = [
    AppliancePattern {
        seed_tag: 1,
        center_hour: 8.0,
        amplitude_kw: 1.2,
        sigma_h: 0.4,
    },
    AppliancePattern {
        seed_tag: 2,
        center_hour: 12.0,
        amplitude_kw: 2.0,
        sigma_h: 0.5,
    },
    AppliancePattern {
        seed_tag: 3,
        center_hour: 18.0,
        amplitude_kw: 2.5,
        sigma_h: 0.6,
    },
    AppliancePattern {
        seed_tag: 4,
        center_hour: 20.0,
        amplitude_kw: 0.4,
        sigma_h: 1.5,
    },
];

/// Base load config. Fixed background consumption (positive = import).
/// Non-flexible — planner never schedules allocations for this asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseLoad {
    pub baseline_kw: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub baseline_kw_profile: f64,
}

/// BaseLoad mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseLoadState {
    /// Actual power last tick. Always ≥ 0 (consumption only). Unit: kW.
    pub actual_power_kw: f64,
}

impl BaseLoad {
    pub fn from_params(cfg: &BaseLoadParams) -> Self {
        Self {
            baseline_kw: cfg.baseline_kw,
            baseline_kw_profile: cfg.baseline_kw,
        }
    }

    /// Deterministic simulated appliance noise \[kW\] for `now`: additive
    /// bumps around typical coffee/cooking/TV-and-lights times, jittered
    /// per calendar day (seeded by day-ordinal + appliance tag) so the same
    /// simulated day always reproduces the same pattern — reproducible in
    /// tests — while still varying non-trivially day to day. Exists so
    /// BL-14's learned heuristics (Phase 5 WP5.2) have a real daily/weekly
    /// shape to fit instead of a perfectly flat baseline.
    pub fn appliance_noise_kw(now: DateTime<Utc>) -> f64 {
        let day_ordinal = now.date_naive().num_days_from_ce() as u64;
        let hour = now.hour() as f64 + now.minute() as f64 / 60.0 + now.second() as f64 / 3600.0;

        APPLIANCE_PATTERNS
            .iter()
            .map(|p| {
                let seed = day_ordinal
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(p.seed_tag);
                let mut rng = StdRng::seed_from_u64(seed);
                let center_jitter_h: f64 = rng.gen_range(-0.5..0.5);
                let amplitude_jitter: f64 = rng.gen_range(0.7..1.3);
                let center = p.center_hour + center_jitter_h;
                let amplitude = p.amplitude_kw * amplitude_jitter;
                amplitude * (-(hour - center).powi(2) / (2.0 * p.sigma_h.powi(2))).exp()
            })
            .sum()
    }

    pub fn initial_state(cfg: &BaseLoadParams) -> BaseLoadState {
        BaseLoadState {
            actual_power_kw: cfg.baseline_kw,
        }
    }

    /// Pure physics step. Always returns baseline_kw; setpoint and state are ignored.
    pub fn step_inner(
        &self,
        _state: &BaseLoadState,
        _setpoint_kw: f64,
        _dt: Duration,
    ) -> (BaseLoadState, f64) {
        let actual_kw = self.baseline_kw;
        (
            BaseLoadState {
                actual_power_kw: actual_kw,
            },
            actual_kw,
        )
    }

    /// Point-range capability (non-curtailable).
    pub fn capability_inner(&self, state: &BaseLoadState) -> AssetCapability {
        AssetCapability {
            max_export_kw: state.actual_power_kw,
            max_import_kw: state.actual_power_kw,
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        self.baseline_kw
    }

    pub fn state_values(&self, _state: &BaseLoadState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("baseline_kw".into(), self.baseline_kw);
        m
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![
            ControlDescriptor {
                key: "base_load_kw".into(),
                label: "Base Load Override".into(),
                kind: ControlKind::Slider,
                min: Some(0.0),
                max: Some(6.0),
                unit: "kW".into(),
                display_scale: None,
            },
            ControlDescriptor {
                key: "base_load_alpha".into(),
                label: "Blend-back Speed".into(),
                kind: ControlKind::Slider,
                min: Some(0.01),
                max: Some(1.0),
                unit: "".into(),
                display_scale: None,
            },
        ]
    }

    pub fn reset(&self, _state: &mut BaseLoadState, _values: HashMap<String, f64>) {}

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("baseline_kw") {
            self.baseline_kw = v.max(0.0);
        }
    }

    pub fn forecast(&self, _state: &BaseLoadState, timespan: Duration) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Step);
        }
        let now = Utc::now();
        TimeSeries {
            samples: vec![(now, self.baseline_kw), (now + timespan, self.baseline_kw)],
            interpolation: Interpolation::Step,
        }
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

impl Asset for BaseLoad {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::BaseLoad(s) = state else {
            unreachable!("BaseLoad/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::BaseLoad(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::BaseLoad(s) = state else {
            unreachable!()
        };
        self.capability_inner(s)
    }
}

// ── BaseLoad MILP plugin types ────────────────────────────────────────────────

/// Pre-computed baseline load contribution for one planning cycle.
/// BaseLoad has no LP decision variables — it contributes a constant demand
/// to the power balance at each slot.
#[derive(Debug, Clone)]
pub struct BaseLoadMilpContext {
    /// Background consumption [kW] per slot (positive = import demand).
    pub p_base_kw: Vec<f64>,
}

impl BaseLoad {
    /// Build the BaseLoad MILP context: `n` slots of constant baseline_kw.
    /// Callers may add per-slot `BaselineOverride` adjustments to the returned vec.
    pub fn build_milp_context(&self, n: usize) -> BaseLoadMilpContext {
        BaseLoadMilpContext {
            p_base_kw: vec![self.baseline_kw; n],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn base_load_params_baseline() {
        let params = BaseLoadParams {
            baseline_kw: 1.5,
            ..BaseLoadParams::default()
        };
        assert!((params.baseline_kw - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn appliance_noise_kw_peaks_near_named_hours() {
        let day = Utc.with_ymd_and_hms(2026, 7, 13, 0, 0, 0).unwrap();
        let coffee = BaseLoad::appliance_noise_kw(day + Duration::hours(8));
        let quiet_night = BaseLoad::appliance_noise_kw(day + Duration::hours(3));
        assert!(
            coffee > quiet_night,
            "8am (coffee) should draw more than 3am (quiet), got coffee={coffee}, night={quiet_night}"
        );
        assert!(coffee > 0.1, "expected a real bump near 8am, got {coffee}");
    }

    #[test]
    fn appliance_noise_kw_is_never_negative() {
        let day = Utc.with_ymd_and_hms(2026, 7, 13, 0, 0, 0).unwrap();
        for h in 0..24 {
            let kw = BaseLoad::appliance_noise_kw(day + Duration::hours(h));
            assert!(
                kw >= 0.0,
                "noise at hour {h} should be non-negative, got {kw}"
            );
        }
    }

    #[test]
    fn appliance_noise_kw_is_deterministic_for_same_instant() {
        let now = Utc.with_ymd_and_hms(2026, 7, 13, 8, 5, 0).unwrap();
        assert_eq!(
            BaseLoad::appliance_noise_kw(now),
            BaseLoad::appliance_noise_kw(now)
        );
    }

    #[test]
    fn appliance_noise_kw_varies_day_to_day() {
        let day1 = Utc.with_ymd_and_hms(2026, 7, 13, 8, 0, 0).unwrap();
        let day2 = Utc.with_ymd_and_hms(2026, 7, 14, 8, 0, 0).unwrap();
        assert_ne!(
            BaseLoad::appliance_noise_kw(day1),
            BaseLoad::appliance_noise_kw(day2),
            "different calendar days should jitter to different values"
        );
    }
}
