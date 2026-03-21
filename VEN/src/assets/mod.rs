use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

use crate::common::{Interpolation, TimeSeries};
use crate::controller::trace::AssetHistoryBuffer;

pub mod base_load;
pub mod battery;
pub mod ev;
pub mod heater;
pub mod pv;

pub use base_load::BaseLoad;
pub use battery::Battery;
pub use ev::EvCharger;
pub use heater::Heater;
pub use pv::PvInverter;

/// Generic environment passed to all assets during a physics tick.
/// Each asset reads what it needs and ignores the rest.
pub type TickEnvironment = HashMap<String, f64>;

/// Planning capability descriptor for a single asset.
#[derive(Debug, Clone)]
pub struct AssetCapabilities {
    pub asset_id: String,
    pub max_import_kw: f64,
    pub max_export_kw: f64,
    pub is_flexible: bool,
    pub energy_state: Option<EnergyState>,
    pub availability: Option<TimeWindow>,
}

/// Storage state for flexible energy assets.
#[derive(Debug, Clone)]
pub struct EnergyState {
    pub current_kwh: f64,
    pub min_kwh: f64,
    pub max_kwh: f64,
}

/// Time window for asset availability.
#[derive(Debug, Clone)]
pub struct TimeWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

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
}

/// Discriminated union over all supported asset types.
/// Serialized/deserialized with an internal "type" tag for sim_state.json persistence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "asset_type", rename_all = "snake_case")]
pub enum AssetState {
    Ev(EvCharger),
    Heater(Heater),
    Pv(PvInverter),
    Battery(Battery),
    BaseLoad(BaseLoad),
}

impl AssetState {
    /// Run one physics tick. Returns actual power in kW (positive = import, negative = export).
    pub fn update(&mut self, dt_s: f64, setpoint: f64, env: &TickEnvironment) -> f64 {
        match self {
            Self::Ev(inner) => inner.update(dt_s, setpoint, env),
            Self::Heater(inner) => inner.update(dt_s, setpoint, env),
            Self::Pv(inner) => {
                let gen = inner.update(dt_s, setpoint, env);
                -gen // PV generation is negative (export)
            }
            Self::Battery(inner) => inner.update(dt_s, setpoint, env),
            Self::BaseLoad(inner) => inner.update(dt_s, setpoint, env),
        }
    }

    /// Forward projection. Returns a self-describing TimeSeries over [now, now + timespan].
    pub fn forecast(&self, timespan: Duration) -> TimeSeries {
        match self {
            Self::Ev(inner) => inner.forecast(timespan),
            Self::Heater(inner) => inner.forecast(timespan),
            Self::Pv(inner) => inner.forecast(timespan),
            Self::Battery(inner) => inner.forecast(timespan),
            Self::BaseLoad(inner) => inner.forecast(timespan),
        }
    }

    /// Historical power data. Returns a self-describing TimeSeries over [now - timespan, now].
    pub fn history(&self, timespan: Duration, history: &AssetHistoryBuffer) -> TimeSeries {
        match self {
            Self::Ev(inner) => inner.history(timespan, history),
            Self::Heater(inner) => inner.history(timespan, history),
            Self::Pv(inner) => inner.history(timespan, history),
            Self::Battery(inner) => inner.history(timespan, history),
            Self::BaseLoad(inner) => inner.history(timespan, history),
        }
    }

    /// Asset-specific state as a key-value map.
    pub fn state_values(&self) -> HashMap<String, f64> {
        match self {
            Self::Ev(inner) => inner.state_values(),
            Self::Heater(inner) => inner.state_values(),
            Self::Pv(inner) => inner.state_values(),
            Self::Battery(inner) => inner.state_values(),
            Self::BaseLoad(inner) => inner.state_values(),
        }
    }

    /// Natural operating setpoint when no plan allocation is active.
    pub fn default_setpoint(&self) -> f64 {
        match self {
            Self::Ev(inner) => inner.default_setpoint(),
            Self::Heater(inner) => inner.default_setpoint(),
            Self::Pv(inner) => inner.default_setpoint(),
            Self::Battery(inner) => inner.default_setpoint(),
            Self::BaseLoad(inner) => inner.default_setpoint(),
        }
    }

    /// Planning interface capabilities.
    pub fn capabilities(&self, asset_id: &str) -> AssetCapabilities {
        match self {
            Self::Ev(inner) => inner.capabilities(asset_id),
            Self::Heater(inner) => inner.capabilities(asset_id),
            Self::Pv(inner) => inner.capabilities(asset_id),
            Self::Battery(inner) => inner.capabilities(asset_id),
            Self::BaseLoad(inner) => inner.capabilities(asset_id),
        }
    }

