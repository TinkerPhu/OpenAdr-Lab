//! What a user request is resolved against: each asset's own answer for its target, the energy
//! that target needs, and the power it runs at. Split out of `asset_params.rs` (500-line cap);
//! re-exported from there, so callers keep their imports.
use super::asset::{ComfortRate, CompletionPolicy};
use super::asset_params::heater_energy_above_min_kwh;

/// What a storage-shaped asset (battery, EV) declares about itself when a user request is
/// resolved against it — the asset's own answer, so no route or service reads its config to
/// decide. SoC is a fraction 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RequestDefaults {
    pub current_soc: f64,
    /// Target applied when the request names none.
    pub default_soc_target: f64,
    pub capacity_kwh: f64,
    /// Default desired power when the request names none.
    pub max_charge_kw: f64,
}

/// What a thermostat asset (the heater) answers about a request: where it is now, how much energy
/// a degree costs, and its rating. Declared by the asset (`Thermostat::thermal_request_defaults`),
/// so the energy a target temperature needs is the heater's own physics, not a route's guess.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThermalRequestDefaults {
    pub temperature_c: f64,
    pub temp_min_c: f64,
    pub thermal_mass_kwh_per_c: f64,
    /// Full power [kW]; the power a request runs at when it states none.
    pub rated_kw: f64,
}

impl ThermalRequestDefaults {
    /// Energy [kWh] from the current temperature to `target_c`; 0 when already there or above.
    pub fn energy_to_reach_kwh(&self, target_c: f64) -> f64 {
        let stored =
            |t| heater_energy_above_min_kwh(t, self.temp_min_c, self.thermal_mass_kwh_per_c);
        (stored(target_c) - stored(self.temperature_c)).max(0.0)
    }
}

/// What a user request against an asset becomes, as the asset declares it
/// (`Asset::request_kind`). Request routing asks this, never the asset's id (R-128).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    /// A charge session toward a state of charge by a departure (the EV).
    ChargeSession,
    /// A temperature target by a time (a heater).
    TemperatureTarget,
}

/// Minimal asset snapshot for user-request creation.
/// Built by the adapter layer (routes/hems.rs) from a locked SimState.
/// Pure domain type — no assets/ or simulator/ imports.
#[derive(Debug, Clone)]
pub struct AssetRequestSlice {
    pub id: String,
    /// Current SoC [0.0, 1.0] for storage assets; None for non-storage.
    pub current_soc: Option<f64>,
    /// Default SoC target when body.target_soc is None.
    pub default_soc_target: Option<f64>,
    /// Usable capacity in kWh; None for non-storage assets.
    pub capacity_kwh: Option<f64>,
    /// Max charge rate (kW); used as default desired_power when not specified.
    pub max_charge_kw: Option<f64>,
    pub completion_policy: CompletionPolicy,
    pub comfort_rates: Vec<ComfortRate>,
    /// A thermostat asset's declared default target (°C) for a request that states none
    /// (`Thermostat::default_request_target_c`); `None` for every other asset.
    pub default_target_temp_c: Option<f64>,
    /// A thermostat asset's answer for sizing a request (`ThermalRequestDefaults`); `None` for
    /// every other asset.
    pub thermal: Option<ThermalRequestDefaults>,
    /// What a request against this asset becomes (`Asset::request_kind`); `None` = the asset
    /// takes no user request (battery, PV, base load).
    pub request_kind: Option<RequestKind>,
}

impl AssetRequestSlice {
    /// The temperature a request aims for: the one it states, else the asset's declared default.
    /// The one place this is decided, for the energy and the heater session alike.
    pub fn target_temp_c(&self, requested: Option<f64>) -> Option<f64> {
        requested.or(self.default_target_temp_c)
    }

    /// The power a request runs at when it states none: the asset's own rating (storage charge
    /// rate, heater full power). `None` = the asset declares no rating (R-122: never a guessed kW).
    pub fn rated_power_kw(&self) -> Option<f64> {
        self.max_charge_kw.or(self.thermal.map(|t| t.rated_kw))
    }

    /// Energy [kWh] to the request's target, from the asset's own state: a SoC gap for storage, a
    /// temperature gap for a thermostat. `None` = no target stated and none declared; `Some(0.0)`
    /// = already there.
    pub fn energy_to_target_kwh(
        &self,
        target_soc_frac: Option<f64>,
        target_temp_c: Option<f64>,
    ) -> Option<f64> {
        if let (Some(current), Some(capacity_kwh)) = (self.current_soc, self.capacity_kwh) {
            let target = self.target_soc_frac(target_soc_frac)?;
            return Some((target - current).max(0.0) * capacity_kwh);
        }
        let thermal = self.thermal?;
        Some(thermal.energy_to_reach_kwh(self.target_temp_c(target_temp_c)?))
    }

    /// The state-of-charge target a request aims for: the one it states, else the asset's own
    /// declared default (`RequestDefaults::default_soc_target`); `None` for an asset with no SoC.
    /// The one place this is decided, so the energy to charge and the session it creates can never
    /// aim at two different targets (R-112).
    pub fn target_soc_frac(&self, requested: Option<f64>) -> Option<f64> {
        requested.or(self.default_soc_target)
    }
}
