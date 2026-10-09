use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::own_state::{own, own_mut};
use super::{
    Asset, AssetCapability, AssetFlexibilityFloor, AssetState, ControlDescriptor, KeyFeature,
    MilpParticipant, RequestResolvable,
};
use crate::entities::asset::{
    AssetHistoryView, ComfortRate, CompletionPolicy, PowerAdjustability, SetpointResponse,
};
use crate::entities::asset_params::{BatteryParams, RequestDefaults};
use crate::entities::device_session::{EvSession, HeaterTarget};
use lab_core::time_series::{Interpolation, TimeSeries};

/// Minimum time a direction's max rate must be sustainable to be reported as
/// available power (see `Battery::capability_inner`).
const SUSTAINED_POWER_MIN_S: f64 = 60.0;

/// Battery storage config. Bidirectional.
/// Positive setpoint = charge (import), negative = discharge (export).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Battery {
    pub capacity_kwh: f64,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    /// R-69 (battery-efficiency-model-reconciliation): loss is split symmetrically as
    /// `sqrt(round_trip_efficiency)` on both the charge and discharge legs (`step_inner`/
    /// `forecast`) — matching `battery_milp.rs::build_milp_context`'s `eff_ch`/`eff_dis`
    /// derivation, which already used this convention. Was previously asymmetric here (all
    /// loss on charge, none on discharge) — both models agreed on full-cycle totals but
    /// diverged on intermediate SoC for any partial cycle, the normal case under this
    /// project's 5-minute rolling replan. Keep these two files' efficiency math in sync if
    /// either changes; see `docs/history/project_journal.md`'s R-69 entry for the worked
    /// example.
    pub round_trip_efficiency: f64,
    #[serde(rename = "min_soc")]
    pub min_soc_frac: f64,
}

/// Battery mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryState {
    /// State of charge in [0.0, 1.0]. 0.0 = empty, 1.0 = full.
    #[serde(rename = "soc")]
    pub soc_frac: f64,
    /// Actual power last tick. Positive = charging (import). Negative = discharging (export).
    pub actual_power_kw: f64,
}

impl Battery {
    pub fn from_params(cfg: &BatteryParams) -> Self {
        Self {
            capacity_kwh: cfg.capacity_kwh,
            max_charge_kw: cfg.max_charge_kw,
            max_discharge_kw: cfg.max_discharge_kw,
            round_trip_efficiency: cfg.round_trip_efficiency,
            min_soc_frac: cfg.min_soc_frac,
        }
    }

    pub fn initial_state(cfg: &BatteryParams) -> BatteryState {
        BatteryState {
            soc_frac: cfg.initial_soc_frac,
            actual_power_kw: 0.0,
        }
    }

    /// Pure physics step. Returns (new_state, actual_power_kw).
    pub fn step_inner(
        &self,
        state: &BatteryState,
        setpoint_kw: f64,
        dt: Duration,
    ) -> (BatteryState, f64) {
        let dt_h = crate::entities::units::dt_h_from_duration(dt);
        let clamped = setpoint_kw
            .max(-self.max_discharge_kw)
            .min(self.max_charge_kw);
        let actual = if (clamped > 0.0 && state.soc_frac >= 1.0)
            || (clamped < 0.0 && state.soc_frac <= self.min_soc_frac)
        {
            0.0
        } else {
            clamped
        };
        // R-69: symmetric sqrt(round_trip_efficiency) split on both legs,
        // matching battery_milp.rs's own convention (D-A, resolved in
        // battery-efficiency-model-reconciliation) -- was all-loss-on-charge.
        let eff = self.round_trip_efficiency.sqrt();
        let energy_kwh = actual * dt_h * if actual > 0.0 { eff } else { 1.0 / eff };
        let new_soc = (state.soc_frac + energy_kwh / self.capacity_kwh).clamp(0.0, 1.0);
        (
            BatteryState {
                soc_frac: new_soc,
                actual_power_kw: actual,
            },
            actual,
        )
    }

