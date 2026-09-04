use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

use crate::common::{Interpolation, TimeSeries};
use crate::controller::milp_planner::asset_port::{
    BatteryMilpContext, EvMilpContext, HeaterMilpContext,
};

mod asset_trait;
pub mod base_load;
pub mod battery;
mod battery_milp;
pub mod ev;
mod ev_comfort;
mod ev_milp;
pub mod grid;
pub mod heater;
mod heater_milp;
mod history;
pub mod pv;

// AssetHandle/TrajectoryPoint are consumed only within asset_trait's own tests — same
// bin-crate "pub items have no external consumer" situation AssetHandle was already
// #[allow(dead_code)]'d for before this file split.
#[allow(unused_imports)]
pub use asset_trait::{Asset, AssetHandle, Trajectory, TrajectoryPoint};
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

/// Runtime config dispatch enum. Holds physics config for each asset type.
/// This is the renamed + restructured successor to what was previously called `AssetState`
/// (which conflated config and state).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "asset_type", rename_all = "snake_case")]
pub enum AssetConfig {
    Battery(Battery),
    Ev(EvCharger),
    Heater(Heater),
    Pv(PvInverter),
    BaseLoad(BaseLoad),
}

/// Forwards a method call to the config held by whichever `AssetConfig` variant `self` is,
/// declaring the `Battery|Ev|Heater|Pv|BaseLoad` variant list exactly once. For methods whose
/// signature is uniform across all 5 variants (mirrors the `Asset` trait or is a simple
/// per-config accessor) — see `delegate_asset_state!` for methods that also match on `AssetState`.
macro_rules! delegate_asset {
    ($self:expr, $method:ident($($arg:expr),*)) => {
        match $self {
            AssetConfig::Battery(cfg) => cfg.$method($($arg),*),
            AssetConfig::Ev(cfg) => cfg.$method($($arg),*),
            AssetConfig::Heater(cfg) => cfg.$method($($arg),*),
            AssetConfig::Pv(cfg) => cfg.$method($($arg),*),
            AssetConfig::BaseLoad(cfg) => cfg.$method($($arg),*),
        }
    };
}

/// Like `delegate_asset!`, but also matches `$state` against the same variant so the config
/// and state can be destructured together (e.g. `state_values`, `reset`, `forecast`). Falls
/// back to `$default` on a config/state variant mismatch — mirrors the hand-written `_ => ...`
/// arm every one of these methods had before the macro existed.
macro_rules! delegate_asset_state {
    ($self:expr, $state:expr, $method:ident($($arg:expr),*), $default:expr) => {
        match ($self, $state) {
            (AssetConfig::Battery(cfg), AssetState::Battery(s)) => cfg.$method(s, $($arg),*),
            (AssetConfig::Ev(cfg), AssetState::Ev(s)) => cfg.$method(s, $($arg),*),
            (AssetConfig::Heater(cfg), AssetState::Heater(s)) => cfg.$method(s, $($arg),*),
            (AssetConfig::Pv(cfg), AssetState::Pv(s)) => cfg.$method(s, $($arg),*),
            (AssetConfig::BaseLoad(cfg), AssetState::BaseLoad(s)) => cfg.$method(s, $($arg),*),
            _ => $default,
        }
    };
}

impl AssetConfig {
    // ── Spec A Phase 1: trait-object bridge ─────────────────────────────────

    /// Temporary bridge (`asset-dispatch-trait-objects` tasks.md 3.1/3.2):
    /// construct the `Box<dyn Asset>` equivalent of this config, proving
    /// trait-object dispatch produces identical results to today's
    /// enum dispatch ahead of the Phase 2b storage cutover. Deleted in
    /// Phase 3 once `AssetConfig` itself is gone — callers construct
    /// `Box<dyn Asset>` directly by then, no bridge needed.
    pub fn to_boxed_asset(&self) -> Box<dyn Asset> {
        match self {
            AssetConfig::Battery(cfg) => Box::new(cfg.clone()),
            AssetConfig::Ev(cfg) => Box::new(cfg.clone()),
            AssetConfig::Heater(cfg) => Box::new(cfg.clone()),
            AssetConfig::Pv(cfg) => Box::new(cfg.clone()),
            AssetConfig::BaseLoad(cfg) => Box::new(cfg.clone()),
        }
    }

    // ── Asset trait dispatch ────────────────────────────────────────────────

