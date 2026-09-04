mod asset_trait;
pub mod base_load;
pub mod battery;
mod battery_milp;
pub mod ev;
mod ev_comfort;
mod ev_milp;
pub mod grid;
pub mod heater;
mod heater_capabilities;
mod heater_control_schema;
mod heater_emergency;
mod heater_milp;
mod history;
pub mod pv;

// AssetHandle/TrajectoryPoint are consumed only within asset_trait's own tests — same
// bin-crate "pub items have no external consumer" situation AssetHandle was already
// #[allow(dead_code)]'d for before this file split.
#[allow(unused_imports)]
pub use asset_trait::{
    Asset, AssetHandle, MilpParticipant, RequestResolvable, Thermostat, TickOverridable,
    TickOverrides, Trajectory, TrajectoryPoint,
};
pub use base_load::{BaseLoad, BaseLoadState};
pub use battery::{Battery, BatteryState};
pub use ev::{EvCharger, EvState};
pub use grid::Grid;
pub use heater::{Heater, HeaterState};
pub use history::{AssetHistoryBuffer, HistoryPoint};
pub use pv::{PvInverter, PvPowerInputs, PvState};

// ─── Input type for a runtime-controllable parameter ─────────────────────────

/// Input type for a runtime-controllable parameter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Slider,
    Switch,
    NumberInput,
}

/// Descriptor for one controllable parameter exposed via GET /sim/schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ControlDescriptor {
    pub key: String,
    pub label: String,
    pub kind: ControlKind,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub unit: String,
    /// UI display multiplier: raw value × display_scale for display; divide by scale on send.
    /// E.g. display_scale=100.0 renders SoC fraction 0.8 as "80 %".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_scale: Option<f64>,
    /// Marks this slider as representing an optional override where `max` is
    /// physically equivalent to "no limit" (e.g. a generation cap at the
    /// asset's rated power curtails nothing). The frontend renders the
    /// top of the range as an explicit "Off" state — both when no override
    /// is active (current value is `None`) and when the user drags into
    /// that top zone and releases, which sends `null` instead of `max` to
    /// clear the override. Not meaningful for controls whose max is a real,
    /// distinct setpoint (e.g. a temperature or SoC target), so defaults to
    /// `false` and is omitted from the wire format unless `true`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nullable: bool,
}

// ─── Phase A new types ────────────────────────────────────────────────────────

/// Point-in-time feasible power range. Valid only for the state it was computed from.
///
/// Sign convention: negative = export/generation, positive = import/consumption.
///   max_export_kw ≤ 0  — ceiling (maximum export magnitude)
///   max_import_kw ≥ 0  — ceiling (maximum import magnitude)
///
/// Each field describes only its own direction. An asset that cannot physically
/// move in a direction at all (e.g. PV never imports, BaseLoad never exports)
/// reports 0.0 for that direction's field — never a copy of the other
/// direction's live value. The two fields are only ever equal by coincidence
/// (both genuinely 0), never by construction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetCapability {
    pub max_export_kw: f64,
    pub max_import_kw: f64,
    /// BL-27: how this asset can be controlled — on/off, stepped, continuous,
    /// curtail-only, etc. A static classification of the device's control mode,
    /// independent of the live ceiling above.
    pub adjustability: crate::entities::asset::PowerAdjustability,
    /// For `Stepped` adjustability: explicit discrete import levels in kW
    /// (ascending, including 0.0). Empty for every other adjustability.
    pub power_steps_kw: Vec<f64>,
}

impl AssetCapability {
    /// True if the asset has no controllable headroom in either direction right
    /// now (floor == ceiling for both `min_export_kw`/`max_export_kw` and
    /// `min_import_kw`/`max_import_kw`) — not whether the two directions equal
    /// each other, which conflates "stuck at a single operating point" with
    /// "structurally can't move this way at all."
    pub fn is_fixed(&self, floor: &AssetFlexibilityFloor) -> bool {
        (self.max_export_kw - floor.min_export_kw).abs() < 1e-6
            && (self.max_import_kw - floor.min_import_kw).abs() < 1e-6
    }
}

/// The lowest magnitude the asset could still be forced to right now, if the VEN had
/// to minimize its power irrespective of current commitments — distinct from 0 for
/// assets with a genuine operational floor (e.g. a heater's discrete hardware tiers).
/// Same sign convention as `AssetCapability`; `min_export_kw`/`min_import_kw` sit
/// between 0 and the corresponding `max_export_kw`/`max_import_kw`.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct AssetFlexibilityFloor {
    pub min_export_kw: f64,
    pub min_import_kw: f64,
}