    /// Point-in-time feasible power range. A direction only counts as available
    /// if its max rate can be sustained for `SUSTAINED_POWER_MIN_S`: a battery
    /// parked at 99.95 % has seconds of room, not 5 kW of import capability.
    /// Energy physics (`step_inner`, the MILP's energy balance) stay exact.
    pub fn capability_inner(&self, state: &BatteryState) -> AssetCapability {
        let eff = self.round_trip_efficiency.sqrt();
        let window_h = crate::entities::units::dt_h_from_s(SUSTAINED_POWER_MIN_S);
        let charge_room_kwh = (1.0 - state.soc_frac) * self.capacity_kwh;
        let discharge_room_kwh = (state.soc_frac - self.min_soc_frac) * self.capacity_kwh;
        AssetCapability {
            max_export_kw: if discharge_room_kwh > self.max_discharge_kw / eff * window_h {
                -self.max_discharge_kw
            } else {
                0.0
            },
            max_import_kw: if charge_room_kwh > self.max_charge_kw * eff * window_h {
                self.max_charge_kw
            } else {
                0.0
            },
            adjustability: PowerAdjustability::Stepless,
            response: SetpointResponse::continuous(),
        }
    }

    /// Continuously controllable in both directions — can always idle at 0,
    /// no minimum-nonzero commitment exists for battery.
    pub fn flexibility_floor_inner(&self, _state: &BatteryState) -> AssetFlexibilityFloor {
        AssetFlexibilityFloor {
            min_export_kw: 0.0,
            min_import_kw: 0.0,
        }
    }

    pub fn state_values(&self, state: &BatteryState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("soc".into(), state.soc_frac);
        m.insert("capacity_kwh".into(), self.capacity_kwh);
        m.insert("max_charge_kw".into(), self.max_charge_kw);
        m.insert("max_discharge_kw".into(), self.max_discharge_kw);
        m.insert("min_soc".into(), self.min_soc_frac);
        m.insert("round_trip_efficiency".into(), self.round_trip_efficiency);
        m
    }

    pub fn reset(&self, state: &mut BatteryState, values: HashMap<String, f64>) {
        if let Some(&soc_frac) = values.get("soc") {
            state.soc_frac = soc_frac.clamp(0.0, 1.0);
        }
    }

    pub fn forecast(
        &self,
        state: &BatteryState,
        timespan: Duration,
        now: DateTime<Utc>,
    ) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Linear);
        }
        let end = now + timespan;
        let setpoint = state
            .actual_power_kw
            .clamp(-self.max_discharge_kw, self.max_charge_kw);
        let mut samples: Vec<(DateTime<Utc>, f64)> = Vec::new();

        let mut t = now;
        let mut soc_frac = state.soc_frac;

        while t < end {
            let kw = if (setpoint > 0.0 && soc_frac >= 1.0)
                || (setpoint < 0.0 && soc_frac <= self.min_soc_frac)
            {
                0.0
            } else {
                setpoint
            };
            samples.push((t, kw));

            let dt_h = 1.0 / 60.0;
            // R-69: same symmetric sqrt(round_trip_efficiency) split as step_inner.
            let eff = self.round_trip_efficiency.sqrt();
            if kw > 0.0 {
                soc_frac +=
                    (crate::entities::units::energy_kwh(kw, dt_h) * eff) / self.capacity_kwh;
            } else {
                soc_frac +=
                    (crate::entities::units::energy_kwh(kw, dt_h) / eff) / self.capacity_kwh;
            }
            soc_frac = soc_frac.clamp(0.0, 1.0);
            t += Duration::seconds(60);
        }
        let end_kw = if (setpoint > 0.0 && soc_frac >= 1.0)
            || (setpoint < 0.0 && soc_frac <= self.min_soc_frac)
        {
            0.0
        } else {
            setpoint
        };
        samples.push((end, end_kw));

        TimeSeries {
            samples,
            interpolation: Interpolation::Linear,
        }
    }
}

impl Asset for Battery {
    fn key_features(&self, _state: &AssetState) -> Vec<KeyFeature> {
        vec![KeyFeature::energy_kwh("capacity", self.capacity_kwh)]
    }

    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let s: &BatteryState = own(state);
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::Battery(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let s: &BatteryState = own(state);
        self.capability_inner(s)
    }

    fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor {
        let s: &BatteryState = own(state);
        self.flexibility_floor_inner(s)
    }

    fn default_setpoint(&self) -> f64 {
        0.0 // hold by default; dispatcher controls
    }

    fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![]
    }

    /// A state of charge or a minimum outside 0..=1, or a capacity that is not positive, is
    /// refused rather than clamped: the caller asked for something this battery cannot be.
    fn validate_values(
        &self,
        asset_id: &str,
        values: &HashMap<String, f64>,
    ) -> Result<(), crate::entities::DomainError> {
        let refuse = |key: &str, message: &str| crate::entities::DomainError::InvalidValue {
            asset_id: asset_id.to_string(),
            key: key.to_string(),
            message: message.to_string(),
        };
        for key in ["soc", "min_soc"] {
            if values.get(key).is_some_and(|v| !(0.0..=1.0).contains(v)) {
                return Err(refuse(key, &format!("{key} must be between 0.0 and 1.0")));
            }
        }
        if values.get("capacity_kwh").is_some_and(|v| *v <= 0.0) {
            return Err(refuse("capacity_kwh", "capacity_kwh must be > 0"));
        }
        Ok(())
    }

    fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("capacity_kwh") {
            self.capacity_kwh = v.max(0.1);
        }
        if let Some(&v) = values.get("min_soc") {
            self.min_soc_frac = v.clamp(0.0, 1.0);
        }
    }

    fn default_comfort_rates(&self) -> Vec<ComfortRate> {
        vec![
            crate::entities::asset::ComfortRate {
                fill: 0.0,
                max_marginal_price_eur_kwh: 0.20,
                max_marginal_co2: 0.0,
            },
            crate::entities::asset::ComfortRate {
                fill: 1.0,
                max_marginal_price_eur_kwh: 0.05,
                max_marginal_co2: 0.0,
            },
        ]
    }

    fn default_completion_policy(&self) -> CompletionPolicy {
        crate::entities::asset::CompletionPolicy::Stop
    }

    fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        None
    }

    fn state_values(&self, state: &AssetState) -> HashMap<String, f64> {
        let s: &BatteryState = own(state);
        Self::state_values(self, s)
    }

    fn history_view(&self, state: &AssetState) -> AssetHistoryView {
        let s: &BatteryState = own(state);
        AssetHistoryView {
            soc_frac: Some(s.soc_frac),
            ..Default::default()
        }
    }

    fn reset(&self, state: &mut AssetState, values: HashMap<String, f64>) {
        let s: &mut BatteryState = own_mut(state);
        Self::reset(self, s, values)
    }

    fn forecast(&self, state: &AssetState, timespan: Duration, now: DateTime<Utc>) -> TimeSeries {
        let s: &BatteryState = own(state);
        Self::forecast(self, s, timespan, now)
    }

    fn as_milp_participant(&self) -> Option<&dyn MilpParticipant> {
        Some(self)
    }

    fn as_request_resolvable(&self) -> Option<&dyn RequestResolvable> {
        Some(self)
    }

    fn asset_type(&self) -> crate::entities::asset::AssetType {
        crate::entities::asset::AssetType::Battery
    }

    fn asset_type_str(&self) -> &'static str {
        "battery"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn clone_box(&self) -> Box<dyn Asset> {
        Box::new(self.clone())
    }
}

impl MilpParticipant for Battery {
    #[allow(clippy::too_many_arguments)] // trait-mandated signature shared by 4 heterogeneous asset kinds — see trait doc
    fn build_milp_context(
        &self,
        _asset_id: &str,
        state: &AssetState,
        _n: usize,
        _cum_s: &[i64],
        _now: DateTime<Utc>,
        _ev_sessions: &[EvSession],
        _heater_target: Option<&HeaterTarget>,
        _comfort_rates: &[crate::entities::asset::ComfortRate],
        _ev_min_charge_kw: f64,
        _v_ev_extra_eur_kwh: f64,
        _v_ev_core_eur_kwh: f64,
        _asap_lateness_eur_kwh_h: f64,
        _v_ev_free_charge_eur_kwh: f64,
        _lambda_sw: f64,
        c_terminal_eur_kwh: f64,
        _heater_anchor: Vec<Option<f64>>,
        _w_ghg_eur_kg: f64,
    ) -> Box<dyn crate::controller::milp_planner::AssetMilpContext> {
        Box::new(
            crate::controller::milp_planner::asset_port::BatteryMilpContext::from_state(
                state,
                self,
                c_terminal_eur_kwh,
            ),
        )
    }
}

impl RequestResolvable for Battery {
    /// A battery with no stated target fills to full.
    fn request_defaults(&self, state: &AssetState) -> RequestDefaults {
        let s: &BatteryState = own(state);
        RequestDefaults {
            current_soc: s.soc_frac,
            default_soc_target: 1.0,
            capacity_kwh: self.capacity_kwh,
            max_charge_kw: self.max_charge_kw,
        }
    }

    /// Moved here verbatim from `AssetConfig::available_storage_kwh`'s Battery
    /// arm (no prior `Battery`-only inherent method existed for this).
    fn available_storage_kwh(&self, state: &AssetState) -> Option<(f64, f64)> {
        let s: &BatteryState = own(state);
        Some((
            (s.soc_frac - self.min_soc_frac).max(0.0) * self.capacity_kwh,
            (1.0 - s.soc_frac).max(0.0) * self.capacity_kwh,
        ))
    }

