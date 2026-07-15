use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Asset, AssetCapability, AssetState, ControlDescriptor, ControlKind};
use crate::common::{Interpolation, TimeSeries};
use crate::entities::asset_params::{ApplianceSpikeParams, BaseLoadParams};

/// One configured appliance's daily draw: a trapezoidal power pulse
/// centered on `center_hour` — flat at `amplitude_kw` for the plateau,
/// with a short linear ramp on each edge — plus day-to-day jitter in
/// both timing and magnitude, an optional weekday restriction, and a
/// `probability` that it fires at all on a given day. Sourced from a
/// profile's `base_load.spikes` list (see `profile::schema::SpikeConfig`);
/// empty by default (no configured noise).
///
/// A trapezoid (not a Gaussian) is used deliberately: a Gaussian's tails
/// never reach zero, so its energy integral (`amplitude × sigma × √(2π)`)
/// is uncontrollably larger than a real appliance's on-period draw. A
/// trapezoid's energy is directly `≈ amplitude_kw × (duration_h − ramp_h)`
/// — settable to match a real appliance session.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppliancePattern {
    /// Distinguishes this spike's jitter seed from the others' in the same
    /// list (so their day-to-day draws don't move in lockstep). Derived
    /// from the spike's position in the configured list, not stored in YAML.
    seed_tag: u64,
    center_hour: f64,
    jitter_h: f64,
    amplitude_kw: f64,
    /// Total on-period width in hours (ramp-up + plateau + ramp-down).
    duration_h: f64,
    /// Linear transition width at each edge, in hours. `0.0` (or `>=
    /// duration_h/2`) degenerates to a plain rectangular pulse.
    ramp_h: f64,
    probability: f64,
    /// `0`=Monday..`6`=Sunday; empty means every day.
    weekdays: Vec<u8>,
}

impl AppliancePattern {
    fn from_params(index: usize, p: &ApplianceSpikeParams) -> Self {
        Self {
            seed_tag: index as u64 + 1,
            center_hour: p.center_hour,
            jitter_h: p.jitter_h,
            amplitude_kw: p.amplitude_kw,
            duration_h: p.duration_h,
            ramp_h: p.ramp_h,
            probability: p.probability,
            weekdays: p.weekdays.clone(),
        }
    }
}

/// Trapezoidal pulse value at horizontal distance `dist_h` from center:
/// full `amplitude` on the plateau, a linear ramp down to `0.0` across
/// the outer `ramp_h` band on each side, `0.0` beyond `duration_h / 2`.
fn trapezoid_kw(amplitude: f64, dist_h: f64, duration_h: f64, ramp_h: f64) -> f64 {
    let half = duration_h / 2.0;
    if dist_h >= half {
        0.0
    } else if ramp_h <= 0.0 || dist_h <= half - ramp_h {
        amplitude
    } else {
        amplitude * (half - dist_h) / ramp_h
    }
}