/// State-only enum. Variants hold only mutable runtime state — no config fields.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "asset_type", rename_all = "snake_case")]
pub enum AssetState {
    Battery(BatteryState),
    Ev(EvState),
    Heater(HeaterState),
    Pv(PvState),
    BaseLoad(BaseLoadState),
    /// Virtual asset: derived from sum of all other assets + VTN capacity limits.
    Grid(GridState),
}

impl AssetState {
    /// Actual power in this state. Positive = import from grid, negative = export.
    pub fn actual_power_kw(&self) -> f64 {
        match self {
            Self::Battery(s) => s.actual_power_kw,
            Self::Ev(s) => s.actual_power_kw,
            Self::Heater(s) => s.actual_power_kw,
            Self::Pv(s) => s.actual_power_kw,
            Self::BaseLoad(s) => s.actual_power_kw,
            Self::Grid(s) => s.net_power_kw,
        }
    }

    /// State of charge in [0.0, 1.0] for storage assets; None for all others.
    pub fn soc(&self) -> Option<f64> {
        match self {
            Self::Battery(s) => Some(s.soc),
            Self::Ev(s) => Some(s.soc),
            _ => None,
        }
    }
}

/// Grid virtual state. Not controllable; derived from sum of all other assets.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GridState {
    /// Net site power. Positive = importing from grid. Negative = exporting to grid.
    pub net_power_kw: f64,
    /// Maximum site import power allowed by active VTN events. Always ≥ 0.
    pub import_limit_kw: f64,
    /// Maximum site export power allowed by active VTN events. Always ≤ 0.
    pub export_limit_kw: f64,
}

#[cfg(test)]
mod phase2a_battery_tests {
    //! Spec A Phase 2a (`asset-dispatch-trait-objects` tasks.md 4.1): Battery's
    //! `Asset`/`MilpParticipant`/`RequestResolvable` trait methods, called
    //! through `Box<dyn Asset>` — the only dispatch path once `AssetConfig`
    //! is deleted (Phase 3). Originally written as "enum vs boxed"
    //! equivalence tests during the migration; simplified to direct
    //! assertions now that there's only one implementation to test.

    use super::*;
    use crate::controller::milp_planner::{AssetKind, AssetMilpParams};
    use crate::entities::asset_params::BatteryParams;
    use chrono::{Duration, Utc};
    use std::collections::HashMap;

    fn boxed_and_state(initial_soc: f64) -> (Box<dyn Asset>, AssetState) {
        let params = BatteryParams {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            initial_soc,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
            c_terminal_eur_kwh: None,
        };
        (
            Box::new(Battery::from_params(&params)),
            AssetState::Battery(Battery::initial_state(&params)),
        )
    }

    #[test]
    fn state_values_exposes_soc() {
        let (boxed, state) = boxed_and_state(0.5);
        assert_eq!(boxed.state_values(&state).get("soc"), Some(&0.5));
    }

    #[test]
    fn reset_applies_soc_override() {
        let (boxed, _) = boxed_and_state(0.5);
        let mut state = AssetState::Battery(BatteryState {
            soc: 0.5,
            actual_power_kw: 0.0,
        });
        boxed.reset(&mut state, HashMap::from([("soc".to_string(), 0.8)]));
        assert_eq!(state.soc(), Some(0.8));
    }

    #[test]
    fn forecast_produces_samples() {
        let (boxed, state) = boxed_and_state(0.5);
        let series = boxed.forecast(&state, Duration::seconds(300), Utc::now());
        assert!(!series.samples.is_empty());
    }

    #[test]
    fn resolve_request_target_toward_higher_soc_returns_energy_and_power() {
        let (boxed, state) = boxed_and_state(0.5);
        let result = boxed
            .as_request_resolvable()
            .expect("Battery must implement RequestResolvable")
            .resolve_request_target(&state, Some(0.9), None);
        assert!(result.is_some(), "request toward a higher SoC must resolve");
    }

    #[test]
    fn available_storage_kwh_reports_both_directions() {
        let (boxed, state) = boxed_and_state(0.6);
        let result = boxed
            .as_request_resolvable()
            .expect("Battery must implement RequestResolvable")
            .available_storage_kwh(&state);
        assert!(result.is_some(), "Battery always reports available storage");
    }

    #[test]
    fn surplus_charge_kw_always_none() {
        let (boxed, state) = boxed_and_state(0.5);
        let result = boxed
            .as_request_resolvable()
            .expect("Battery must implement RequestResolvable")
            .surplus_charge_kw(&state, 2.0);
        assert_eq!(result, None, "Battery never absorbs surplus");
    }

