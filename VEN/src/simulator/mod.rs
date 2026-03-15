pub mod assets;
pub mod energy;
pub mod persist;
pub mod power_model;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::SensorSnapshot;
use crate::profile::{AssetConfig, BaseLoadConfig, Profile};
use crate::state::UserOverrides;
use assets::{AssetState, BaseLoad, Battery, EvCharger, Heater, PvInverter, TickEnvironment};
use energy::EnergyCounter;

/// One entry in the generic asset list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id: String,
    pub state: AssetState,
    /// Last commanded setpoint (kW).
    pub setpoint: f64,
    /// Actual power output from the last tick (kW). Positive = import, negative = export.
    pub last_power_kw: f64,
    /// Cumulative energy for this asset since startup.
    pub energy: EnergyCounter,
}

/// Grid-level totals derived by summing all asset powers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GridMeter {
    pub net_power_w: f64,
    pub import_w: f64,
    pub export_w: f64,
    pub voltage_v: f64,
    pub import_kwh: f64,
    pub export_kwh: f64,
}

/// Full simulator state — persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimState {
    pub assets: Vec<AssetEntry>,
    pub grid: GridMeter,
    pub last_tick: DateTime<Utc>,
}

impl SimState {
    /// Look up an asset entry by id (immutable).
    pub fn asset(&self, id: &str) -> Option<&AssetEntry> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// Look up an asset entry by id (mutable).
    pub fn asset_mut(&mut self, id: &str) -> Option<&mut AssetEntry> {
        self.assets.iter_mut().find(|a| a.id == id)
    }

    /// Convenience accessor: returns the EvCharger if an "ev" asset exists.
    pub fn ev(&self) -> Option<&EvCharger> {
        self.asset("ev").and_then(|e| {
            if let AssetState::Ev(ev) = &e.state { Some(ev) } else { None }
        })
    }

    /// Convenience accessor: returns the Battery if a "battery" asset exists.
    pub fn battery(&self) -> Option<&Battery> {
        self.asset("battery").and_then(|e| {
            if let AssetState::Battery(b) = &e.state { Some(b) } else { None }
        })
    }

    /// Initialize from profile configuration, preferring `assets` list over legacy `devices`.
    pub fn from_profile(profile: &Profile) -> Self {
        let mut entries: Vec<AssetEntry> = Vec::new();

        if !profile.assets.is_empty() {
            for cfg in &profile.assets {
                let state = match cfg {
                    AssetConfig::Ev(c) => AssetState::Ev(EvCharger::from_config(c)),
                    AssetConfig::Heater(c) => AssetState::Heater(Heater::from_config(c)),
                    AssetConfig::Pv(c) => AssetState::Pv(PvInverter::from_config(c)),
                    AssetConfig::Battery(c) => AssetState::Battery(Battery::from_config(c)),
                    AssetConfig::BaseLoad(c) => AssetState::BaseLoad(BaseLoad::from_config(c)),
                };
                let setpoint = state.default_setpoint();
                entries.push(AssetEntry {
                    id: cfg.id().to_string(),
                    state,
                    setpoint,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                });
            }
        } else {
            // Fall back to legacy `devices` format
            let dev = &profile.devices;
            if let Some(c) = &dev.ev {
                let state = AssetState::Ev(EvCharger::from_config(c));
                let sp = state.default_setpoint();
                entries.push(AssetEntry { id: c.id.clone(), state, setpoint: sp, last_power_kw: 0.0, energy: EnergyCounter::new() });
            }
            if let Some(c) = &dev.heater {
                let state = AssetState::Heater(Heater::from_config(c));
                let sp = state.default_setpoint();
                entries.push(AssetEntry { id: c.id.clone(), state, setpoint: sp, last_power_kw: 0.0, energy: EnergyCounter::new() });
            }
            if let Some(c) = &dev.pv {
                let state = AssetState::Pv(PvInverter::from_config(c));
                let sp = state.default_setpoint();
                entries.push(AssetEntry { id: c.id.clone(), state, setpoint: sp, last_power_kw: 0.0, energy: EnergyCounter::new() });
            }
            if let Some(c) = &dev.battery {
                let state = AssetState::Battery(Battery::from_config(c));
                let sp = state.default_setpoint();
                entries.push(AssetEntry { id: c.id.clone(), state, setpoint: sp, last_power_kw: 0.0, energy: EnergyCounter::new() });
            }
            if dev.base_load_w > 0.0 {
                let c = BaseLoadConfig { id: "base_load".to_string(), baseline_kw: dev.base_load_w / 1000.0 };
                let state = AssetState::BaseLoad(BaseLoad::from_config(&c));
                let sp = state.default_setpoint();
                entries.push(AssetEntry { id: c.id.clone(), state, setpoint: sp, last_power_kw: 0.0, energy: EnergyCounter::new() });
            }
        }

        Self {
            assets: entries,
            grid: GridMeter::default(),
            last_tick: Utc::now(),
        }
    }