    /// UI control descriptors for POST /sim/override parameters.
    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        match self {
            Self::Ev(inner) => inner.control_schema(),
            Self::Heater(inner) => inner.control_schema(),
            Self::Pv(inner) => inner.control_schema(),
            Self::Battery(inner) => inner.control_schema(),
            Self::BaseLoad(inner) => inner.control_schema(),
        }
    }

    /// Write initial state directly (e.g. SoC jump).
    pub fn reset(&mut self, values: HashMap<String, f64>) {
        match self {
            Self::Ev(inner) => inner.reset(values),
            Self::Heater(inner) => inner.reset(values),
            Self::Pv(inner) => inner.reset(values),
            Self::Battery(inner) => inner.reset(values),
            Self::BaseLoad(inner) => inner.reset(values),
        }
    }

    /// Resolve a user request target for this asset.
    /// Returns Some((target_energy_kwh, desired_power_kw)) for energy-storage assets,
    /// or None if the asset does not support SoC-based requests.
    pub fn resolve_request_target(
        &self,
        target_soc: Option<f64>,
        desired_power_kw: Option<f64>,
    ) -> Option<(f64, f64)> {
        match self {
            Self::Ev(inner) => inner.resolve_request_target(target_soc, desired_power_kw),
            Self::Battery(inner) => inner.resolve_request_target(target_soc, desired_power_kw),
            Self::Heater(_) | Self::Pv(_) | Self::BaseLoad(_) => None,
        }
    }

    /// Default comfort rates for user requests targeting this asset type.
    pub fn default_comfort_rates(&self) -> Vec<crate::entities::asset::ComfortRate> {
        match self {
            Self::Ev(inner) => inner.default_comfort_rates(),
            Self::Heater(inner) => inner.default_comfort_rates(),
            Self::Pv(inner) => inner.default_comfort_rates(),
            Self::Battery(inner) => inner.default_comfort_rates(),
            Self::BaseLoad(inner) => inner.default_comfort_rates(),
        }
    }

    /// Default completion policy for user requests targeting this asset type.
    pub fn default_completion_policy(&self) -> crate::entities::asset::CompletionPolicy {
        match self {
            Self::Ev(inner) => inner.default_completion_policy(),
            Self::Heater(inner) => inner.default_completion_policy(),
            Self::Pv(inner) => inner.default_completion_policy(),
            Self::Battery(inner) => inner.default_completion_policy(),
            Self::BaseLoad(inner) => inner.default_completion_policy(),
        }
    }

    /// Default post-deadline comfort bid for user requests targeting this asset type.
    pub fn default_post_deadline_comfort_bid(&self) -> Option<f64> {
        match self {
            Self::Ev(inner) => inner.default_post_deadline_comfort_bid(),
            Self::Heater(inner) => inner.default_post_deadline_comfort_bid(),
            Self::Pv(inner) => inner.default_post_deadline_comfort_bid(),
            Self::Battery(inner) => inner.default_post_deadline_comfort_bid(),
            Self::BaseLoad(inner) => inner.default_post_deadline_comfort_bid(),
        }
    }

    /// Update config fields in place (e.g. capacity_kwh).
    pub fn update_config(&mut self, values: HashMap<String, f64>) {
        match self {
            Self::Ev(inner) => inner.update_config(values),
            Self::Heater(inner) => inner.update_config(values),
            Self::Pv(inner) => inner.update_config(values),
            Self::Battery(inner) => inner.update_config(values),
            Self::BaseLoad(inner) => inner.update_config(values),
        }
    }
}

// ─── Shared history() helper ─────────────────────────────────────────────────

/// Slice the ring buffer to [now − timespan, now] and return a TimeSeries.
///
/// - Extracts the `power_kw` column; drops NaN rows.
/// - Prepends a boundary point at `now − timespan` using the declared interpolation mode:
///   Step → holds the first available sample's value; Linear → same (first available value).
/// - Returns empty series if the buffer is empty or timespan ≤ 0.
pub fn history_from_buffer(
    timespan: Duration,
    history: &AssetHistoryBuffer,
    interpolation: Interpolation,
) -> TimeSeries {
    if timespan <= Duration::zero() {
        return TimeSeries::empty(interpolation);
    }
    let now = Utc::now();
    let window_start = now - timespan;

    // Slice to [window_start, now] and extract power_kw column (drop NaN).
    let points = history.to_timeline(Some((window_start, now)));
    let mut samples: Vec<(DateTime<Utc>, f64)> = points
        .iter()
        .filter_map(|p| {
            let v = p.values.get("power_kw").copied()?;
            if v.is_nan() { None } else { Some((p.ts, v)) }
        })
        .collect();

    if samples.is_empty() {
        return TimeSeries::empty(interpolation);
    }

    // Prepend boundary point at window_start if not already there.
    if samples[0].0 > window_start + Duration::milliseconds(500) {
        let boundary_value = samples[0].1; // hold first known value
        samples.insert(0, (window_start, boundary_value));
    }

    TimeSeries { samples, interpolation }
}