/// Base load config. Fixed background consumption (positive = import).
/// Non-flexible — planner never schedules allocations for this asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseLoad {
    pub baseline_kw: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub baseline_kw_profile: f64,
    /// Configured appliance-noise spikes (coffee/cooking/TV etc.), empty
    /// when the profile declares none.
    patterns: Vec<AppliancePattern>,
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
            patterns: cfg
                .spikes
                .iter()
                .enumerate()
                .map(|(i, s)| AppliancePattern::from_params(i, s))
                .collect(),
        }
    }

    /// Deterministic simulated appliance noise \[kW\] for `now`: additive
    /// trapezoidal pulses around this asset's configured spike times
    /// (coffee/cooking/TV-and-lights etc., from the profile's
    /// `base_load.spikes` list), jittered per calendar day (seeded by
    /// day-ordinal + spike tag) so the same simulated day always
    /// reproduces the same pattern — reproducible in tests — while still
    /// varying non-trivially day to day. A spike restricted to specific
    /// `weekdays` contributes `0.0` outright on any other day. Exists so
    /// BL-14's learned heuristics (Phase 5 WP5.2) have a real daily/weekly
    /// shape to fit instead of a perfectly flat baseline. Returns `0.0`
    /// when no spikes are configured (unconfigured profiles keep today's
    /// flat behavior).
    pub fn appliance_noise_kw(&self, now: DateTime<Utc>) -> f64 {
        let day_ordinal = now.date_naive().num_days_from_ce() as u64;
        let weekday = now.weekday().num_days_from_monday() as u8;
        let hour = now.hour() as f64 + now.minute() as f64 / 60.0 + now.second() as f64 / 3600.0;

        self.patterns
            .iter()
            .map(|p| {
                if !p.weekdays.is_empty() && !p.weekdays.contains(&weekday) {
                    return 0.0;
                }
                let seed = day_ordinal
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(p.seed_tag);
                let mut rng = StdRng::seed_from_u64(seed);
                let center_jitter_h: f64 = rng.gen_range(-p.jitter_h..p.jitter_h);
                let amplitude_jitter: f64 = rng.gen_range(0.7..1.3);
                let fires_today: f64 = rng.gen_range(0.0..1.0);
                if fires_today > p.probability {
                    return 0.0;
                }
                let center = p.center_hour + center_jitter_h;
                let amplitude = p.amplitude_kw * amplitude_jitter;
                let dist_h = (hour - center).abs();
                trapezoid_kw(amplitude, dist_h, p.duration_h, p.ramp_h)
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

    // jitter_h is kept well under duration_h/2 - ramp_h (0.095) so that tests
    // sampling at the exact center_hour instant reliably land in the plateau
    // regardless of the per-day jitter draw — day-to-day variation is still
    // exercised via the independent amplitude jitter (0.7x-1.3x).
    fn coffee_spike() -> ApplianceSpikeParams {
        ApplianceSpikeParams {
            center_hour: 8.0,
            jitter_h: 0.05,
            amplitude_kw: 1.2,
            duration_h: 0.25,
            ramp_h: 0.03,
            probability: 1.0,
            weekdays: vec![],
        }
    }

    fn base_load_with_spikes(spikes: Vec<ApplianceSpikeParams>) -> BaseLoad {
        BaseLoad::from_params(&BaseLoadParams {
            baseline_kw: 0.3,
            spikes,
            ..BaseLoadParams::default()
        })
    }

    #[test]
    fn base_load_params_baseline() {
        let params = BaseLoadParams {
            baseline_kw: 1.5,
            ..BaseLoadParams::default()
        };
        assert!((params.baseline_kw - 1.5).abs() < f64::EPSILON);
    }

    // ── trapezoid_kw shape ────────────────────────────────────────────────

    #[test]
    fn trapezoid_kw_full_amplitude_at_center() {
        assert_eq!(trapezoid_kw(2.0, 0.0, 0.5, 0.05), 2.0);
    }

    #[test]
    fn trapezoid_kw_zero_outside_half_duration() {
        assert_eq!(trapezoid_kw(2.0, 0.3, 0.5, 0.05), 0.0); // 0.3 >= 0.25 (half)
    }

    #[test]
    fn trapezoid_kw_ramps_linearly_at_the_edge() {
        // half=0.25, ramp=0.05: plateau ends at dist=0.20; at dist=0.225
        // (halfway into the ramp band) expect ~half amplitude.
        let v = trapezoid_kw(2.0, 0.225, 0.5, 0.05);
        assert!(
            (v - 1.0).abs() < 1e-9,
            "expected ~1.0 kW halfway down the ramp, got {v}"
        );
    }

    #[test]
    fn trapezoid_kw_never_negative() {
        for i in 0..200 {
            let dist = i as f64 * 0.01;
            let v = trapezoid_kw(2.0, dist, 0.5, 0.05);
            assert!(v >= 0.0, "dist={dist} got {v}");
        }
    }

    #[test]
    fn trapezoid_kw_zero_ramp_is_a_rectangle() {
        assert_eq!(trapezoid_kw(2.0, 0.1, 0.5, 0.0), 2.0);
        assert_eq!(trapezoid_kw(2.0, 0.24, 0.5, 0.0), 2.0);
        assert_eq!(trapezoid_kw(2.0, 0.26, 0.5, 0.0), 0.0);
    }

    // ── appliance_noise_kw ───────────────────────────────────────────────

    #[test]
    fn appliance_noise_kw_peaks_near_named_hours() {
        let bl = base_load_with_spikes(vec![coffee_spike()]);
        let day = Utc.with_ymd_and_hms(2026, 7, 13, 0, 0, 0).unwrap();
        let coffee = bl.appliance_noise_kw(day + Duration::hours(8));
        let quiet_night = bl.appliance_noise_kw(day + Duration::hours(3));
        assert!(
            coffee > quiet_night,
            "8am (coffee) should draw more than 3am (quiet), got coffee={coffee}, night={quiet_night}"
        );
        assert!(coffee > 0.1, "expected a real bump near 8am, got {coffee}");
    }

    #[test]
    fn appliance_noise_kw_is_never_negative() {
        let bl = base_load_with_spikes(vec![coffee_spike()]);
        let day = Utc.with_ymd_and_hms(2026, 7, 13, 0, 0, 0).unwrap();
        for h in 0..24 {
            let kw = bl.appliance_noise_kw(day + Duration::hours(h));
            assert!(
                kw >= 0.0,
                "noise at hour {h} should be non-negative, got {kw}"
            );
        }
    }

    #[test]
    fn appliance_noise_kw_is_deterministic_for_same_instant() {
        let bl = base_load_with_spikes(vec![coffee_spike()]);
        let now = Utc.with_ymd_and_hms(2026, 7, 13, 8, 5, 0).unwrap();
        assert_eq!(bl.appliance_noise_kw(now), bl.appliance_noise_kw(now));
    }

    #[test]
    fn appliance_noise_kw_varies_day_to_day() {
        let bl = base_load_with_spikes(vec![coffee_spike()]);
        let day1 = Utc.with_ymd_and_hms(2026, 7, 13, 8, 0, 0).unwrap();
        let day2 = Utc.with_ymd_and_hms(2026, 7, 14, 8, 0, 0).unwrap();
        assert_ne!(
            bl.appliance_noise_kw(day1),
            bl.appliance_noise_kw(day2),
            "different calendar days should jitter to different values"
        );
    }

    #[test]
    fn appliance_noise_kw_is_zero_with_no_configured_spikes() {
        let bl = base_load_with_spikes(vec![]);
        let day = Utc.with_ymd_and_hms(2026, 7, 13, 8, 0, 0).unwrap();
        assert_eq!(
            bl.appliance_noise_kw(day),
            0.0,
            "unconfigured profiles must keep today's flat (no-noise) behavior"
        );
    }

    #[test]
    fn appliance_noise_kw_probability_zero_never_fires() {
        let mut spike = coffee_spike();
        spike.probability = 0.0;
        let bl = base_load_with_spikes(vec![spike]);
        let day = Utc.with_ymd_and_hms(2026, 7, 13, 8, 0, 0).unwrap();
        for d in 0..30 {
            let kw = bl.appliance_noise_kw(day + Duration::days(d));
            assert_eq!(kw, 0.0, "probability=0.0 must never fire, day offset {d}");
        }
    }

    #[test]
    fn appliance_noise_kw_probability_one_always_fires() {
        let bl = base_load_with_spikes(vec![coffee_spike()]); // probability: 1.0
        let day = Utc.with_ymd_and_hms(2026, 7, 13, 8, 0, 0).unwrap();
        for d in 0..30 {
            let kw = bl.appliance_noise_kw(day + Duration::days(d));
            assert!(
                kw > 0.0,
                "probability=1.0 must always fire, day offset {d}, got {kw}"
            );
        }
    }

    // ── weekday-conditional spikes ───────────────────────────────────────

    #[test]
    fn appliance_noise_kw_weekday_restricted_spike_is_silent_on_other_days() {
        // 2023-01-02 was a Monday, 2023-01-07 a Saturday.
        let mut weekday_only = coffee_spike();
        weekday_only.weekdays = vec![0, 1, 2, 3, 4]; // Mon-Fri
        let bl = base_load_with_spikes(vec![weekday_only]);

        let monday_8am = Utc.with_ymd_and_hms(2023, 1, 2, 8, 0, 0).unwrap();
        let saturday_8am = Utc.with_ymd_and_hms(2023, 1, 7, 8, 0, 0).unwrap();

        assert!(
            bl.appliance_noise_kw(monday_8am) > 0.0,
            "must fire on Monday"
        );
        assert_eq!(
            bl.appliance_noise_kw(saturday_8am),
            0.0,
            "must be silent on Saturday when restricted to Mon-Fri"
        );
    }

    #[test]
    fn appliance_noise_kw_weekend_restricted_spike_fires_only_on_weekend() {
        let mut weekend_only = coffee_spike();
        weekend_only.center_hour = 10.5;
        weekend_only.weekdays = vec![5, 6]; // Sat, Sun
        let bl = base_load_with_spikes(vec![weekend_only]);

        let saturday = Utc.with_ymd_and_hms(2023, 1, 7, 10, 30, 0).unwrap();
        let monday = Utc.with_ymd_and_hms(2023, 1, 2, 10, 30, 0).unwrap();

        assert!(
            bl.appliance_noise_kw(saturday) > 0.0,
            "must fire on Saturday"
        );
        assert_eq!(
            bl.appliance_noise_kw(monday),
            0.0,
            "must be silent on Monday when restricted to Sat/Sun"
        );
    }
}