    pub fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
        use Asset as _;
        delegate_asset!(self, step(state, setpoint_kw, dt))
    }

    pub fn capability(&self, state: &AssetState) -> AssetCapability {
        use Asset as _;
        delegate_asset!(self, capability(state))
    }

    pub fn flexibility_floor(&self, state: &AssetState) -> AssetFlexibilityFloor {
        use Asset as _;
        delegate_asset!(self, flexibility_floor(state))
    }

    // ── Dispatch methods (previously on AssetState) ─────────────────────────

    pub fn default_setpoint(&self, _state: &AssetState) -> f64 {
        delegate_asset!(self, default_setpoint())
    }

    /// Returns a stateful trajectory computer seeded from the current live state,
    /// or `None` for assets without planned state to recompute (battery, EV, PV, etc.).
    pub fn plan_trajectory(
        &self,
        live_state: &AssetState,
    ) -> Option<crate::entities::timeline::HeaterPlanTrajectory> {
        match self {
            Self::Heater(cfg) => Heater::plan_trajectory(cfg, live_state),
            _ => None,
        }
    }

    pub fn state_values(&self, state: &AssetState) -> HashMap<String, f64> {
        delegate_asset_state!(self, state, state_values(), HashMap::new())
    }

    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        delegate_asset!(self, control_schema())
    }

    pub fn reset(&self, state: &mut AssetState, values: HashMap<String, f64>) {
        delegate_asset_state!(self, state, reset(values), ())
    }

    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        delegate_asset!(self, update_config(values))
    }

    pub fn forecast(
        &self,
        state: &AssetState,
        timespan: Duration,
        now: DateTime<Utc>,
    ) -> TimeSeries {
        delegate_asset_state!(
            self,
            state,
            forecast(timespan, now),
            TimeSeries::empty(Interpolation::Linear)
        )
    }

    pub fn resolve_request_target(
        &self,
        state: &AssetState,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)> {
        match (self, state) {
            (Self::Battery(cfg), AssetState::Battery(s)) => {
                cfg.resolve_request_target(s, target_soc, desired_power_kw)
            }
            (Self::Ev(cfg), AssetState::Ev(s)) => {
                cfg.resolve_request_target(s, target_soc, desired_power_kw)
            }
            _ => None,
        }
    }

    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        delegate_asset!(self, default_comfort_rates())
    }

    pub fn default_completion_policy(&self) -> crate::entities::asset::CompletionPolicy {
        delegate_asset!(self, default_completion_policy())
    }

    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        delegate_asset!(self, default_post_deadline_comfort_bid())
    }

    /// Available storage energy. Returns `(discharge_kwh, charge_kwh)`.
    /// Returns `None` for non-storage assets or an unplugged EV.
    pub fn available_storage_kwh(&self, state: &AssetState) -> Option<(f64, f64)> {
        match (self, state) {
            (Self::Battery(b), AssetState::Battery(s)) => Some((
                (s.soc - b.min_soc).max(0.0) * b.capacity_kwh,
                (1.0 - s.soc).max(0.0) * b.capacity_kwh,
            )),
            (Self::Ev(e), AssetState::Ev(s)) if s.plugged => Some((
                (s.soc - e.min_soc).max(0.0) * e.battery_kwh,
                (1.0 - s.soc).max(0.0) * e.battery_kwh,
            )),
            _ => None,
        }
    }

    /// Thermostat ON/OFF setpoint [kW] for heating assets given a target temperature.
    /// Returns `None` for non-thermostat assets.
    pub fn thermostat_setpoint_kw(&self, state: &AssetState, target_c: f64) -> Option<f64> {
        match (self, state) {
            (Self::Heater(hcfg), AssetState::Heater(hs)) => Some(if hs.temperature_c < target_c {
                hcfg.max_kw
            } else {
                0.0
            }),
            _ => None,
        }
    }

    /// Surplus-charge absorption [kW] for assets that can opportunistically consume excess PV.
    /// Returns `None` when the asset cannot absorb surplus right now.
    pub fn surplus_charge_kw(&self, state: &AssetState, surplus_kw: f64) -> Option<f64> {
        match (self, state) {
            (Self::Ev(ecfg), AssetState::Ev(es)) if es.plugged && es.soc < ecfg.soc_target => {
                Some(surplus_kw.min(ecfg.max_charge_kw))
            }
            _ => None,
        }
    }

    /// Build the MILP context for this asset, or `None` for non-MILP assets (PV, base load, grid).
    #[allow(clippy::too_many_arguments)]
    pub fn build_milp_context(
        &self,
        state: &AssetState,
        n: usize,
        cum_s: &[i64],
        now: DateTime<Utc>,
        ev_session: Option<&crate::entities::device_session::EvSession>,
        heater_target: Option<&crate::entities::device_session::HeaterTarget>,
        ev_min_charge_kw: f64,
        v_ev_extra_eur_kwh: f64,
        v_ev_core_eur_kwh: f64,
        asap_lateness_eur_kwh_h: f64,
        v_ev_free_charge_eur_kwh: f64,
        lambda_sw: f64,
        c_terminal_eur_kwh: f64,
        heater_anchor: Vec<Option<f64>>,
        w_ghg_eur_kg: f64,
    ) -> Option<Box<dyn crate::controller::milp_planner::AssetMilpContext>> {
        match self {
            Self::Battery(cfg) => Some(Box::new(BatteryMilpContext::from_state(
                state,
                cfg,
                c_terminal_eur_kwh,
            ))),
            Self::Ev(cfg) => Some(Box::new(EvMilpContext::from_state(
                state,
                cfg,
                n,
                cum_s,
                now,
                ev_session,
                ev_min_charge_kw,
                v_ev_extra_eur_kwh,
                v_ev_core_eur_kwh,
                asap_lateness_eur_kwh_h,
                v_ev_free_charge_eur_kwh,
                w_ghg_eur_kg,
            ))),
            Self::Heater(cfg) => Some(Box::new(HeaterMilpContext::from_state(
                state,
                cfg,
                n,
                cum_s,
                now,
                heater_target,
                lambda_sw,
                c_terminal_eur_kwh,
                heater_anchor,
                w_ghg_eur_kg,
            ))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod phase1_bridge_tests {
    //! Spec A Phase 1 (`asset-dispatch-trait-objects` tasks.md 3.2): proves
    //! `Box<dyn Asset>` dispatch (via `AssetConfig::to_boxed_asset`) produces
    //! identical results to today's enum dispatch, for at least one asset
    //! type, ahead of the Phase 2b storage cutover.

    use super::*;
    use crate::entities::asset_params::BatteryParams;
    use chrono::Duration;

    fn battery_config() -> AssetConfig {
        AssetConfig::Battery(Battery::from_params(&BatteryParams {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            initial_soc: 0.5,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
            c_terminal_eur_kwh: None,
        }))
    }

    fn battery_state() -> AssetState {
        AssetState::Battery(Battery::initial_state(&BatteryParams {
            id: "battery".to_string(),
            capacity_kwh: 10.0,
            max_charge_kw: 5.0,
            max_discharge_kw: 5.0,
            initial_soc: 0.5,
            round_trip_efficiency: 0.9,
            min_soc: 0.1,
            c_terminal_eur_kwh: None,
        }))
    }

    #[test]
    fn boxed_asset_step_matches_enum_dispatched_step() {
        let cfg = battery_config();
        let state = battery_state();
        let boxed = cfg.to_boxed_asset();

        let (enum_state, enum_kw) = cfg.step(&state, 3.0, Duration::hours(1));
        let (boxed_state, boxed_kw) = boxed.step(&state, 3.0, Duration::hours(1));

        assert_eq!(enum_kw, boxed_kw, "actual power must match");
        assert_eq!(
            enum_state.actual_power_kw(),
            boxed_state.actual_power_kw(),
            "resulting state's power must match"
        );
        let AssetState::Battery(enum_bs) = enum_state else {
            unreachable!()
        };
        let AssetState::Battery(boxed_bs) = boxed_state else {
            unreachable!()
        };
        assert!(
            (enum_bs.soc - boxed_bs.soc).abs() < 1e-12,
            "resulting SoC must match: enum={}, boxed={}",
            enum_bs.soc,
            boxed_bs.soc
        );
    }

    #[test]
    fn boxed_asset_capability_matches_enum_dispatched_capability() {
        let cfg = battery_config();
        let state = battery_state();
        let boxed = cfg.to_boxed_asset();

        let enum_cap = cfg.capability(&state);
        let boxed_cap = boxed.capability(&state);

        assert_eq!(enum_cap.max_export_kw, boxed_cap.max_export_kw);
        assert_eq!(enum_cap.max_import_kw, boxed_cap.max_import_kw);
    }

    #[test]
    fn boxed_asset_flexibility_floor_matches_enum_dispatched_flexibility_floor() {
        let cfg = battery_config();
        let state = battery_state();
        let boxed = cfg.to_boxed_asset();

        let enum_floor = cfg.flexibility_floor(&state);
        let boxed_floor = boxed.flexibility_floor(&state);

        assert_eq!(enum_floor.min_export_kw, boxed_floor.min_export_kw);
        assert_eq!(enum_floor.min_import_kw, boxed_floor.min_import_kw);
    }
}
