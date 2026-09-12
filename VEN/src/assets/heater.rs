use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    Asset, AssetCapability, AssetFlexibilityFloor, AssetState, ControlDescriptor, MilpParticipant,
    Thermostat, TickOverridable,
};
use crate::common::{Interpolation, TimeSeries};
use crate::entities::asset::{ComfortRate, CompletionPolicy, PowerAdjustability};
use crate::entities::asset_params::HeaterParams;
use crate::entities::timeline::HeaterPlanTrajectory;

// HeaterEmergencyMode moved to heater_emergency.rs (Spec A Phase 2a, file-size split).
pub use super::heater_emergency::HeaterEmergencyMode;

/// Heater config. Consumes power for space heating (positive = import).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heater {
    pub max_kw: f64,
    /// Number of switchable power stages: 1 (on/off) or 2 (mid/full). Reachable
    /// levels are `k × max_kw / power_stages` for k in 0..=power_stages; setpoints
    /// are quantized to the nearest in step_inner(). Default 2 — also what old
    /// persisted JSON without this field deserialises to, matching the previous
    /// `max_kw / 2.0` mid-level fallback.
    #[serde(default = "default_power_stages_sim")]
    pub power_stages: u8,
    /// Tank hysteresis lower bound. Overridable at runtime via SimInjectState.
    pub temp_min_c: f64,
    /// Tank hysteresis upper bound. Overridable at runtime via SimInjectState.
    pub temp_max_c: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub temp_min_c_profile: f64,
    /// Original profile value — used for snap-back when inject override is released.
    pub temp_max_c_profile: f64,
    /// True hard safety ceiling, above `temp_max_c`. Only reachable in `Absorb` mode.
    /// Defaults to `temp_max_c` when not configured — behaviour is then identical to
    /// today's (no extra headroom), so existing profiles are unaffected.
    #[serde(default)]
    pub temp_safety_max_c: f64,
    /// Set each tick by sim from SimInjectState (Behaviour C); NOT from YAML. Defaults
    /// to `Normal` — no behaviour change until something actively sets it.
    #[serde(default)]
    pub emergency_mode: HeaterEmergencyMode,
    /// Thermal mass in kWh/°C. Derived from volume_l (water tank) or explicit config.
    pub thermal_mass_kwh_per_c: f64,
    /// Newton cooling coefficient (kW/°C). Loss = k_loss × (temp − ambient).
    pub k_loss_kw_per_c: f64,
    /// Constant simulated hot water draw (kW thermal). Defaults to 0.0.
    pub draw_kw: f64,
    /// Set each tick by sim from SimInjectState.ambient_temp_c; NOT from YAML.
    pub ambient_temp_c: f64,
}

/// Heater mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaterState {
    pub temperature_c: f64,
    /// Actual power last tick. Always ≥ 0 (heaters only consume).
    pub actual_power_kw: f64,
    /// The thermostat's emergency fired (temperature reached `temp_min_c`) and is
    /// still running its hysteresis toward `temp_min_c + 3 °C`. Kept explicitly
    /// rather than inferred from `actual_power_kw`, which cannot tell an emergency
    /// from the planner commanding the top stage (GB-44).
    #[serde(default)]
    pub emergency_latched: bool,
}

/// Serde default for `Heater::power_stages` — see the field's doc comment.
fn default_power_stages_sim() -> u8 {
    2
}

impl Heater {
    /// Power delivered per switchable stage [kW]. Every reachable level is a
    /// whole multiple of this, which is what lets the MILP encode the tier as a
    /// single integer (see `heater_milp.rs`).
    pub fn p_step_kw(&self) -> f64 {
        self.max_kw / self.power_stages.max(1) as f64
    }

    pub fn from_params(cfg: &HeaterParams) -> Self {
        Self {
            max_kw: cfg.max_kw,
            power_stages: cfg.power_stages.max(1),
            temp_min_c: cfg.temp_min_c,
            temp_max_c: cfg.temp_max_c,
            temp_min_c_profile: cfg.temp_min_c,
            temp_max_c_profile: cfg.temp_max_c,
            temp_safety_max_c: cfg.temp_safety_max_c,
            emergency_mode: HeaterEmergencyMode::Normal,
            thermal_mass_kwh_per_c: cfg.thermal_mass_kwh_per_c,
            k_loss_kw_per_c: cfg.k_loss_kw_per_c,
            draw_kw: cfg.draw_kw,
            ambient_temp_c: 10.0,
        }
    }