    #[test]
    fn build_milp_context_reports_battery_kind_and_scalars() {
        let (boxed, state) = boxed_and_state(0.5);
        let now = Utc::now();

        let ctx = boxed
            .as_milp_participant()
            .expect("Battery must implement MilpParticipant")
            .build_milp_context(
                &state,
                4,
                &[300, 600, 900, 1200],
                now,
                None,
                None,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.05,
                vec![],
                0.0,
            );

        assert_eq!(ctx.asset_kind(), AssetKind::Battery);
        let AssetMilpParams::Battery(scalars) = ctx.milp_params(4, now) else {
            panic!("expected Battery scalars");
        };
        assert_eq!(scalars.e_nom_kwh, 10.0);
        assert!((scalars.e_init_kwh - 5.0).abs() < 1e-9, "0.5 soc * 10 kWh");
    }
}

#[cfg(test)]
mod phase2a_ev_tests {
    //! Spec A Phase 2a (`asset-dispatch-trait-objects` tasks.md 4.2) — see
    //! `phase2a_battery_tests`'s module doc for the "why boxed-only now"
    //! rationale.

    use super::*;
    use crate::controller::milp_planner::AssetMilpParams;
    use crate::entities::asset_params::EvParams;
    use chrono::{Duration, Utc};

    fn ev_params() -> EvParams {
        EvParams {
            id: "ev".to_string(),
            max_charge_kw: 7.0,
            max_discharge_kw: 7.0,
            initial_soc: 0.4,
            battery_kwh: 50.0,
            soc_target: 0.8,
            default_charge_kw: 7.0,
            min_charge_kw: 1.4,
            response_delay_s: 0.0,
            v2g_capable: true,
        }
    }

    fn boxed_and_state() -> (Box<dyn Asset>, AssetState) {
        let params = ev_params();
        (
            Box::new(EvCharger::from_params(&params)),
            AssetState::Ev(EvCharger::initial_state(&params)),
        )
    }

    #[test]
    fn state_values_exposes_soc() {
        let (boxed, state) = boxed_and_state();
        assert_eq!(boxed.state_values(&state).get("soc"), Some(&0.4));
    }

    #[test]
    fn forecast_produces_samples() {
        let (boxed, state) = boxed_and_state();
        let series = boxed.forecast(&state, Duration::seconds(300), Utc::now());
        assert!(!series.samples.is_empty());
    }

    #[test]
    fn resolve_request_target_toward_higher_soc_returns_energy_and_power() {
        let (boxed, state) = boxed_and_state();
        let result = boxed
            .as_request_resolvable()
            .expect("EvCharger must implement RequestResolvable")
            .resolve_request_target(&state, Some(0.9), None);
        assert!(result.is_some());
    }

    #[test]
    fn available_storage_kwh_reports_both_directions_when_plugged() {
        let (boxed, state) = boxed_and_state();
        let result = boxed
            .as_request_resolvable()
            .expect("EvCharger must implement RequestResolvable")
            .available_storage_kwh(&state);
        assert!(result.is_some());
    }

    #[test]
    fn available_storage_kwh_none_when_unplugged() {
        let (boxed, _) = boxed_and_state();
        let unplugged = AssetState::Ev(EvState {
            soc: 0.4,
            plugged: false,
            actual_power_kw: 0.0,
            pending_command_kw: 0.0,
        });
        let result = boxed
            .as_request_resolvable()
            .unwrap()
            .available_storage_kwh(&unplugged);
        assert_eq!(result, None, "unplugged EV must report no storage");
    }

    #[test]
    fn surplus_charge_kw_positive_when_plugged_and_below_target() {
        let (boxed, state) = boxed_and_state();
        let result = boxed
            .as_request_resolvable()
            .expect("EvCharger must implement RequestResolvable")
            .surplus_charge_kw(&state, 3.0);
        assert!(
            result.is_some(),
            "EV below soc_target and plugged must absorb surplus"
        );
    }

    #[test]
    fn build_milp_context_reports_ev_scalars() {
        let (boxed, state) = boxed_and_state();
        let now = Utc::now();

        let ctx = boxed
            .as_milp_participant()
            .expect("EvCharger must implement MilpParticipant")
            .build_milp_context(
                &state,
                4,
                &[300, 600, 900, 1200],
                now,
                None,
                None,
                1.4,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.05,
                vec![],
                0.0,
            );

        let AssetMilpParams::Ev(scalars) = ctx.milp_params(4, now) else {
            panic!("expected Ev scalars");
        };
        assert_eq!(scalars.p_max_kw, 7.0);
    }
}