    /// Run one simulation tick.
    pub fn tick(&mut self, dt_s: f64, setpoints: HashMap<String, f64>, now: DateTime<Utc>, overrides: &UserOverrides) {
        let hour = now.format("%H").to_string().parse::<f64>().unwrap_or(12.0)
            + now.format("%M").to_string().parse::<f64>().unwrap_or(0.0) / 60.0;

        // Build TickEnvironment
        let mut env = TickEnvironment::new();
        env.insert("hour_of_day".to_string(), hour);
        if let Some(amb) = overrides.ambient_temp_c {
            env.insert("ambient_temp_c".to_string(), amb);
        }
        if let Some(irr) = overrides.pv_irradiance {
            env.insert("pv_irradiance".to_string(), irr);
        }

        // Apply device spec overrides each tick (shadow profile values)
        for asset in &mut self.assets {
            match asset.id.as_str() {
                "ev" => {
                    if let AssetState::Ev(ev) = &mut asset.state {
                        if let Some(max_kw) = overrides.ev_max_charge_kw { ev.max_charge_kw = max_kw; }
                        if let Some(target) = overrides.ev_soc_target { ev.soc_target = target; }
                        if let Some(plugged) = overrides.ev_plugged { ev.plugged = plugged; }
                    }
                }
                "heater" => {
                    if let AssetState::Heater(h) = &mut asset.state {
                        if let Some(max_kw) = overrides.heater_max_kw { h.max_kw = max_kw; }
                        if let Some(min) = overrides.heater_temp_min_c { h.temp_min_c = min; }
                        if let Some(max) = overrides.heater_temp_max_c { h.temp_max_c = max; }
                    }
                }
                "pv" => {
                    if let AssetState::Pv(pv) = &mut asset.state {
                        if let Some(rated) = overrides.pv_rated_kw { pv.rated_kw = rated; }
                    }
                }
                "battery" => {
                    if let AssetState::Battery(_bat) = &mut asset.state {
                        // Battery state is controlled via POST /sim/reset/battery and PUT /sim/config/battery
                    }
                }
                "base_load" => {
                    if let AssetState::BaseLoad(bl) = &mut asset.state {
                        if let Some(w) = overrides.base_load_w { bl.baseline_kw = w / 1000.0; }
                    }
                }
                _ => {}
            }
        }

        // Tick each asset; accumulate grid power
        let mut total_kw = 0.0;
        for asset in &mut self.assets {
            let sp = setpoints.get(&asset.id).copied()
                .unwrap_or_else(|| asset.state.default_setpoint());
            let power_kw = asset.state.update(dt_s, sp, &env);
            asset.last_power_kw = power_kw;
            asset.setpoint = sp;
            // Integrate per-asset energy (in watts to match EnergyCounter interface)
            asset.energy.integrate(power_kw * 1000.0, dt_s);
            total_kw += power_kw;
        }

        // Derive grid meter
        let net_kw = total_kw;
        let import_kw = net_kw.max(0.0);
        let export_kw = (-net_kw).max(0.0);
        let dt_h = dt_s / 3600.0;

        self.grid.net_power_w = net_kw * 1000.0;
        self.grid.import_w = import_kw * 1000.0;
        self.grid.export_w = export_kw * 1000.0;
        self.grid.voltage_v = power_model::random_voltage();
        self.grid.import_kwh += import_kw * dt_h;
        self.grid.export_kwh += export_kw * dt_h;

        self.last_tick = now;
    }

    /// Build a SensorSnapshot for backward compatibility with /sensors endpoint.
    pub fn to_sensor_snapshot(&self) -> SensorSnapshot {
        let temp_c = self.asset("heater").and_then(|e| {
            if let AssetState::Heater(h) = &e.state { Some(h.temp_c) } else { None }
        });
        SensorSnapshot {
            id: Uuid::new_v4(),
            ts: self.last_tick,
            temperature_c: temp_c,
            power_w: Some(self.grid.net_power_w),
            voltage_v: Some(self.grid.voltage_v),
            raw: serde_json::json!({
                "source": "simulator",
                "import_w": self.grid.import_w,
                "export_w": self.grid.export_w,
            }),
        }
    }

    /// Build a SimSnapshot for the /sim endpoint.
    pub fn to_sim_snapshot(&self) -> SimSnapshot {
        let mut assets_map = HashMap::new();
        for entry in &self.assets {
            let values = entry.state.state_values();
            assets_map.insert(entry.id.clone(), AssetSnapshot {
                power_kw: entry.last_power_kw,
                values,
            });
        }
        SimSnapshot {
            ts: self.last_tick,
            net_power_w: self.grid.net_power_w,
            import_w: self.grid.import_w,
            export_w: self.grid.export_w,
            voltage_v: self.grid.voltage_v,
            import_kwh: self.grid.import_kwh,
            export_kwh: self.grid.export_kwh,
            assets: assets_map,
        }
    }
}

/// Per-asset snapshot in the /sim response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSnapshot {
    /// Actual power from the last tick (kW). Positive = import, negative = export.
    pub power_kw: f64,
    /// Asset-specific state values flattened into the same JSON object as power_kw.
    #[serde(flatten)]
    pub values: HashMap<String, f64>,
}

/// API response for GET /sim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimSnapshot {
    pub ts: DateTime<Utc>,
    pub net_power_w: f64,
    pub import_w: f64,
    pub export_w: f64,
    pub voltage_v: f64,
    pub import_kwh: f64,
    pub export_kwh: f64,
    pub assets: HashMap<String, AssetSnapshot>,
}