    /// Apply this tick's Behaviour C sim-inject overrides (ambient temp, comfort band,
    /// emergency mode) — hold override or snap back to profile default. Called once per
    /// tick from `SimState::tick`, before physics runs.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_tick_overrides(
        &mut self,
        ambient_temp_c_override: Option<f64>,
        temp_min_override: Option<f64>,
        temp_max_override: Option<f64>,
        emergency_curtail_override: Option<bool>,
        emergency_absorb_override: Option<bool>,
    ) {
        self.ambient_temp_c = ambient_temp_c_override.unwrap_or(10.0);
        self.temp_min_c = temp_min_override.unwrap_or(self.temp_min_c_profile);
        self.temp_max_c = temp_max_override.unwrap_or(self.temp_max_c_profile);
        self.emergency_mode = HeaterEmergencyMode::from_overrides(
            emergency_curtail_override,
            emergency_absorb_override,
        );
    }

    pub fn initial_state(cfg: &HeaterParams) -> HeaterState {
        HeaterState {
            temperature_c: cfg.temp_initial_c,
            actual_power_kw: 0.0,
            emergency_latched: false,
        }
    }

    /// Pure physics step. Returns (new_state, actual_power_kw).
    /// Reads `self.ambient_temp_c` (set by sim loop each tick before calling).
    pub fn step_inner(
        &self,
        state: &HeaterState,
        setpoint_kw: f64,
        dt: Duration,
    ) -> (HeaterState, f64) {
        let dt_h = dt.num_milliseconds() as f64 / 3_600_000.0;
        // Quantize to the nearest reachable stage: k × p_step_kw. Each stage is
        // its own contactor, so intermediate values are physically impossible.
        let p_step = self.p_step_kw();
        let tier = if p_step > 0.0 {
            (setpoint_kw / p_step)
                .round()
                .clamp(0.0, self.power_stages.max(1) as f64)
                * p_step
        } else {
            0.0
        };
        let actual = self.thermostat_forced_kw(state).unwrap_or(tier);
        // Thermal model: Newton cooling + simulated draw
        let loss_kw = (state.temperature_c - self.ambient_temp_c) * self.k_loss_kw_per_c;
        let delta_c = (actual - loss_kw - self.draw_kw) / self.thermal_mass_kwh_per_c * dt_h;
        let new_temp = state.temperature_c + delta_c;
        let emergency_latched = self.emergency_active_in(state, self.emergency_mode)
            && new_temp < self.temp_min_c + Self::EMERGENCY_HYSTERESIS_C;
        (
            HeaterState {
                temperature_c: new_temp,
                actual_power_kw: actual,
                emergency_latched,
            },
            actual,
        )
    }

    /// Once the emergency fires at `temp_min_c`, it keeps running until this far above
    /// it, to prevent rapid relay cycling.
    const EMERGENCY_HYSTERESIS_C: f64 = 3.0;

    /// Is the thermostat's emergency heat running under `mode`: at/below `temp_min_c`,
    /// or still inside the hysteresis band of an emergency that already fired. Curtail
    /// suppresses it: drifting toward ambient below `temp_min_c` is then the desired
    /// response, not a fault to fight (§2 — no physical floor on this side).
    fn emergency_active_in(&self, state: &HeaterState, mode: HeaterEmergencyMode) -> bool {
        mode != HeaterEmergencyMode::Curtail
            && (state.temperature_c <= self.temp_min_c
                || (state.emergency_latched
                    && state.temperature_c < self.temp_min_c + Self::EMERGENCY_HYSTERESIS_C))
    }

    /// The power the thermostat forces regardless of setpoint, if any: off at
    /// the forced-off ceiling, full power in an emergency. The single rule
    /// `step_inner`, `capability_inner` and `flexibility_floor_inner` all read.
    fn thermostat_forced_kw(&self, state: &HeaterState) -> Option<f64> {
        self.thermostat_forced_kw_in(state, self.emergency_mode)
    }

    /// Same rule, evaluated as if `mode` were active — answers the arbiter's
    /// what-if questions ("heat forced without Curtail?", "room under Absorb?").
    fn thermostat_forced_kw_in(
        &self,
        state: &HeaterState,
        mode: HeaterEmergencyMode,
    ) -> Option<f64> {
        let emergency_active = self.emergency_active_in(state, mode);
        // Absorb mode relaxes the forced-off ceiling from temp_max_c to the true
        // safety ceiling temp_safety_max_c (§2).
        let safety_ceiling_c = if mode == HeaterEmergencyMode::Absorb {
            self.temp_safety_max_c
        } else {
            self.temp_max_c
        };
        if state.temperature_c >= safety_ceiling_c {
            Some(0.0)
        } else if emergency_active {
            Some(self.max_kw)
        } else {
            None
        }
    }

    /// Point-in-time feasible power range.
    pub fn capability_inner(&self, state: &HeaterState) -> AssetCapability {
        let max_import_kw = self.thermostat_forced_kw(state).unwrap_or(self.max_kw);
        AssetCapability {
            max_export_kw: 0.0,
            max_import_kw,
            adjustability: PowerAdjustability::Stepped,
            // Full physical stage set (0, p_step, 2·p_step, …) regardless of the
            // current temperature-driven ceiling above — a hardware fact, not a
            // live feasibility range.
            power_steps_kw: (0..=self.power_stages.max(1))
                .map(|k| k as f64 * self.p_step_kw())
                .collect(),
        }
    }

    /// Smallest nonzero achievable commitment. Hardware is a staged relay
    /// (0, p_step, … — see `step_inner`'s quantization), so unlike a
    /// continuously-controllable asset the floor while running is one stage,
    /// not 0. While the thermostat forces a value (off, or full-power
    /// emergency), min == max == that value, same as `capability_inner`.
    pub fn flexibility_floor_inner(&self, state: &HeaterState) -> AssetFlexibilityFloor {
        let min_import_kw = self
            .thermostat_forced_kw(state)
            .unwrap_or_else(|| self.p_step_kw());
        AssetFlexibilityFloor {
            min_export_kw: 0.0,
            min_import_kw,
        }
    }

    pub fn default_setpoint(&self) -> f64 {
        // Off between plan slots; thermostat emergency and plan allocations turn it on.
        0.0
    }

    pub fn state_values(&self, state: &HeaterState) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        m.insert("temp_c".into(), state.temperature_c);
        m.insert("max_kw".into(), self.max_kw);
        m.insert("power_stages".into(), self.power_stages as f64);
        m.insert("p_step_kw".into(), self.p_step_kw());
        m.insert("temp_min_c".into(), self.temp_min_c);
        m.insert("temp_max_c".into(), self.temp_max_c);
        m.insert("temp_safety_max_c".into(), self.temp_safety_max_c);
        m.insert("thermal_mass_kwh_per_c".into(), self.thermal_mass_kwh_per_c);
        m.insert(
            "emergency_curtail".into(),
            (self.emergency_mode == HeaterEmergencyMode::Curtail) as u8 as f64,
        );
        m.insert(
            "emergency_absorb".into(),
            (self.emergency_mode == HeaterEmergencyMode::Absorb) as u8 as f64,
        );
        // The heater's own answers to the arbiter's emergency-mode what-ifs:
        // heat its thermostat forces unless curtailed, and room under Absorb.
        let emergency_heat_kw = self
            .thermostat_forced_kw_in(state, HeaterEmergencyMode::Normal)
            .unwrap_or(0.0);
        m.insert("emergency_heat_kw".into(), emergency_heat_kw);
        let absorb_headroom_kw = self
            .thermostat_forced_kw_in(state, HeaterEmergencyMode::Absorb)
            .unwrap_or(self.max_kw);
        m.insert("absorb_headroom_kw".into(), absorb_headroom_kw);
        m
    }

    /// State values for a future MILP time slot, given the thermal energy stored
    /// above `temp_min_c` at the start of that slot (kWh).
    /// Returns `{"temp_c": <temperature>}`.
    #[allow(dead_code)] // pre-existing, unrelated to Spec A: asset_port.rs::heater_future_state is a separate "Mirrors" reimplementation; found while removing AssetConfig, not fixed here (R-73)
    pub fn future_state_values(&self, e_tank_kwh: f64) -> HashMap<String, f64> {
        let temp_c = crate::entities::asset_params::heater_temp_c_from_energy(
            e_tank_kwh,
            self.temp_min_c,
            self.thermal_mass_kwh_per_c,
        );
        HashMap::from([("temp_c".into(), temp_c)])
    }

    /// Create a plan trajectory starting from the current live state.
    /// Returns `None` if `live_state` is not a heater state.
    pub fn plan_trajectory(
        cfg: &Self,
        live_state: &super::AssetState,
    ) -> Option<HeaterPlanTrajectory> {
        if let super::AssetState::Heater(s) = live_state {
            use crate::entities::asset_params::heater_energy_above_min_kwh;
            let e_max_kwh = heater_energy_above_min_kwh(
                cfg.temp_max_c,
                cfg.temp_min_c,
                cfg.thermal_mass_kwh_per_c,
            );
            let e_kwh = heater_energy_above_min_kwh(
                s.temperature_c,
                cfg.temp_min_c,
                cfg.thermal_mass_kwh_per_c,
            )
            .clamp(0.0, e_max_kwh);
            Some(HeaterPlanTrajectory {
                e_kwh,
                temp_min_c: cfg.temp_min_c,
                thermal_mass: cfg.thermal_mass_kwh_per_c,
                q_dem_kw: cfg.forecast_demand_kw(cfg.ambient_temp_c),
                e_max_kwh,
            })
        } else {
            None
        }
    }

    // control_schema() moved to heater_control_schema.rs (Spec A Phase 2a,
    // file-size split) — still `impl Heater`, just a separate block/file.

    pub fn reset(&self, state: &mut HeaterState, values: HashMap<String, f64>) {
        if let Some(&t) = values.get("temp_c") {
            state.temperature_c = t;
        }
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        if let Some(&v) = values.get("max_kw") {
            self.max_kw = v.max(0.0);
        }
    }

    pub fn forecast(
        &self,
        state: &HeaterState,
        timespan: Duration,
        now: DateTime<Utc>,
    ) -> TimeSeries {
        if timespan <= Duration::zero() {
            return TimeSeries::empty(Interpolation::Linear);
        }
        let end = now + timespan;
        // Simulate uncontrolled thermostat operation (no plan overlay, setpoint = 0)
        // through the same step the live tick uses, so the forecast's thermostat is
        // the real one (emergency at T_min, its hysteresis, cutoff at T_max) rather
        // than a second copy of the rule.
        let setpoint = self.default_setpoint();
        let mut samples: Vec<(DateTime<Utc>, f64)> = Vec::new();

        let mut t = now;
        let mut s = state.clone();

        while t < end {
            let (next, kw) = self.step_inner(&s, setpoint, Duration::seconds(60));
            samples.push((t, kw));
            s = next;
            t += Duration::seconds(60);
        }
        let end_kw = self.step_inner(&s, setpoint, Duration::zero()).1;
        samples.push((end, end_kw));

        TimeSeries {
            samples,
            interpolation: Interpolation::Linear,
        }
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        vec![
            crate::entities::asset::ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.30,
                max_marginal_co2: 0.0,
            },
            crate::entities::asset::ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.10,
                max_marginal_co2: 0.0,
            },
        ]
    }

    pub fn default_completion_policy(&self) -> crate::entities::asset::CompletionPolicy {
        crate::entities::asset::CompletionPolicy::Continue
    }

    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        Some(0.10)
    }

    /// Constant per-step thermal demand forecast [kW].
    /// Uses the midpoint of the comfort band as the representative tank temperature.
    /// `Q_dem = draw_kw + k_loss × (T_mid − ambient_temp_c)`
    pub fn forecast_demand_kw(&self, ambient_temp_c: f64) -> f64 {
        let t_mid = (self.temp_min_c + self.temp_max_c) / 2.0;
        (self.draw_kw + self.k_loss_kw_per_c * (t_mid - ambient_temp_c)).max(0.0)
    }
}