#[cfg(test)]
mod phase2a_heater_tests {
    //! Spec A Phase 2a (`asset-dispatch-trait-objects` tasks.md 4.3) — see
    //! `phase2a_battery_tests`'s module doc for the "why boxed-only now"
    //! rationale.

    use super::*;
    use crate::controller::milp_planner::AssetMilpParams;
    use crate::entities::asset_params::HeaterParams;
    use chrono::{Duration, Utc};

    fn heater_params() -> HeaterParams {
        HeaterParams {
            id: "heater".to_string(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            temp_safety_max_c: 23.0,
            power_stages: 2,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            switching_penalty_eur: 0.0,
            c_terminal_eur_kwh: None,
        }
    }

    fn boxed_and_state() -> (Box<dyn Asset>, AssetState) {
        let params = heater_params();
        (
            Box::new(Heater::from_params(&params)),
            AssetState::Heater(Heater::initial_state(&params)),
        )
    }

    #[test]
    fn state_values_exposes_temp_c() {
        let (boxed, state) = boxed_and_state();
        assert_eq!(boxed.state_values(&state).get("temp_c"), Some(&20.0));
    }

    #[test]
    fn forecast_produces_samples() {
        let (boxed, state) = boxed_and_state();
        let series = boxed.forecast(&state, Duration::seconds(300), Utc::now());
        assert!(!series.samples.is_empty());
    }

    #[test]
    fn plan_trajectory_returns_a_trajectory() {
        let (boxed, state) = boxed_and_state();
        let result = boxed
            .as_thermostat()
            .expect("Heater must implement Thermostat")
            .plan_trajectory(&state);
        assert!(result.is_some());
    }

    #[test]
    fn thermostat_setpoint_kw_below_target_calls_for_max_kw() {
        let (boxed, state) = boxed_and_state(); // temp_initial_c=20.0
        let result = boxed
            .as_thermostat()
            .expect("Heater must implement Thermostat")
            .thermostat_setpoint_kw(&state, 22.0);
        assert_eq!(result, 3.0, "below target must call for max_kw");
    }

    #[test]
    fn thermostat_setpoint_kw_above_target_calls_for_nothing() {
        let (boxed, state) = boxed_and_state(); // temp_initial_c=20.0
        let result = boxed
            .as_thermostat()
            .expect("Heater must implement Thermostat")
            .thermostat_setpoint_kw(&state, 18.0);
        assert_eq!(result, 0.0, "above target must call for nothing");
    }

    #[test]
    fn build_milp_context_reports_heater_scalars() {
        let (boxed, state) = boxed_and_state();
        let now = Utc::now();

        let ctx = boxed
            .as_milp_participant()
            .expect("Heater must implement MilpParticipant")
            .build_milp_context(
                &state,
                4,
                &[300, 600, 900, 1200],
                now,
                None,
                None,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.05,
                vec![None; 4],
                0.0,
            );

        let AssetMilpParams::Heater(scalars) = ctx.milp_params(4, now) else {
            panic!("expected Heater scalars");
        };
        assert!((scalars.e_max_kwh - 10.0).abs() < 1e-9, "(23-18)*2 kWh/C");
    }
}

#[cfg(test)]
mod phase2a_pv_tests {
    //! Spec A Phase 2a (`asset-dispatch-trait-objects` tasks.md 4.4). PV
    //! implements none of D4's three capability traits — only the 9
    //! universal methods need equivalence coverage.

    use super::*;
    use crate::entities::asset_params::PvParams;
    use chrono::{Duration, Utc};

    fn boxed_and_state() -> (Box<dyn Asset>, AssetState) {
        let params = PvParams {
            id: "pv".to_string(),
            rated_kw: 10.0,
            inverter_max_kw: 8.5,
            co2_g_kwh: 40.0,
        };
        (
            Box::new(PvInverter::from_params(&params)),
            AssetState::Pv(PvInverter::initial_state(&params)),
        )
    }

    #[test]
    fn state_values_exposes_rated_kw() {
        let (boxed, state) = boxed_and_state();
        assert_eq!(boxed.state_values(&state).get("rated_kw"), Some(&10.0));
    }

    #[test]
    fn forecast_produces_samples() {
        let (boxed, state) = boxed_and_state();
        let series = boxed.forecast(&state, Duration::seconds(300), Utc::now());
        assert!(!series.samples.is_empty());
    }