    /// Battery cannot opportunistically absorb surplus — matches
    /// `AssetConfig::surplus_charge_kw`'s existing `_ => None` fallback for
    /// every non-EV asset kind today.
    fn surplus_charge_kw(&self, _state: &AssetState, _surplus_kw: f64) -> Option<f64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn key_features_declare_capacity_from_the_profile() {
        let (bat, state) = make_battery_cfg(0.5);
        let features = Asset::key_features(&bat, &AssetState::Battery(state));
        assert_eq!(features, vec![KeyFeature::new("capacity", "10.0 kWh")]);
    }

    #[test]
    fn history_view_agrees_with_state_values() {
        let (bat, state) = make_battery_cfg(0.37);
        let state = AssetState::Battery(state);
        let view = Asset::history_view(&bat, &state);
        assert_eq!(
            view.soc_frac,
            Asset::state_values(&bat, &state).get("soc").copied()
        );
        assert_eq!(view.plugged, None);
        assert_eq!(view.temperature_c, None);
    }

    fn make_battery_cfg(initial_soc_frac: f64) -> (Battery, BatteryState) {
        let cfg = BatteryParams {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            round_trip_efficiency: 0.95,
            min_soc_frac: 0.1,
            initial_soc_frac,
            c_terminal_eur_kwh: None,
        };
        (Battery::from_params(&cfg), Battery::initial_state(&cfg))
    }

    // Capacity-forecast (openspec/changes/flexibility-capacity-forecast) needs
    // round_trip_efficiency alongside the already-present capacity_kwh/max_charge_kw/
    // max_discharge_kw/min_soc to compute charge-direction duration without importing
    // Battery directly.
    #[test]
    fn state_values_exposes_round_trip_efficiency() {
        let (bat, state) = make_battery_cfg(0.5);
        let vals = bat.state_values(&state);
        assert_eq!(
            vals.get("round_trip_efficiency"),
            Some(&bat.round_trip_efficiency)
        );
    }

    #[test]
    fn capability_reports_stepless_adjustability_with_no_power_steps() {
        let (bat, state) = make_battery_cfg(0.5);
        let cap = bat.capability_inner(&state);
        assert_eq!(cap.adjustability, PowerAdjustability::Stepless);
        assert!(cap.response.power_steps_kw.is_empty());
    }

    #[test]
    fn capability_reports_no_import_when_the_room_cannot_sustain_max_charge_for_60s() {
        // 10 kWh, 5 kW, eff 0.95: 60 s at max charge stores ~0.081 kWh (~0.81 %).
        // VEN1 parked at 99.955 % and kept reporting the full 5 kW for ~3 s of room.
        let (bat, mut state) = make_battery_cfg(0.99955);
        assert_eq!(bat.capability_inner(&state).max_import_kw, 0.0);
        state.soc_frac = 0.995;
        assert_eq!(bat.capability_inner(&state).max_import_kw, 0.0);
        state.soc_frac = 0.99; // 0.1 kWh room: more than 60 s at 5 kW
        assert_eq!(bat.capability_inner(&state).max_import_kw, 5.0);
    }

    #[test]
    fn capability_reports_no_export_when_the_energy_cannot_sustain_max_discharge_for_60s() {
        let (bat, mut state) = make_battery_cfg(0.105); // min_soc 0.1: 0.05 kWh left
        assert_eq!(bat.capability_inner(&state).max_export_kw, 0.0);
        state.soc_frac = 0.11;
        assert_eq!(bat.capability_inner(&state).max_export_kw, -5.0);
    }

    #[test]
    fn step_still_tops_up_the_last_percent_exactly() {
        // The 60 s rule is about what power can be promised, not energy physics:
        // step (and the MILP's energy balance) still fill up to exactly 100 %.
        let (bat, state) = make_battery_cfg(0.995);
        let (next, kw) = bat.step_inner(&state, 5.0, Duration::seconds(1));
        assert_eq!(kw, 5.0);
        assert!(next.soc_frac > 0.995);
    }

