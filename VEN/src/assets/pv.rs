use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    Asset, AssetCapability, AssetFlexibilityFloor, AssetState, ControlDescriptor, ControlKind,
};
use crate::common::{Interpolation, TimeSeries};
use crate::entities::asset::PowerAdjustability;
use crate::entities::asset_params::{PvCurtailmentSource, PvParams};

fn f64_infinity() -> f64 {
    f64::INFINITY
}

/// PV Inverter config. Generates power (export = negative).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvInverter {
    pub rated_kw: f64,
    /// Inverter's true AC output capability (kW); distinct from `rated_kw` (DC panel peak).
    /// DC potential is clamped to this before any commanded `generation_limit_kw` — see
    /// `openspec/changes/pv-curtailment-history/`. Defaults to `rated_kw` (no hardware ceiling
    /// below panel peak).
    ///
    /// `#[serde(default)]`: `PvInverter` is part of the persisted `sim_state.json` blob, and
    /// `simulator::persist::load_with_params` always overwrites `asset_configs` from fresh
    /// profile params after a successful load — so the deserialized value here is never actually
    /// used, but a missing field must not fail the *whole* state file's deserialization (which
    /// would otherwise also lose unrelated persisted runtime state — SoC, temperature, etc.).
    #[serde(default = "f64_infinity")]
    pub inverter_max_kw: f64,
    /// Active generation limit in kW (≤ 0); None = no curtailment limit. This is a device-level
    /// cap on the inverter's own output — not the site's net grid export, which the inverter has
    /// no visibility into (see `OadrCapacityState.export_limit_kw`/`SimInjectState.grid_export_limit_kw`
    /// for that, genuinely site-level, concept).
    pub generation_limit_kw: Option<f64>,
    /// Which source produced `generation_limit_kw` (plan, live capacity/VTN, arbiter, or
    /// manual sim-inject). Set each tick alongside `generation_limit_kw`, copied into `PvState`
    /// by `step_inner` for accurate historical reconstruction. `#[serde(default)]`: see
    /// `inverter_max_kw`'s doc comment.
    #[serde(default)]
    pub curtailment_source: PvCurtailmentSource,
    /// [0.0, 1.0]; set each tick by sim (natural + offset, clamped). NOT from YAML.
    pub irradiance: f64,
    /// Current perturbation offset above/below the natural sin model. Decays toward zero
    /// each tick at rate `pv_alpha`. Set each tick from PvSmoothingState. NOT from YAML.
    pub irradiance_offset: f64,
    /// Per-tick decay factor for irradiance_offset (0–1). Set from pv_irradiance_alpha inject.
    /// NOT from YAML.
    pub pv_alpha: f64,
    /// Weather-sourced actual power for this tick (kW, generation-positive),
    /// via `entities::solar::resolve_weather_pv_kw` — the same translation
    /// the planner's own PV input uses (R-50), reused here rather than
    /// reimplemented. `None` when no weather feed is configured or the
    /// cached forecast has gone stale. Set each tick by the sim loop. NOT
    /// from YAML.
    pub weather_power_kw: Option<f64>,
    /// True only on the exact tick a manual `pv_irradiance` inject is posted
    /// (Behaviour C: full override, weather ignored entirely that tick).
    /// False while the resulting offset is merely decaying — during that
    /// window the residual perturbation blends additively onto whichever
    /// base (weather or sin-model) is otherwise authoritative, so weather is
    /// never fully suppressed by an old, nearly-decayed manual override
    /// (see `SimState::tick`). Set each tick by the sim loop. NOT from YAML.
    #[serde(default)]
    pub irradiance_forced: bool,
    /// Real measured PV power for this tick (kW, generation-positive), via
    /// `entities::measurement::resolve_measured_kw` — outranks
    /// `weather_power_kw` when present (measured ground truth beats a
    /// forecast estimate). `None` when no measurement feed is configured
    /// (env var absent), the profile doesn't enable it
    /// (`measurements.pv_enabled`), or the cached reading has gone stale.
    /// Set each tick by the sim loop. NOT from YAML.
    #[serde(default)]
    pub measured_power_kw: Option<f64>,
}