    #[test]
    fn no_capability_trait_overrides() {
        let (boxed, _) = boxed_and_state();
        assert!(boxed.as_milp_participant().is_none());
        assert!(boxed.as_request_resolvable().is_none());
        assert!(boxed.as_thermostat().is_none());
    }
}

#[cfg(test)]
mod phase2a_base_load_tests {
    //! Spec A Phase 2a (`asset-dispatch-trait-objects` tasks.md 4.5).
    //! BaseLoad implements none of D4's three capability traits — only the
    //! 9 universal methods need equivalence coverage.

    use super::*;
    use crate::entities::asset_params::BaseLoadParams;
    use chrono::{Duration, Utc};

    fn boxed_and_state() -> (Box<dyn Asset>, AssetState) {
        let params = BaseLoadParams {
            id: "base_load".to_string(),
            baseline_kw: 0.6,
            spikes: vec![],
        };
        (
            Box::new(BaseLoad::from_params(&params)),
            AssetState::BaseLoad(BaseLoad::initial_state(&params)),
        )
    }

    #[test]
    fn state_values_exposes_baseline_kw() {
        let (boxed, state) = boxed_and_state();
        assert_eq!(boxed.state_values(&state).get("baseline_kw"), Some(&0.6));
    }

    #[test]
    fn forecast_produces_samples() {
        let (boxed, state) = boxed_and_state();
        let series = boxed.forecast(&state, Duration::seconds(300), Utc::now());
        assert!(!series.samples.is_empty());
    }

    #[test]
    fn no_capability_trait_overrides() {
        let (boxed, _) = boxed_and_state();
        assert!(boxed.as_milp_participant().is_none());
        assert!(boxed.as_request_resolvable().is_none());
        assert!(boxed.as_thermostat().is_none());
    }
}

#[cfg(test)]
mod phase2a_trivial_delegation_smoke_tests {
    //! Closes a real gap found on review: the 6 trivial-delegation universal
    //! methods (`default_setpoint`, `control_schema`, `update_config`,
    //! `default_comfort_rates`, `default_completion_policy`,
    //! `default_post_deadline_comfort_bid`) were never actually invoked
    //! through `Box<dyn Asset>` in any test — their correctness rested on
    //! "inherent methods always shadow same-named trait methods" (correct
    //! per Rust's method resolution rules, and empirically consistent with
    //! the full suite never hanging/crashing on a stack overflow), but that's
    //! inference, not proof. These smoke tests call each one through the
    //! boxed path directly, for every asset type, confirming real dispatch
    //! to the inherent method rather than infinite self-recursion into the
    //! trait's own panicking default (which would stack-overflow, not
    //! silently misbehave — so "doesn't crash" here is a real, meaningful
    //! assertion, not a tautology).

    use super::*;
    use crate::entities::asset_params::{
        BaseLoadParams, BatteryParams, EvParams, HeaterParams, PvParams,
    };
    use std::collections::HashMap;

    fn exercise_trivial_methods(mut boxed: Box<dyn Asset>) {
        let _ = boxed.default_setpoint();
        let _ = boxed.control_schema();
        boxed.update_config(HashMap::new());
        let _ = boxed.default_comfort_rates();
        let _ = boxed.default_completion_policy();
        let _ = boxed.default_post_deadline_comfort_bid();
    }

    #[test]
    fn battery_trivial_methods_reach_inherent_impl_not_infinite_recursion() {
        let params = BatteryParams {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            initial_soc: 0.5,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
            c_terminal_eur_kwh: None,
        };
        exercise_trivial_methods(Box::new(Battery::from_params(&params)));
    }

    #[test]
    fn ev_trivial_methods_reach_inherent_impl_not_infinite_recursion() {
        let params = EvParams {
            id: "ev".to_string(),
            max_charge_kw: 7.0,
            max_discharge_kw: 7.0,
            initial_soc: 0.4,
            battery_kwh: 50.0,
            soc_target: 0.8,
            default_charge_kw: 7.0,
            min_charge_kw: 1.4,
            response_delay_s: 0.0,
            v2g_capable: true,
        };
        exercise_trivial_methods(Box::new(EvCharger::from_params(&params)));
    }