    #[test]
    fn flexibility_floor_is_always_zero_regardless_of_soc() {
        for soc_frac in [0.05, 0.5, 1.0] {
            // 0.05 is below min_soc=0.1; 1.0 is fully charged.
            let (bat, state) = make_battery_cfg(soc_frac);
            let floor = bat.flexibility_floor_inner(&state);
            assert_eq!(floor.min_export_kw, 0.0, "soc={soc_frac}");
            assert_eq!(floor.min_import_kw, 0.0, "soc={soc_frac}");
        }
    }

    #[test]
    fn forecast_zero_timespan_returns_empty() {
        let (bat, state) = make_battery_cfg(0.5);
        let series = bat.forecast(&state, Duration::zero(), Utc::now());
        assert!(series.samples.is_empty());
    }

    #[test]
    fn forecast_at_full_soc_charge_setpoint_returns_zero() {
        let (bat, mut state) = make_battery_cfg(1.0);
        state.actual_power_kw = 5.0;
        let series = bat.forecast(&state, Duration::seconds(300), Utc::now());
        for (_, v) in &series.samples {
            assert_eq!(
                *v, 0.0,
                "Full SoC battery with charge setpoint must return zero"
            );
        }
    }

    #[test]
    fn forecast_has_boundary_point_at_exactly_now_plus_timespan() {
        let (bat, state) = make_battery_cfg(0.5);
        let timespan = Duration::seconds(120);
        let now = Utc.with_ymd_and_hms(2026, 7, 20, 9, 0, 0).unwrap();
        let series = bat.forecast(&state, timespan, now);
        assert!(!series.samples.is_empty());
        let last_ts = series.samples.last().unwrap().0;
        assert_eq!(
            last_ts,
            now + timespan,
            "boundary point must be exactly now+timespan, not merely close to wall-clock"
        );
    }

    #[test]
    fn step_inner_applies_symmetric_sqrt_efficiency_split_on_both_legs() {
        // R-69 (battery-efficiency-model-reconciliation): battery.rs previously put
        // all round-trip loss on the charge leg only; battery_milp.rs already split
        // it symmetrically via sqrt(round_trip_efficiency) on both legs. Both agree
        // on full-cycle totals, but the intermediate SoC after a partial cycle
        // diverges -- this pins the now-shared (D-A: symmetric) convention.
        //
        // rte=0.81 -> eff=sqrt(0.81)=0.9. capacity=100kWh (large, to isolate the
        // efficiency math from the min_soc/full-SoC clamps), initial_soc=0.5 (50 kWh).
        let mut bat = make_battery_cfg(0.5).0;
        bat.capacity_kwh = 100.0;
        bat.max_charge_kw = 20.0;
        bat.max_discharge_kw = 20.0;
        bat.round_trip_efficiency = 0.81;
        let state = BatteryState {
            soc_frac: 0.5,
            actual_power_kw: 0.0,
        };

        // Charge 10 kWh of AC import (10 kW for 1h) -> 9.0 kWh actually stored.
        let (state, actual) = bat.step_inner(&state, 10.0, Duration::hours(1));
        assert_eq!(actual, 10.0);
        assert!(
            (state.soc_frac - 0.59).abs() < 1e-9,
            "expected soc=0.59 (50 + 10*0.9 = 59 kWh / 100), got {}",
            state.soc_frac
        );

        // Discharge 9 kWh of AC export (9 kW for 1h) -> 10.0 kWh actually removed
        // from storage (more removed than delivered -- the discharge-leg loss).
        let (state, actual) = bat.step_inner(&state, -9.0, Duration::hours(1));
        assert_eq!(actual, -9.0);
        assert!(
            (state.soc_frac - 0.49).abs() < 1e-9,
            "expected soc=0.49 (59 - 9/0.9 = 49 kWh / 100), got {}",
            state.soc_frac
        );
    }

    #[test]
    fn step_charges_and_stops_at_full() {
        let (bat, mut state) = make_battery_cfg(0.99);
        for _ in 0..1000 {
            let (ns, _) = bat.step_inner(&state, 10.0, Duration::seconds(1));
            state = ns;
        }
        assert!((state.soc_frac - 1.0).abs() < 0.001);
        let (_, actual) = bat.step_inner(&state, 10.0, Duration::seconds(1));
        assert_eq!(actual, 0.0);
    }
}

#[cfg(test)]
mod param_tests {
    use super::*;

    #[test]
    fn battery_params_default_soc() {
        assert!((BatteryParams::default().initial_soc_frac - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn battery_params_custom_capacity() {
        let params = BatteryParams {
            capacity_kwh: 20.0,
            ..BatteryParams::default()
        };
        assert!((params.capacity_kwh - 20.0).abs() < f64::EPSILON);
    }
}