/// PV mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvState {
    /// Actual power last tick. Always ≤ 0 (PV only exports). Unit: kW.
    pub actual_power_kw: f64,
    /// The generation limit active during the tick that produced this state — a snapshot copy
    /// of `PvInverter.generation_limit_kw` at that moment, not the live/current value, so a
    /// later historical reconstruction reports what was actually active then.
    #[serde(default)]
    pub generation_limit_kw: Option<f64>,
    /// The source of `generation_limit_kw` during the tick that produced this state.
    #[serde(default)]
    pub curtailment_source: PvCurtailmentSource,
}

impl PvInverter {
    pub fn from_params(cfg: &PvParams) -> Self {
        Self {
            rated_kw: cfg.rated_kw,
            inverter_max_kw: cfg.inverter_max_kw,
            generation_limit_kw: None,
            curtailment_source: PvCurtailmentSource::None,
            irradiance: 0.0,
            irradiance_offset: 0.0,
            pv_alpha: 0.1,
            weather_power_kw: None,
            irradiance_forced: false,
            measured_power_kw: None,
        }
    }

    pub fn initial_state(_cfg: &PvParams) -> PvState {
        PvState {
            actual_power_kw: 0.0,
            generation_limit_kw: None,
            curtailment_source: PvCurtailmentSource::None,
        }
    }

    /// Pure physics step. Ignores setpoint (non-curtailable in Phase A).
    /// While `irradiance_forced` (a manual override posted this exact tick),
    /// `self.irradiance` fully dictates output, weather/measurement ignored
    /// entirely. Otherwise the base is `measured_power_kw` if present (real
    /// ground truth outranks a forecast), else `weather_power_kw`, else the
    /// sin-model `self.irradiance` — with the decaying `irradiance_offset`
    /// from any recently-released manual override blended additively on top
    /// of whichever base wins.
    pub fn step_inner(&self, _state: &PvState, _setpoint_kw: f64, _dt: Duration) -> (PvState, f64) {
        let base_kw = self.measured_power_kw.or(self.weather_power_kw);
        let dc_potential_kw = if self.irradiance_forced {
            self.rated_kw * self.irradiance
        } else {
            match base_kw {
                Some(kw) => (kw.max(0.0) + self.irradiance_offset * self.rated_kw).max(0.0),
                None => self.rated_kw * self.irradiance,
            }
        };
        // Inverter's own AC-side ceiling clips DC potential before any commanded limit —
        // see openspec/changes/pv-curtailment-history/.
        let raw_kw = -dc_potential_kw.min(self.inverter_max_kw); // negative = export
        let actual_kw = self
            .generation_limit_kw
            .map(|lim| raw_kw.max(lim)) // lim ≤ 0; max() clamps to less export
            .unwrap_or(raw_kw);
        (
            PvState {
                actual_power_kw: actual_kw,
                generation_limit_kw: self.generation_limit_kw,
                curtailment_source: self.curtailment_source,
            },
            actual_kw,
        )
    }

    /// Export-only: PV never imports, so `max_import_kw` is pinned to 0
    /// rather than mirroring the export value (see `AssetCapability`'s doc
    /// comment for the general rule). The export ceiling itself is the
    /// currently uncurtailed value; curtailment's achievable range down to
    /// 0 is expressed via `flexibility_floor_inner`, not here.
    pub fn capability_inner(&self, state: &PvState) -> AssetCapability {
        AssetCapability {
            max_export_kw: state.actual_power_kw, // e.g. -2.0
            max_import_kw: 0.0,
            adjustability: PowerAdjustability::Croppable,
            power_steps_kw: vec![],
        }
    }