    #[test]
    fn heater_trivial_methods_reach_inherent_impl_not_infinite_recursion() {
        let params = HeaterParams {
            id: "heater".to_string(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            temp_safety_max_c: 23.0,
            power_stages: 2,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            switching_penalty_eur: 0.0,
            c_terminal_eur_kwh: None,
        };
        exercise_trivial_methods(Box::new(Heater::from_params(&params)));
    }

    #[test]
    fn pv_trivial_methods_reach_inherent_impl_not_infinite_recursion() {
        let params = PvParams {
            id: "pv".to_string(),
            rated_kw: 10.0,
            inverter_max_kw: 8.5,
            co2_g_kwh: 40.0,
        };
        exercise_trivial_methods(Box::new(PvInverter::from_params(&params)));
    }

    #[test]
    fn base_load_trivial_methods_reach_inherent_impl_not_infinite_recursion() {
        let params = BaseLoadParams {
            id: "base_load".to_string(),
            baseline_kw: 0.6,
            spikes: vec![],
        };
        exercise_trivial_methods(Box::new(BaseLoad::from_params(&params)));
    }
}

#[cfg(test)]
mod phase2b_asset_type_and_downcast_tests {
    //! Spec A Phase 2b groundwork: `asset_type`/`asset_type_str` (needed by
    //! `simulator/snapshot.rs`) and `as_any`/`as_any_mut` (needed by the
    //! remaining `AssetConfig`-matching call sites) must reproduce exactly
    //! what those call sites' own match arms currently return.

    use super::*;
    use crate::entities::asset::AssetType;

    #[test]
    fn asset_type_and_str_match_snapshot_rs_existing_mapping() {
        // Mirrors simulator/snapshot.rs's two independent matches verbatim --
        // including BaseLoad's deliberate divergence (GenericConsumer vs.
        // "base_load", not a 1:1 pair).
        let cases: Vec<(Box<dyn Asset>, AssetType, &str)> = vec![
            (
                Box::new(Battery::from_params(
                    &crate::entities::asset_params::BatteryParams {
                        id: "battery".into(),
                        capacity_kwh: 10.0,
                        max_charge_kw: 5.0,
                        max_discharge_kw: 5.0,
                        initial_soc: 0.5,
                        round_trip_efficiency: 0.9,
                        min_soc: 0.1,
                        c_terminal_eur_kwh: None,
                    },
                )),
                AssetType::Battery,
                "battery",
            ),
            (
                Box::new(EvCharger::from_params(
                    &crate::entities::asset_params::EvParams {
                        id: "ev".into(),
                        max_charge_kw: 7.0,
                        max_discharge_kw: 7.0,
                        initial_soc: 0.4,
                        battery_kwh: 50.0,
                        soc_target: 0.8,
                        default_charge_kw: 7.0,
                        min_charge_kw: 1.4,
                        response_delay_s: 0.0,
                        v2g_capable: true,
                    },
                )),
                AssetType::Ev,
                "ev",
            ),
            (
                Box::new(PvInverter::from_params(
                    &crate::entities::asset_params::PvParams {
                        id: "pv".into(),
                        rated_kw: 10.0,
                        inverter_max_kw: 8.5,
                        co2_g_kwh: 40.0,
                    },
                )),
                AssetType::Pv,
                "pv",
            ),
            (
                Box::new(BaseLoad::from_params(
                    &crate::entities::asset_params::BaseLoadParams {
                        id: "base_load".into(),
                        baseline_kw: 0.6,
                        spikes: vec![],
                    },
                )),
                AssetType::GenericConsumer,
                "base_load",
            ),
        ];

        for (boxed, expected_type, expected_str) in cases {
            assert_eq!(boxed.asset_type(), expected_type);
            assert_eq!(boxed.asset_type_str(), expected_str);
        }
    }

    #[test]
    fn as_any_downcasts_to_the_correct_concrete_type() {
        let boxed: Box<dyn Asset> = Box::new(PvInverter::from_params(
            &crate::entities::asset_params::PvParams {
                id: "pv".into(),
                rated_kw: 10.0,
                inverter_max_kw: 8.5,
                co2_g_kwh: 40.0,
            },
        ));
        assert!(boxed.as_any().downcast_ref::<PvInverter>().is_some());
        assert!(boxed.as_any().downcast_ref::<Battery>().is_none());
    }

    #[test]
    fn as_any_mut_allows_mutating_the_recovered_concrete_type() {
        let mut boxed: Box<dyn Asset> = Box::new(PvInverter::from_params(
            &crate::entities::asset_params::PvParams {
                id: "pv".into(),
                rated_kw: 10.0,
                inverter_max_kw: 8.5,
                co2_g_kwh: 40.0,
            },
        ));
        let pv = boxed
            .as_any_mut()
            .downcast_mut::<PvInverter>()
            .expect("must downcast to PvInverter");
        pv.irradiance_offset = 0.3;
        assert_eq!(
            boxed
                .as_any()
                .downcast_ref::<PvInverter>()
                .unwrap()
                .irradiance_offset,
            0.3
        );
    }
}

#[cfg(test)]
mod phase2b_tick_overridable_tests {
    //! Spec A Phase 2b prerequisite (`asset-dispatch-trait-objects` tasks.md
    //! 5.3 groundwork): proves each `TickOverridable` impl reproduces exactly
    //! what `SimState::tick()`'s hand-written match arm used to do, for the
    //! same inputs. `tick()` itself isn't rewired to call these yet (that's
    //! the atomic storage-cutover step) -- these tests exercise the trait
    //! directly via the Phase 1 `to_boxed_asset()` bridge.