impl Asset for Heater {
    fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        let AssetState::Heater(s) = state else {
            unreachable!("Heater/state mismatch")
        };
        let (ns, p) = self.step_inner(s, setpoint_kw, dt);
        (AssetState::Heater(ns), p)
    }

    fn capability(&self, state: &AssetState) -> AssetCapability {
        let AssetState::Heater(s) = state else {
            unreachable!()
        };
        self.capability_inner(s)
    }

    fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor {
        let AssetState::Heater(s) = state else {
            unreachable!()
        };
        self.flexibility_floor_inner(s)
    }

    fn forced_power_kw(&self, state: &AssetState) -> Option<f64> {
        let AssetState::Heater(s) = state else {
            unreachable!()
        };
        self.thermostat_forced_kw(s)
    }

    fn default_setpoint(&self) -> f64 {
        Self::default_setpoint(self)
    }

    fn control_schema(&self) -> Vec<ControlDescriptor> {
        Self::control_schema(self)
    }

    fn update_config(&mut self, values: HashMap<String, f64>) {
        Self::update_config(self, values)
    }

    fn default_comfort_rates(&self) -> Vec<ComfortRate> {
        Self::default_comfort_rates(self)
    }

    fn default_completion_policy(&self) -> CompletionPolicy {
        Self::default_completion_policy(self)
    }

    fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        Self::default_post_deadline_comfort_bid(self)
    }

    fn state_values(&self, state: &AssetState) -> HashMap<String, f64> {
        let AssetState::Heater(s) = state else {
            unreachable!("Heater/state mismatch")
        };
        Self::state_values(self, s)
    }

    fn reset(&self, state: &mut AssetState, values: HashMap<String, f64>) {
        let AssetState::Heater(s) = state else {
            unreachable!("Heater/state mismatch")
        };
        Self::reset(self, s, values)
    }

    fn forecast(&self, state: &AssetState, timespan: Duration, now: DateTime<Utc>) -> TimeSeries {
        let AssetState::Heater(s) = state else {
            unreachable!("Heater/state mismatch")
        };
        Self::forecast(self, s, timespan, now)
    }

    fn as_milp_participant(&self) -> Option<&dyn MilpParticipant> {
        Some(self)
    }

    fn as_thermostat(&self) -> Option<&dyn Thermostat> {
        Some(self)
    }

    fn asset_type(&self) -> crate::entities::asset::AssetType {
        crate::entities::asset::AssetType::Heater
    }

    fn asset_type_str(&self) -> &'static str {
        "heater"
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

    fn as_tick_overridable(&mut self) -> Option<&mut dyn TickOverridable> {
        Some(self)
    }
}