    /// Export is curtailable to 0 via `generation_limit_kw` (sim-inject or VTN
    /// EXPORT_CAPACITY_LIMIT — see `step_inner`, which clamps every tick, not
    /// just on config change). The floor reflects that achievable minimum,
    /// independent of whether a limit is currently active. PV never imports,
    /// so both directions collapse to the same curtailed-to-zero point —
    /// mirrors battery's idle floor. The ceiling (`capability_inner`) stays
    /// fixed at `actual_power_kw`: curtailment can only reduce export, never
    /// force more than the weather allows.
    pub fn flexibility_floor_inner(&self, _state: &PvState) -> AssetFlexibilityFloor {
        AssetFlexibilityFloor {
            min_export_kw: 0.0,
            min_import_kw: 0.0,
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        f64::MAX // no generation limit by default
    }

    pub fn state_values(&self, state: &PvState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("irradiance".into(), self.irradiance);
        m.insert("rated_kw".into(), self.rated_kw);
        m.insert("inverter_max_kw".into(), self.inverter_max_kw);
        m.insert("irradiance_offset".into(), self.irradiance_offset);
        m.insert("pv_alpha".into(), self.pv_alpha);
        // Read from `state`, not `self`: for a historical point (e.g. from the in-memory
        // AssetHistoryBuffer), `state` is the snapshot taken at that past tick, while `self` is
        // the live/current PvInverter — reading `self` here would report the current limit on
        // every past point instead of what was actually active then.
        if let Some(lim) = state.generation_limit_kw {
            m.insert("generation_limit_kw".into(), lim);
        }
        m.insert(
            "curtailment_source".into(),
            state.curtailment_source.as_f64(),
        );
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
                nullable: false,
            },
            ControlDescriptor {
                key: "pv_irradiance_alpha".into(),
                label: "Blend-back Speed".into(),
                kind: ControlKind::Slider,
                min: Some(0.01),
                max: Some(1.0),
                unit: "".into(),
                display_scale: None,
                nullable: false,
            },
            ControlDescriptor {
                key: "pv_generation_limit_kw".into(),
                label: "Generation Limit".into(),
                kind: ControlKind::Slider,
                min: Some(0.0),
                max: Some(self.inverter_max_kw),
                unit: "kW".into(),
                display_scale: None,
                // max (inverter_max_kw, the true AC ceiling — not rated_kw, the DC
                // panel peak, which can exceed what the inverter can ever output)
                // is physically identical to "no limit", since step_inner clamps
                // to inverter_max_kw everywhere. So the top of the range doubles
                // as the release/"Off" state.
                nullable: true,
            },
        ]
    }

    pub fn reset(&self, _state: &mut PvState, _values: HashMap<String, f64>) {}

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("rated_kw") {
            self.rated_kw = v.max(0.0);
        }
    }

    pub fn forecast(&self, _state: &PvState, timespan: Duration, now: DateTime<Utc>) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Linear);
        }
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

    /// Natural sin-model irradiance [0,1] at `ts`, without any user offset.
    /// Delegates to the domain-owned definition so the formula exists once —
    /// `controller::milp_planner::inputs` used to keep its own mirrored copy.
    pub fn natural_irradiance_at(ts: DateTime<Utc>) -> f64 {
        crate::entities::solar::natural_irradiance_at(ts)
    }

    /// Power output from the sin model at `ts` (kW, negative = export).
    /// Used by `forecast()`. Does NOT include the live irradiance_offset.
    fn irradiance_at(&self, ts: DateTime<Utc>) -> f64 {
        let natural_kw =
            (self.rated_kw * Self::natural_irradiance_at(ts)).min(self.inverter_max_kw);
        let limited_kw = match self.generation_limit_kw {
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

    fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor {
        let AssetState::Pv(s) = state else {
            unreachable!()
        };
        self.flexibility_floor_inner(s)
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
                inverter_max_kw: rated_kw,
                generation_limit_kw: None,
                curtailment_source: PvCurtailmentSource::None,
                weather_power_kw: None,
                irradiance_forced: false,
                measured_power_kw: None,
            },
            PvState {
                actual_power_kw: 0.0,
                generation_limit_kw: None,
                curtailment_source: PvCurtailmentSource::None,
            },
        )
    }

    #[test]
    fn capability_max_import_kw_is_always_zero() {
        // PV never imports — max_import_kw must not mirror the export value.
        let (pv, _) = make_pv(10.0);
        let state = PvState {
            actual_power_kw: -4.2,
            generation_limit_kw: None,
            curtailment_source: PvCurtailmentSource::None,
        };
        let cap = pv.capability_inner(&state);
        assert_eq!(cap.max_import_kw, 0.0);
        assert_eq!(cap.max_export_kw, -4.2);
    }

    #[test]
    fn capability_reports_croppable_adjustability_with_no_power_steps() {
        let (pv, _) = make_pv(10.0);
        let state = PvState {
            actual_power_kw: -4.2,
            generation_limit_kw: None,
            curtailment_source: PvCurtailmentSource::None,
        };
        let cap = pv.capability_inner(&state);
        assert_eq!(cap.adjustability, PowerAdjustability::Croppable);
        assert!(cap.power_steps_kw.is_empty());
    }

    #[test]
    fn flexibility_floor_is_zero_regardless_of_actual_power_kw() {
        // Curtailment (generation_limit_kw) clamps every tick in step_inner, so the
        // achievable floor is 0 kW export — independent of the current
        // uncurtailed actual_power_kw, and independent of whether a limit is
        // presently active on this PvInverter instance.
        let (pv, _) = make_pv(10.0);
        let state = PvState {
            actual_power_kw: -4.2,
            generation_limit_kw: None,
            curtailment_source: PvCurtailmentSource::None,
        };
        let floor = pv.flexibility_floor_inner(&state);
        assert_eq!(floor.min_export_kw, 0.0);
        assert_eq!(floor.min_import_kw, 0.0);
    }

    // ── step_inner: weather_power_kw precedence ──────────────────────────────

    #[test]
    fn step_inner_uses_weather_power_kw_when_set() {
        let (mut pv, state) = make_pv(10.0);
        pv.irradiance = 1.0; // would give -10.0 kW via the sin/manual path if used
        pv.weather_power_kw = Some(6.5);
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 6.5).abs() < 1e-9,
            "weather_power_kw must override the irradiance-based calc, got {power_kw}"
        );
    }

    #[test]
    fn step_inner_falls_back_to_irradiance_when_weather_power_kw_is_none() {
        let (pv, state) = make_pv(10.0);
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        // make_pv sets irradiance=0.0 → 0.0 kW either way, but confirms no panic/None-handling bug.
        assert!((power_kw - 0.0).abs() < 1e-9);
    }

    #[test]
    fn step_inner_clamps_weather_power_kw_to_generation_limit() {
        let (mut pv, state) = make_pv(10.0);
        pv.weather_power_kw = Some(9.0);
        pv.generation_limit_kw = Some(-3.0);
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 3.0).abs() < 1e-9,
            "generation_limit_kw must still clamp weather-sourced output, got {power_kw}"
        );
    }

    #[test]
    fn step_inner_treats_negative_weather_power_kw_as_zero() {
        // forecast_ac_kw is documented non-negative, but defend against a
        // malformed/negative value rather than silently flipping sign to import.
        let (mut pv, state) = make_pv(10.0);
        pv.weather_power_kw = Some(-2.0);
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!((power_kw - 0.0).abs() < 1e-9, "got {power_kw}");
    }

    // ── step_inner: blend decaying manual offset onto weather (not a binary null) ──

    #[test]
    fn step_inner_blends_decaying_offset_onto_weather_power_kw() {
        // Regression for the production bug: a not-yet-fully-decayed manual
        // pv_irradiance override must NOT suppress weather entirely — it
        // blends additively on top of it instead.
        let (mut pv, state) = make_pv(20.0);
        pv.weather_power_kw = Some(5.0);
        pv.irradiance_offset = 0.5; // residual perturbation, still decaying
        pv.irradiance_forced = false; // released, not actively forced
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 15.0).abs() < 1e-9,
            "expected weather(5.0) + offset(0.5)*rated_kw(20.0) = 15.0 kW export, got {power_kw}"
        );
    }

    #[test]
    fn step_inner_forced_override_ignores_weather_entirely() {
        // Behaviour C must be unchanged: while a manual override is actively
        // forced (the exact tick it's posted), weather is ignored outright.
        let (mut pv, state) = make_pv(10.0);
        pv.irradiance = 1.0;
        pv.irradiance_forced = true;
        pv.weather_power_kw = Some(6.5);
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 10.0).abs() < 1e-9,
            "forced override must ignore weather entirely, got {power_kw}"
        );
    }

    #[test]
    fn step_inner_negative_offset_blend_clamps_to_zero() {
        // A large negative residual offset must never push output positive
        // (import) — clamp the blended result, not just the raw weather term.
        let (mut pv, state) = make_pv(10.0);
        pv.weather_power_kw = Some(1.0);
        pv.irradiance_offset = -5.0;
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw - 0.0).abs() < 1e-9,
            "blended dc_potential must clamp to 0, not go negative/import, got {power_kw}"
        );
    }

    // ── step_inner: measured_power_kw precedence (measured > weather > sin) ──

    #[test]
    fn step_inner_measured_power_kw_outranks_weather_power_kw() {
        let (mut pv, state) = make_pv(10.0);
        pv.weather_power_kw = Some(6.5);
        pv.measured_power_kw = Some(4.0);
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 4.0).abs() < 1e-9,
            "measured_power_kw must outrank weather_power_kw, got {power_kw}"
        );
    }

    #[test]
    fn step_inner_falls_back_to_weather_when_measured_power_kw_is_none() {
        let (mut pv, state) = make_pv(10.0);
        pv.weather_power_kw = Some(6.5);
        pv.measured_power_kw = None;
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 6.5).abs() < 1e-9,
            "must fall back to weather_power_kw when measured is absent, got {power_kw}"
        );
    }

    #[test]
    fn step_inner_measured_offset_blend_uses_measured_as_base() {
        let (mut pv, state) = make_pv(20.0);
        pv.measured_power_kw = Some(5.0);
        pv.irradiance_offset = 0.5; // residual perturbation, still decaying
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 15.0).abs() < 1e-9,
            "expected measured(5.0) + offset(0.5)*rated_kw(20.0) = 15.0 kW export, got {power_kw}"
        );
    }

    #[test]
    fn step_inner_forced_override_ignores_measured_power_kw_too() {
        let (mut pv, state) = make_pv(10.0);
        pv.irradiance = 1.0;
        pv.irradiance_forced = true;
        pv.measured_power_kw = Some(4.0);
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 10.0).abs() < 1e-9,
            "forced override must ignore measured_power_kw entirely, got {power_kw}"
        );
    }

    #[test]
    fn forecast_zero_timespan_returns_empty() {
        let (pv, state) = make_pv(5.0);
        let series = pv.forecast(&state, Duration::zero(), Utc::now());
        assert!(
            series.samples.is_empty(),
            "Zero timespan must return empty series"
        );
    }

    #[test]
    fn forecast_has_boundary_point_at_end() {
        let (pv, state) = make_pv(5.0);
        let timespan = Duration::seconds(300);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 9, 0, 0).unwrap();
        let series = pv.forecast(&state, timespan, now);
        assert!(
            !series.samples.is_empty(),
            "Non-zero timespan must produce samples"
        );
        let last_ts = series.samples.last().unwrap().0;
        assert_eq!(
            last_ts,
            now + timespan,
            "Boundary point must be at now+timespan"
        );
    }

    #[test]
    fn forecast_samples_ascending() {
        let (pv, state) = make_pv(5.0);
        let series = pv.forecast(&state, Duration::seconds(120), Utc::now());
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
        let series = pv.forecast(&state, Duration::seconds(300), Utc::now());
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

    // ── inverter_max_kw (pv-curtailment-history) ─────────────────────────────

    #[test]
    fn default_inverter_max_kw_equals_rated_kw_is_a_no_op() {
        // make_pv() sets inverter_max_kw = rated_kw; full irradiance must still yield
        // exactly -rated_kw, matching pre-this-change behavior.
        let (mut pv, state) = make_pv(10.0);
        pv.irradiance = 1.0;
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 10.0).abs() < 1e-9,
            "default inverter_max_kw must not clip output, got {power_kw}"
        );
    }

    #[test]
    fn step_inner_clips_dc_potential_to_inverter_max_kw() {
        let (mut pv, state) = make_pv(10.0);
        pv.irradiance = 1.0; // DC potential = 10.0 kW
        pv.inverter_max_kw = 6.0; // inverter can only deliver 6.0 kW AC
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 6.0).abs() < 1e-9,
            "output must be clipped to inverter_max_kw regardless of DC potential, got {power_kw}"
        );
    }

    #[test]
    fn control_schema_pv_generation_limit_kw_max_is_inverter_max_kw_not_rated_kw() {
        // Regression: the manual generation-limit slider's ceiling (and thus its
        // nullable "Off" position) must track the inverter's true AC capability,
        // not the DC panel peak — step_inner clamps to inverter_max_kw everywhere,
        // so a slider capped at rated_kw would let the "Off" position sit above a
        // value the inverter can ever actually deliver whenever the two diverge.
        let (mut pv, _) = make_pv(14.4);
        pv.inverter_max_kw = 12.5;
        let descriptor = pv
            .control_schema()
            .into_iter()
            .find(|d| d.key == "pv_generation_limit_kw")
            .expect("pv_generation_limit_kw descriptor must exist");
        assert_eq!(
            descriptor.max,
            Some(12.5),
            "slider max must equal inverter_max_kw, not rated_kw (14.4)"
        );
    }

    #[test]
    fn step_inner_clips_weather_power_kw_to_inverter_max_kw() {
        let (mut pv, state) = make_pv(10.0);
        pv.weather_power_kw = Some(9.0);
        pv.inverter_max_kw = 5.0;
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 5.0).abs() < 1e-9,
            "weather-sourced output must also respect inverter_max_kw, got {power_kw}"
        );
    }

    #[test]
    fn commanded_limit_at_or_above_inverter_max_kw_has_no_additional_effect() {
        // The central motivating case: a commanded generation_limit_kw looser than the
        // inverter's own ceiling must not change output beyond what the hardware
        // clamp already produces.
        let (mut pv, state) = make_pv(10.0);
        pv.irradiance = 1.0;
        pv.inverter_max_kw = 6.0;
        pv.generation_limit_kw = Some(-8.0); // looser than -6.0 — must be a no-op
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 6.0).abs() < 1e-9,
            "a looser-than-hardware commanded limit must not change output, got {power_kw}"
        );
    }

    #[test]
    fn commanded_limit_below_inverter_max_kw_still_binds() {
        let (mut pv, state) = make_pv(10.0);
        pv.irradiance = 1.0;
        pv.inverter_max_kw = 6.0;
        pv.generation_limit_kw = Some(-3.0); // tighter than both DC potential and hardware ceiling
        let (_, power_kw) = pv.step_inner(&state, 0.0, Duration::seconds(1));
        assert!(
            (power_kw + 3.0).abs() < 1e-9,
            "a genuinely tighter commanded limit must still bind, got {power_kw}"
        );
    }

    #[test]
    fn pv_inverter_deserializes_from_json_missing_new_fields() {
        // Regression: a PvInverter persisted before this change (in sim_state.json,
        // as part of SimState::asset_configs) lacks `inverter_max_kw` and
        // `curtailment_source`. `simulator::persist::load()` deserializes the whole
        // SimState in one shot and only discards/rebuilds asset_configs afterward —
        // a missing-field error here fails that entire deserialize, losing unrelated
        // persisted runtime state (SoC, temperature) that had nothing to do with PV.
        let json = r#"{
            "rated_kw": 5.0,
            "generation_limit_kw": null,
            "irradiance": 0.5,
            "irradiance_offset": 0.0,
            "pv_alpha": 0.1,
            "weather_power_kw": null
        }"#;
        let pv: PvInverter = serde_json::from_str(json).expect(
            "PvInverter must deserialize from a payload missing inverter_max_kw/curtailment_source",
        );
        assert_eq!(pv.inverter_max_kw, f64::INFINITY);
        assert_eq!(pv.curtailment_source, PvCurtailmentSource::None);
    }

    #[test]
    fn pv_state_deserializes_from_json_missing_new_fields() {
        let json = r#"{ "actual_power_kw": -2.0 }"#;
        let state: PvState = serde_json::from_str(json)
            .expect("PvState must deserialize from a payload missing the new curtailment fields");
        assert_eq!(state.generation_limit_kw, None);
        assert_eq!(state.curtailment_source, PvCurtailmentSource::None);
    }
}