    use super::*;
    use crate::entities::asset_params::PvCurtailmentSource;
    use crate::entities::asset_params::{BaseLoadParams, EvParams, HeaterParams, PvParams};
    use chrono::Duration;

    fn default_overrides() -> TickOverrides {
        TickOverrides {
            pv_irradiance: 0.0,
            pv_irradiance_offset: 0.0,
            pv_alpha: 0.1,
            pv_generation_limit_kw: None,
            pv_curtailment_source: PvCurtailmentSource::None,
            pv_weather_power_kw: None,
            pv_measured_power_kw: None,
            pv_irradiance_forced: false,
            heater_ambient_temp_c_override: None,
            heater_temp_min_override: None,
            heater_temp_max_override: None,
            heater_emergency_curtail_override: None,
            heater_emergency_absorb_override: None,
            base_load_measured_kw: None,
            base_load_baseline_kw: None,
            ev_plugged_override: None,
            ev_soc_target_override: None,
        }
    }

    #[test]
    fn pv_apply_tick_overrides_sets_all_fields() {
        let mut boxed: Box<dyn Asset> = Box::new(PvInverter::from_params(&PvParams {
            id: "pv".to_string(),
            rated_kw: 10.0,
            inverter_max_kw: 8.5,
            co2_g_kwh: 40.0,
        }));
        let mut state = AssetState::Pv(PvState {
            actual_power_kw: 0.0,
            generation_limit_kw: None,
            curtailment_source: PvCurtailmentSource::None,
        });

        let overrides = TickOverrides {
            pv_irradiance: 0.75,
            pv_irradiance_offset: 0.05,
            pv_alpha: 0.2,
            // No generation_limit_kw here deliberately: with one set, a
            // clamped result can't distinguish "irradiance_forced won" from
            // "some other override won and got clamped to the same value."
            pv_generation_limit_kw: None,
            pv_curtailment_source: PvCurtailmentSource::Plan,
            pv_weather_power_kw: Some(4.0),
            pv_measured_power_kw: Some(4.2),
            pv_irradiance_forced: true,
            ..default_overrides()
        };
        boxed
            .as_tick_overridable()
            .expect("PV must implement TickOverridable")
            .apply_tick_overrides(&mut state, &overrides);

        // Confirm the forced-irradiance branch actually took effect downstream:
        // irradiance_forced + irradiance must win outright over the weather/
        // measured overrides also present, per PvInverter's own documented
        // precedence -- 0.75 * 10 kW = 7.5 kW, not the weather(4.0) or
        // measured(4.2) values.
        let (_, actual_kw) = boxed.step(&state, 0.0, Duration::seconds(1));
        assert!(
            (actual_kw + 7.5).abs() < 1e-9,
            "forced irradiance=0.75 on a 10kW array must yield -7.5 kW \
             regardless of weather/measured overrides, got {actual_kw}"
        );
    }