// TickOverridable/MilpParticipant/Thermostat for Heater moved to
// heater_capabilities.rs (Spec A Phase 2b, file-size split).

#[cfg(test)]
mod tests {
    use super::*;

    fn default_heater() -> Heater {
        Heater {
            max_kw: 2.5,
            power_stages: 2,
            temp_min_c: 20.0,
            temp_max_c: 23.0,
            temp_min_c_profile: 20.0,
            temp_max_c_profile: 23.0,
            temp_safety_max_c: 23.0,
            emergency_mode: HeaterEmergencyMode::Normal,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            ambient_temp_c: 10.0,
        }
    }

    /// Hot water tank fixture: 200 L, 40–80 °C comfort band, low heat loss, 0.5 kW draw.
    /// Safety ceiling 90 °C, matching `ven-2.yaml`'s comfort/safety split
    /// (see `docs/architecture/VEN_ARCHITECTURE.md`'s Heater section).
    fn hot_water_heater() -> Heater {
        Heater {
            max_kw: 6.0,
            power_stages: 2,
            temp_min_c: 40.0,
            temp_max_c: 80.0,
            temp_min_c_profile: 40.0,
            temp_max_c_profile: 80.0,
            temp_safety_max_c: 90.0,
            emergency_mode: HeaterEmergencyMode::Normal,
            thermal_mass_kwh_per_c: 200.0 * 4.186 / 3600.0, // ≈ 0.233 kWh/°C
            k_loss_kw_per_c: 0.003,
            draw_kw: 0.5,
            ambient_temp_c: 20.0,
        }
    }

    fn state_at(temperature_c: f64, actual_power_kw: f64) -> HeaterState {
        HeaterState {
            temperature_c,
            actual_power_kw,
            emergency_latched: false,
        }
    }

    /// A heater whose thermostat emergency fired earlier and is still inside
    /// its hysteresis band, running at full power.
    fn latched_state_at(heater: &Heater, temperature_c: f64) -> HeaterState {
        HeaterState {
            temperature_c,
            actual_power_kw: heater.max_kw,
            emergency_latched: true,
        }
    }

    // ── GB-44: emergency latch vs a planner-commanded top stage ─────────────

    #[test]
    fn step_inner_full_stage_commanded_in_comfort_band_does_not_latch() {
        // ven-10 in the 2026-08-31 S-7 run: the planner ran the heater at full
        // power at 19.8-20.4 °C (floor 18 °C), then turned it off for a capacity
        // limit — the thermostat must not treat "was at full power" as an
        // emergency and keep it on.
        let heater = hot_water_heater(); // temp_min 40, max 6.0, hysteresis band 40-43
        let (running, kw) =
            heater.step_inner(&state_at(41.0, 0.0), heater.max_kw, Duration::seconds(1));
        assert_eq!(kw, heater.max_kw);
        assert!(
            !running.emergency_latched,
            "a commanded top stage is not an emergency"
        );

        let (_off, kw) = heater.step_inner(&running, 0.0, Duration::seconds(1));
        assert_eq!(kw, 0.0, "the next plan setpoint must be followed");
    }

    #[test]
    fn step_inner_emergency_latches_until_min_plus_hysteresis() {
        let heater = hot_water_heater(); // temp_min 40, band 40-43
        let (fired, kw) = heater.step_inner(&state_at(39.9, 0.0), 0.0, Duration::seconds(1));
        assert_eq!(kw, heater.max_kw, "at/below temp_min the emergency fires");
        assert!(fired.emergency_latched);

        let (held, kw) =
            heater.step_inner(&latched_state_at(&heater, 42.0), 0.0, Duration::seconds(1));
        assert_eq!(
            kw, heater.max_kw,
            "inside the band a fired emergency keeps running"
        );
        assert!(held.emergency_latched);

        let (released, kw) =
            heater.step_inner(&latched_state_at(&heater, 43.0), 0.0, Duration::seconds(1));
        assert_eq!(kw, 0.0, "at temp_min + 3 °C the setpoint takes over again");
        assert!(!released.emergency_latched);
    }

    #[test]
    fn step_inner_curtail_clears_the_latch() {
        let mut heater = hot_water_heater();
        heater.emergency_mode = HeaterEmergencyMode::Curtail;
        let (next, kw) =
            heater.step_inner(&latched_state_at(&heater, 41.0), 0.0, Duration::seconds(1));
        assert_eq!(kw, 0.0);
        assert!(
            !next.emergency_latched,
            "a curtailed emergency is no longer running"
        );
    }

    #[test]
    fn heater_state_without_latch_field_deserialises_unlatched() {
        // sim_state.json persisted before GB-44 has no emergency_latched field.
        let state: HeaterState =
            serde_json::from_str(r#"{"temperature_c":41.0,"actual_power_kw":6.0}"#).unwrap();
        assert!(!state.emergency_latched);
    }

    #[test]
    fn state_values_exposes_thermal_mass_kwh_per_c() {
        let h = default_heater();
        let vals = h.state_values(&state_at(21.0, 0.0));
        assert_eq!(vals.get("thermal_mass_kwh_per_c"), Some(&2.0));
    }

    // ── capability (BL-27) ───────────────────────────────────────────────────

    #[test]
    fn capability_reports_stepped_adjustability_with_three_tiers() {
        let heater = default_heater(); // p_step=1.25, max_kw=2.5
        let cap = heater.capability_inner(&state_at(21.5, 0.0)); // normal band
        assert_eq!(cap.adjustability, PowerAdjustability::Stepped);
        assert_eq!(cap.power_steps_kw, vec![0.0, 1.25, 2.5]);
    }

    #[test]
    fn capability_power_steps_kw_unaffected_by_overheat_or_too_cold_ceiling() {
        // power_steps_kw is the hardware's physical tier set — it doesn't
        // collapse when the live ceiling (max_import_kw) does.
        let heater = default_heater();
        let overheated = heater.capability_inner(&state_at(23.5, 0.0));
        assert_eq!(overheated.max_import_kw, 0.0);
        assert_eq!(overheated.power_steps_kw, vec![0.0, 1.25, 2.5]);

        let too_cold = heater.capability_inner(&state_at(19.0, 0.0));
        assert_eq!(too_cold.max_import_kw, heater.max_kw);
        assert_eq!(too_cold.power_steps_kw, vec![0.0, 1.25, 2.5]);
    }

    #[test]
    fn capability_power_steps_kw_follows_power_stages() {
        // GB-40: the reachable set is k × max_kw/power_stages, so it is derived
        // rather than configured — a profile can no longer name a level the
        // hardware cannot reach.
        let mut heater = default_heater(); // max_kw = 2.5
        heater.power_stages = 2;
        let cap = heater.capability_inner(&state_at(21.5, 0.0));
        assert_eq!(cap.power_steps_kw, vec![0.0, 1.25, 2.5]);

        heater.power_stages = 1;
        let cap = heater.capability_inner(&state_at(21.5, 0.0));
        assert_eq!(cap.power_steps_kw, vec![0.0, 2.5]);
    }

    /// What `step_inner` actually draws when asked for full power right now.
    fn stepped_kw(heater: &Heater, state: &HeaterState) -> f64 {
        heater.step_inner(state, heater.max_kw, Duration::zero()).1
    }

    #[test]
    fn capability_and_floor_match_step_when_too_cold() {
        // Emergency thermostat forces full power at temp_min_c; capability must
        // report that, not a "forced on at 0 kW" ceiling.
        let heater = default_heater(); // temp_min_c=20, max_kw=2.5
        let state = state_at(19.0, 0.0);
        let cap = heater.capability_inner(&state);
        let floor = heater.flexibility_floor_inner(&state);
        assert_eq!(cap.max_import_kw, stepped_kw(&heater, &state));
        assert_eq!(cap.max_import_kw, heater.max_kw);
        assert_eq!(floor.min_import_kw, heater.max_kw);
    }

    #[test]
    fn capability_and_floor_match_step_during_emergency_hysteresis() {
        // Above temp_min_c but still within the 3 °C hysteresis of an emergency
        // that already fired: step keeps it forced on, so min == max == max_kw.
        let heater = default_heater();
        let state = latched_state_at(&heater, 21.0);
        assert_eq!(
            heater.capability_inner(&state).max_import_kw,
            stepped_kw(&heater, &state)
        );
        assert_eq!(
            heater.flexibility_floor_inner(&state).min_import_kw,
            heater.max_kw
        );
    }

    #[test]
    fn forced_power_kw_reports_what_the_thermostat_forces() {
        let heater = default_heater(); // temp_min_c=20, temp_max_c=23, max_kw=2.5
        let forced = |temp_c, last_kw| {
            Asset::forced_power_kw(&heater, &AssetState::Heater(state_at(temp_c, last_kw)))
        };
        let latched = Asset::forced_power_kw(
            &heater,
            &AssetState::Heater(latched_state_at(&heater, 21.0)),
        );
        assert_eq!(forced(19.0, 0.0), Some(2.5), "too cold: emergency heat");
        assert_eq!(
            latched,
            Some(2.5),
            "fired emergency still within hysteresis"
        );
        assert_eq!(
            forced(21.0, 2.5),
            None,
            "full power the planner commanded is not an emergency (GB-44)"
        );
        assert_eq!(forced(21.5, 0.0), None, "normal band follows the setpoint");
        assert_eq!(forced(23.5, 0.0), Some(0.0), "overheated: forced off");
    }

    #[test]
    fn state_values_answer_the_emergency_mode_what_ifs() {
        // The arbiter's emergency lever asks: how much emergency heat would run
        // without Curtail, and how much could Absorb take? Both are answered here,
        // including while Curtail itself is active (the lever must stay offered).
        let mut heater = default_heater(); // temp_min 20, temp_max 23, safety 23, max 2.5
        heater.emergency_mode = HeaterEmergencyMode::Curtail;
        let cold = heater.state_values(&state_at(19.0, 0.0));
        assert_eq!(cold.get("emergency_heat_kw"), Some(&2.5));
        let mid = heater.state_values(&state_at(21.5, 0.0));
        assert_eq!(mid.get("emergency_heat_kw"), Some(&0.0));
        assert_eq!(mid.get("absorb_headroom_kw"), Some(&2.5));
        let hot = heater.state_values(&state_at(23.0, 0.0));
        assert_eq!(hot.get("absorb_headroom_kw"), Some(&0.0));
    }

    #[test]
    fn capability_uses_the_safety_ceiling_in_absorb_mode() {
        // Absorb relaxes the forced-off ceiling from temp_max_c to temp_safety_max_c.
        let mut heater = hot_water_heater(); // temp_max_c=80, temp_safety_max_c=90
        heater.emergency_mode = HeaterEmergencyMode::Absorb;
        let state = state_at(85.0, 0.0);
        assert_eq!(heater.capability_inner(&state).max_import_kw, heater.max_kw);
        assert_eq!(
            heater.capability_inner(&state).max_import_kw,
            stepped_kw(&heater, &state)
        );
    }

    // ── flexibility_floor ─────────────────────────────────────────────────────

    #[test]
    fn flexibility_floor_uses_one_stage_in_normal_band() {
        let heater = default_heater(); // p_step=1.25, temp_min_c=20, temp_max_c=23
        let floor = heater.flexibility_floor_inner(&state_at(21.5, 0.0));
        assert_eq!(floor.min_import_kw, 1.25);
        assert_eq!(floor.min_export_kw, 0.0);
    }

    #[test]
    fn flexibility_floor_is_full_power_for_a_single_stage_element() {
        // With one stage there is no partial level, so the floor while running
        // is the whole element.
        let mut heater = default_heater();
        heater.power_stages = 1;
        let floor = heater.flexibility_floor_inner(&state_at(21.5, 0.0));
        assert_eq!(floor.min_import_kw, heater.max_kw);
    }

    #[test]
    fn flexibility_floor_is_zero_when_overheated() {
        let heater = default_heater(); // temp_max_c=23
        let floor = heater.flexibility_floor_inner(&state_at(23.0, 0.0));
        assert_eq!(
            floor.min_import_kw, 0.0,
            "must match capability_inner's forced-off ceiling"
        );
        assert_eq!(floor.min_export_kw, 0.0);
    }

    #[test]
    fn flexibility_floor_is_full_power_when_too_cold() {
        let heater = default_heater(); // temp_min_c=20
        let floor = heater.flexibility_floor_inner(&state_at(20.0, 0.0));
        assert_eq!(
            floor.min_import_kw, heater.max_kw,
            "must match capability_inner's forced-on (emergency) ceiling, not one stage"
        );
    }

    // ── control_schema ────────────────────────────────────────────────────────

    #[test]
    fn control_schema_returns_six_descriptors() {
        let heater = default_heater();
        let schema = heater.control_schema();
        let keys: Vec<_> = schema.iter().map(|d| d.key.as_str()).collect();
        assert!(keys.contains(&"heater_temp_c"), "missing heater_temp_c");
        assert!(
            keys.contains(&"heater_setpoint_c"),
            "missing heater_setpoint_c"
        );
        assert!(
            keys.contains(&"heater_temp_min_c"),
            "missing heater_temp_min_c"
        );
        assert!(
            keys.contains(&"heater_temp_max_c"),
            "missing heater_temp_max_c"
        );
        assert!(
            keys.contains(&"heater_emergency_curtail"),
            "missing heater_emergency_curtail"
        );
        assert!(
            keys.contains(&"heater_emergency_absorb"),
            "missing heater_emergency_absorb"
        );
        assert_eq!(schema.len(), 6, "expected exactly 6 control descriptors");
    }

    #[test]
    fn control_schema_heater_setpoint_bounds() {
        // heater_setpoint_c is a comfort-target temperature (compared against
        // temp_c by the dispatcher — see build_setpoints), not a power value:
        // its schema must describe °C over the same range as T_tank, and be
        // nullable so "no override active" renders as "Off" rather than a
        // numeric value indistinguishable from an active override at that value.
        let heater = default_heater();
        let schema = heater.control_schema();
        let sp_d = schema
            .iter()
            .find(|d| d.key == "heater_setpoint_c")
            .unwrap();
        let temp_d = schema.iter().find(|d| d.key == "heater_temp_c").unwrap();
        assert_eq!(sp_d.min.unwrap(), 18.0);
        assert_eq!(sp_d.max.unwrap(), 95.0);
        assert_eq!(sp_d.unit, "°C");
        assert!(sp_d.nullable, "heater_setpoint_c must be nullable");
        assert_eq!(temp_d.min.unwrap(), 18.0);
        assert_eq!(temp_d.max.unwrap(), 95.0);
    }

    #[test]
    fn control_schema_t_tank_bounds_are_18_to_95() {
        let heater = default_heater();
        let schema = heater.control_schema();
        let min_d = schema
            .iter()
            .find(|d| d.key == "heater_temp_min_c")
            .unwrap();
        let max_d = schema
            .iter()
            .find(|d| d.key == "heater_temp_max_c")
            .unwrap();
        assert_eq!(min_d.min.unwrap(), 18.0);
        assert_eq!(min_d.max.unwrap(), 94.0);
        assert_eq!(max_d.min.unwrap(), 19.0);
        assert_eq!(max_d.max.unwrap(), 95.0);
        assert_eq!(min_d.label, "T_tank_min");
        assert_eq!(max_d.label, "T_tank_max");
    }

    // ── forecast ─────────────────────────────────────────────────────────────

    /// When the heater is at temp_max (thermostat forced off), the forecast simulates
    /// thermostat-only operation (setpoint=0). The tank cools to T_min, the emergency
    /// fires at max_kw, and the cycle repeats — average power ≈ heat loss at T_min.
    #[test]
    fn forecast_at_temp_max_gives_non_zero_average_power() {
        let heater = default_heater();
        let state = state_at(23.0, 0.0);
        // thermal_mass=2.0 kWh/°C → τ=20h; T drops from 23→20°C in ~5h.
        // Use 24h to ensure full thermostat cycling is captured.
        let ts = heater.forecast(&state, Duration::hours(24), Utc::now());

        // Compute mean power over the forecast samples
        let n = ts.samples.len() as f64;
        assert!(n > 0.0, "forecast produced no samples");
        let mean: f64 = ts.samples.iter().map(|(_, kw)| kw).sum::<f64>() / n;

        // Thermostat cycles near T_min=20°C → heat loss ≈ 0.1×(20-10) = 1.0 kW.
        // Allow ±0.5 kW tolerance for simulation step error.
        assert!(
            mean > 0.5,
            "forecast mean {mean:.3} kW is too close to 0 — old bug likely present",
        );
        assert!(
            mean < 2.5,
            "forecast mean {mean:.3} kW exceeds max_kw — something is wrong",
        );
    }

    /// When actual_power_kw is already non-zero, both old and new code produce
    /// similar results, but new code is consistent.
    #[test]
    fn forecast_at_mid_temp_gives_reasonable_oscillation() {
        let heater = default_heater();
        let state = state_at(21.5, 1.3);
        // thermal_mass=2.0 kWh/°C → T drops from 21.5→20°C (T_min) in ~2.8h.
        // Use 12h to ensure cycling is captured in the mean.
        let ts = heater.forecast(&state, Duration::hours(12), Utc::now());
        let n = ts.samples.len() as f64;
        assert!(n > 0.0);
        let mean: f64 = ts.samples.iter().map(|(_, kw)| kw).sum::<f64>() / n;
        // Expect long-run equilibrium in reasonable range
        assert!(
            (0.5..=2.5).contains(&mean),
            "mean {mean:.3} kW out of range"
        );
    }

    // ── step_inner physics ────────────────────────────────────────────────────

    #[test]
    fn heater_turns_off_above_temp_max() {
        let heater = default_heater();
        let state = state_at(23.1, 2.5);
        let (_ns, power) = heater.step_inner(&state, 2.5, Duration::seconds(1));
        assert_eq!(power, 0.0, "heater must be forced off above temp_max");
    }

    #[test]
    fn heater_turns_on_below_temp_min() {
        let heater = default_heater();
        let state = state_at(19.9, 0.0);
        let (_ns, power) = heater.step_inner(&state, 1.0, Duration::seconds(1));
        assert_eq!(
            power, heater.max_kw,
            "heater must run at max_kw below temp_min"
        );
    }

    #[test]
    fn heater_follows_setpoint_in_comfort_band() {
        let heater = default_heater();
        let state = state_at(21.5, 0.0);
        let setpoint = 1.5;
        let (_ns, power) = heater.step_inner(&state, setpoint, Duration::seconds(1));
        // Relay quantization: 1.5 / 1.25 rounds to stage 1, i.e. 1.25 kW.
        // Exact passthrough is not possible on a staged element.
        assert!(
            (power - heater.p_step_kw()).abs() < 1e-9,
            "heater should snap setpoint 1.5 to one stage ({}), got {power}",
            heater.p_step_kw()
        );
    }

    // ── emergency safety envelope (see docs/architecture/VEN_ARCHITECTURE.md) ─────

    #[test]
    fn curtail_mode_suppresses_emergency_heat_below_temp_min() {
        let mut heater = default_heater(); // temp_min_c=20
        heater.emergency_mode = HeaterEmergencyMode::Curtail;
        let state = state_at(19.9, 0.0); // below temp_min, would normally force max_kw
        let (_ns, power) = heater.step_inner(&state, 0.0, Duration::seconds(1));
        assert_eq!(
            power, 0.0,
            "curtail mode must let the tank drift below temp_min_c instead of forcing heat"
        );
    }

    #[test]
    fn curtail_mode_still_forces_off_above_temp_max() {
        let mut heater = default_heater(); // temp_max_c=23
        heater.emergency_mode = HeaterEmergencyMode::Curtail;
        let state = state_at(23.1, 2.5);
        let (_ns, power) = heater.step_inner(&state, 2.5, Duration::seconds(1));
        assert_eq!(
            power, 0.0,
            "curtail mode must not relax the temp_max_c ceiling"
        );
    }

    #[test]
    fn absorb_mode_heats_past_temp_max_up_to_safety_ceiling() {
        let mut heater = hot_water_heater(); // temp_max_c=80, temp_safety_max_c=90
        heater.emergency_mode = HeaterEmergencyMode::Absorb;
        let state = state_at(85.0, 6.0); // above comfort ceiling, below safety ceiling
        let (_ns, power) = heater.step_inner(&state, 6.0, Duration::seconds(1));
        assert!(
            power > 0.0,
            "absorb mode must keep heating above temp_max_c and below temp_safety_max_c"
        );
    }

    #[test]
    fn absorb_mode_still_forces_off_above_safety_ceiling() {
        let mut heater = hot_water_heater(); // temp_safety_max_c=90
        heater.emergency_mode = HeaterEmergencyMode::Absorb;
        let state = state_at(90.1, 6.0);
        let (_ns, power) = heater.step_inner(&state, 6.0, Duration::seconds(1));
        assert_eq!(
            power, 0.0,
            "absorb mode must still force off above the true safety ceiling"
        );
    }

    #[test]
    fn absorb_mode_still_forces_emergency_heat_below_temp_min() {
        let mut heater = hot_water_heater(); // temp_min_c=40
        heater.emergency_mode = HeaterEmergencyMode::Absorb;
        let state = state_at(39.9, 0.0);
        let (_ns, power) = heater.step_inner(&state, 0.0, Duration::seconds(1));
        assert_eq!(
            power, heater.max_kw,
            "absorb mode must not relax the temp_min_c emergency floor"
        );
    }

    #[test]
    fn normal_mode_unaffected_by_new_fields() {
        // Regression guard: temp_safety_max_c defaults equal temp_max_c and
        // emergency_mode defaults to Normal, so behaviour must be byte-for-byte
        // identical to before this feature existed.
        let heater = default_heater();
        assert_eq!(heater.emergency_mode, HeaterEmergencyMode::Normal);
        assert_eq!(heater.temp_safety_max_c, heater.temp_max_c);
        let state = state_at(23.1, 2.5);
        let (_ns, power) = heater.step_inner(&state, 2.5, Duration::seconds(1));
        assert_eq!(power, 0.0);
    }

    // ── hot water tank physics ────────────────────────────────────────────────

    #[test]
    fn hwt_uses_configurable_k_loss() {
        // k_loss = 0.003 kW/°C; at 60°C ambient=20°C → loss = (60-20)*0.003 = 0.12 kW
        let heater = hot_water_heater();
        let state = state_at(60.0, 0.0);
        // setpoint = 0 → heater off (in comfort band 40–80°C)
        let (new_state, power) = heater.step_inner(&state, 0.0, Duration::seconds(3600));
        assert_eq!(power, 0.0);
        // In 1 h at 0 kW, 0.12 kW draw subtracted: net = 0 - 0.12 - 0.5 = -0.62 kW
        // delta_c = -0.62 / 0.233 = -2.66 °C  (roughly)
        let expected_loss = (60.0 - 20.0) * 0.003 + 0.5; // loss + draw
        let expected_delta = -expected_loss / (200.0 * 4.186 / 3600.0);
        let actual_delta = new_state.temperature_c - 60.0;
        assert!(
            (actual_delta - expected_delta).abs() < 0.01,
            "k_loss or draw physics wrong: got Δ{:.3}°C, expected Δ{:.3}°C",
            actual_delta,
            expected_delta
        );
    }

    #[test]
    fn hwt_draw_drains_tank_when_off() {
        // With 0.5 kW draw and no heater, tank should cool faster than without draw.
        let heater = hot_water_heater();
        let no_draw = Heater {
            draw_kw: 0.0,
            ..hot_water_heater()
        };
        let state = state_at(60.0, 0.0);
        let dt = Duration::seconds(3600);
        let (s_with_draw, _) = heater.step_inner(&state, 0.0, dt);
        let (s_no_draw, _) = no_draw.step_inner(&state, 0.0, dt);
        assert!(
            s_with_draw.temperature_c < s_no_draw.temperature_c,
            "draw should cause faster cooling"
        );
    }

    #[test]
    fn hwt_heats_slowly_with_low_k_loss() {
        // With k_loss=0.003, a 3 kW heater at 60°C and 20°C ambient
        // should heat the 0.233 kWh/°C tank by ~ (3 - 0.12 - 0.5) * 1h / 0.233 ≈ 10.2°C/h
        let heater = hot_water_heater();
        let state = state_at(60.0, 3.0);
        let (new_state, _) = heater.step_inner(&state, 3.0, Duration::seconds(3600));
        let delta = new_state.temperature_c - 60.0;
        assert!(
            delta > 5.0 && delta < 20.0,
            "tank should heat 5–20°C in 1h with 3kW; got {:.2}°C",
            delta
        );
    }

    #[test]
    fn hwt_emergency_on_below_temp_min() {
        let heater = hot_water_heater();
        let state = state_at(39.9, 0.0); // just below min (40°C)
        let (_ns, power) = heater.step_inner(&state, 0.0, Duration::seconds(1));
        assert_eq!(
            power, heater.max_kw,
            "emergency: must run at max below temp_min"
        );
    }

    #[test]
    fn hwt_forced_off_above_temp_max() {
        let heater = hot_water_heater();
        let state = state_at(80.1, 3.0);
        let (_ns, power) = heater.step_inner(&state, 3.0, Duration::seconds(1));
        assert_eq!(power, 0.0, "must be forced off above temp_max");
    }

    #[test]
    fn forecast_demand_kw_equals_draw_plus_loss_at_midpoint() {
        // forecast_demand_kw(ambient) = draw_kw + k_loss × (T_mid − ambient)
        // T_mid = (40+80)/2 = 60; ambient = 20; draw = 0.5; k_loss = 0.003
        // expected: 0.5 + 0.003 × (60 − 20) = 0.62 kW
        let heater = hot_water_heater();
        let q_dem = heater.forecast_demand_kw(20.0);
        assert!((q_dem - 0.62).abs() < 1e-6, "q_dem={q_dem:.4} != 0.62");
    }

    #[test]
    fn forecast_demand_kw_clamped_at_zero_when_ambient_above_tank() {
        // If ambient > T_mid, loss is negative; result must not go negative.
        let heater = hot_water_heater(); // draw=0.5, k_loss=0.003, T_mid=60
        let q_dem = heater.forecast_demand_kw(80.0); // ambient well above T_mid
                                                     // draw 0.5 + 0.003×(60-80) = 0.5 - 0.06 = 0.44 → positive; still ≥ 0
        assert!(q_dem >= 0.0, "q_dem must be non-negative, got {q_dem}");
    }

    // T016: Heater::future_state_values returns correct temp_c.
    #[test]
    fn future_state_values_mid_energy() {
        let h = default_heater(); // thermal_mass_kwh_per_c = 2.0, temp_min_c = 20.0
                                  // 2.0 kWh stored → temp = 20.0 + 2.0 / 2.0 = 21.0 °C
        let vals = h.future_state_values(2.0);
        let temp_c = vals["temp_c"];
        assert!(
            (temp_c - 21.0).abs() < 1e-9,
            "expected temp_c=21.0, got {temp_c}"
        );
    }

    #[test]
    fn future_state_values_zero_energy() {
        let h = default_heater();
        let vals = h.future_state_values(0.0);
        assert!((vals["temp_c"] - h.temp_min_c).abs() < 1e-9);
    }

    #[test]
    fn future_state_values_returns_only_temp_c() {
        let h = default_heater();
        let vals = h.future_state_values(1.0);
        assert_eq!(vals.len(), 1, "expected exactly one key");
        assert!(vals.contains_key("temp_c"));
    }
}

#[cfg(test)]
mod param_tests {
    use super::*;

    #[test]
    fn heater_params_defaults() {
        let params = HeaterParams::default();
        assert!((params.max_kw - 5.0).abs() < f64::EPSILON);
        assert_eq!(params.power_stages, 2);
    }

    #[test]
    fn heater_stage_size_is_derived_from_power_stages() {
        // GB-40: the mid level is no longer a free field, so it cannot disagree
        // with max_kw — it is always max_kw / power_stages.
        let two = Heater::from_params(&HeaterParams {
            max_kw: 5.0,
            power_stages: 2,
            ..HeaterParams::default()
        });
        assert!((two.p_step_kw() - 2.5).abs() < 1e-9);

        let one = Heater::from_params(&HeaterParams {
            max_kw: 5.0,
            power_stages: 1,
            ..HeaterParams::default()
        });
        assert!((one.p_step_kw() - 5.0).abs() < 1e-9);
    }
}