    #[test]
    fn heater_apply_tick_overrides_matches_inherent_method() {
        let params = HeaterParams {
            id: "heater".to_string(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            temp_safety_max_c: 23.0,
            power_stages: 2,
            thermal_mass_kwh_per_c: 2.0,
            k_loss_kw_per_c: 0.1,
            draw_kw: 0.0,
            switching_penalty_eur: 0.0,
            c_terminal_eur_kwh: None,
        };
        let mut via_trait = Heater::from_params(&params);
        let mut via_inherent = Heater::from_params(&params);
        let mut state = AssetState::Heater(HeaterState {
            temperature_c: 20.0,
            actual_power_kw: 0.0,
        });

        let overrides = TickOverrides {
            heater_ambient_temp_c_override: Some(5.0),
            heater_temp_min_override: Some(17.0),
            heater_temp_max_override: Some(24.0),
            heater_emergency_curtail_override: Some(true),
            heater_emergency_absorb_override: None,
            ..default_overrides()
        };
        // Dot-syntax always resolves to the inherent method (it shadows the
        // trait method of the same name) -- exactly the ambiguity flagged in
        // tasks.md, confirmed here: fully-qualified syntax is required to
        // reach the trait impl on a concrete `Heater`. Through `dyn
        // TickOverridable` (what `tick()`'s rewrite will actually use) this
        // doesn't arise -- a trait object only exposes the trait's own methods.
        TickOverridable::apply_tick_overrides(&mut via_trait, &mut state, &overrides);
        via_inherent.apply_tick_overrides(Some(5.0), Some(17.0), Some(24.0), Some(true), None);

        assert_eq!(via_trait.ambient_temp_c, via_inherent.ambient_temp_c);
        assert_eq!(via_trait.temp_min_c, via_inherent.temp_min_c);
        assert_eq!(via_trait.temp_max_c, via_inherent.temp_max_c);
        assert_eq!(via_trait.emergency_mode, via_inherent.emergency_mode);
    }

    #[test]
    fn base_load_apply_tick_overrides_sets_measured_and_baseline() {
        let mut bl = BaseLoad::from_params(&BaseLoadParams {
            id: "base_load".to_string(),
            baseline_kw: 0.5,
            spikes: vec![],
        });
        let mut state = AssetState::BaseLoad(BaseLoadState {
            actual_power_kw: 0.0,
        });

        let overrides = TickOverrides {
            base_load_measured_kw: Some(1.2),
            base_load_baseline_kw: Some(1.5),
            ..default_overrides()
        };
        bl.apply_tick_overrides(&mut state, &overrides);

        assert_eq!(bl.measured_load_kw, Some(1.2));
        assert_eq!(bl.baseline_kw, 1.5);
    }

    #[test]
    fn base_load_apply_tick_overrides_leaves_baseline_unchanged_when_none() {
        // Mirrors tick()'s "no BaseLoad asset found" case: base_load_baseline_kw
        // is None, and the original match arm never fires at all, so
        // baseline_kw must be left exactly as it was.
        let mut bl = BaseLoad::from_params(&BaseLoadParams {
            id: "base_load".to_string(),
            baseline_kw: 0.5,
            spikes: vec![],
        });
        let mut state = AssetState::BaseLoad(BaseLoadState {
            actual_power_kw: 0.0,
        });

        bl.apply_tick_overrides(&mut state, &default_overrides());

        assert_eq!(bl.baseline_kw, 0.5, "baseline_kw must be untouched");
    }

    #[test]
    fn ev_apply_tick_overrides_sets_plugged_state_and_soc_target() {
        let params = EvParams {
            id: "ev".to_string(),
            max_charge_kw: 7.0,
            max_discharge_kw: 7.0,
            initial_soc: 0.4,
            battery_kwh: 50.0,
            soc_target: 0.8,
            default_charge_kw: 7.0,
            min_charge_kw: 1.4,
            response_delay_s: 0.0,
            v2g_capable: true,
        };
        let mut ev = EvCharger::from_params(&params);
        let mut state = AssetState::Ev(EvCharger::initial_state(&params));

        let overrides = TickOverrides {
            ev_plugged_override: Some(false),
            ev_soc_target_override: Some(0.6),
            ..default_overrides()
        };
        ev.apply_tick_overrides(&mut state, &overrides);

        let AssetState::Ev(s) = state else {
            panic!("expected Ev state")
        };
        assert!(!s.plugged, "plugged override must be applied to state");
        assert_eq!(ev.soc_target, 0.6);
    }

    #[test]
    fn ev_apply_tick_overrides_snaps_back_to_profile_defaults_when_none() {
        let params = EvParams {
            id: "ev".to_string(),
            max_charge_kw: 7.0,
            max_discharge_kw: 7.0,
            initial_soc: 0.4,
            battery_kwh: 50.0,
            soc_target: 0.8,
            default_charge_kw: 7.0,
            min_charge_kw: 1.4,
            response_delay_s: 0.0,
            v2g_capable: true,
        };
        let mut ev = EvCharger::from_params(&params);
        ev.soc_target = 0.5; // simulate a lingering override from a prior tick
        let mut state = AssetState::Ev(EvCharger::initial_state(&params));
        if let AssetState::Ev(s) = &mut state {
            s.plugged = false;
        }

        ev.apply_tick_overrides(&mut state, &default_overrides());

        let AssetState::Ev(s) = state else {
            panic!("expected Ev state")
        };
        assert!(s.plugged, "no override must snap back to plugged=true");
        assert_eq!(
            ev.soc_target, ev.soc_target_profile,
            "no override must snap back to the profile soc_target"
        );
    }
}
