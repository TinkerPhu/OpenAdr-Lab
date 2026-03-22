pub mod energy;
pub mod persist;
pub mod power_model;

// Re-export asset types so existing call sites using `simulator::assets::*` still compile.
pub mod assets {
    pub use crate::assets::*;
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::assets::{
    AssetConfig, AssetState, BaseLoad, Battery, BatteryState, EvCharger, EvState, Heater,
    PvInverter,
};
use crate::controller::trace::AssetHistoryBuffer;
use crate::models::SensorSnapshot;
use crate::profile::{AssetProfile, BaseLoadConfig, Profile};
use crate::state::UserOverrides;
use energy::EnergyCounter;

/// One entry in the generic asset list.
/// Config is NOT stored here — it lives in `SimState.asset_configs` (parallel by index).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id: String,
    /// Mutable physics state. Written by the dispatcher every tick.
    pub state: AssetState,
    /// Last commanded setpoint (kW, signed).
    pub setpoint_kw: f64,
    /// Actual power from the last tick (kW). Positive = import, negative = export.
    pub last_power_kw: f64,
    /// Cumulative energy for this asset since startup.
    pub energy: EnergyCounter,
    /// Per-asset history ring buffer. Initialized empty in CP1; wired in CP2.
    /// Ephemeral — not persisted to disk.
    #[serde(skip, default = "default_history_buffer")]
    pub history: AssetHistoryBuffer,
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
    /// Physics config — parallel to `assets` by index, loaded from profile.
    pub asset_configs: Vec<AssetConfig>,
    /// Mutable state + history for each asset.
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

    /// Look up entry + config by id (immutable).
    pub fn find_asset(&self, id: &str) -> Option<(&AssetEntry, &AssetConfig)> {
        self.assets
            .iter()
            .zip(self.asset_configs.iter())
            .find(|(e, _)| e.id == id)
    }

    /// Look up entry + config by id (mutable). Uses index to satisfy borrow checker.
    pub fn find_asset_mut(&mut self, id: &str) -> Option<(&mut AssetEntry, &mut AssetConfig)> {
        let idx = self.assets.iter().position(|a| a.id == id)?;
        Some((&mut self.assets[idx], &mut self.asset_configs[idx]))
    }

    /// Iterator over (entry, config) pairs — parallel by index.
    pub fn iter_assets(&self) -> impl Iterator<Item = (&AssetEntry, &AssetConfig)> {
        self.assets.iter().zip(self.asset_configs.iter())
    }

    /// Convenience accessor: returns the EvState if an "ev" asset exists.
    pub fn ev_state(&self) -> Option<&EvState> {
        self.asset("ev").and_then(|e| {
            if let AssetState::Ev(s) = &e.state {
                Some(s)
            } else {
                None
            }
        })
    }

    /// Convenience accessor: returns the BatteryState if a "battery" asset exists.
    pub fn battery_state(&self) -> Option<&BatteryState> {
        self.asset("battery").and_then(|e| {
            if let AssetState::Battery(s) = &e.state {
                Some(s)
            } else {
                None
            }
        })
    }

    /// Convenience accessor: returns the Battery config if a "battery" asset exists.
    pub fn battery_config(&self) -> Option<&Battery> {
        self.find_asset("battery").and_then(|(_, cfg)| {
            if let AssetConfig::Battery(b) = cfg {
                Some(b)
            } else {
                None
            }
        })
    }

    /// Initialize from profile configuration, preferring `assets` list over legacy `devices`.
    pub fn from_profile(profile: &Profile) -> Self {
        let mut configs: Vec<AssetConfig> = Vec::new();
        let mut entries: Vec<AssetEntry> = Vec::new();

        if !profile.assets.is_empty() {
            for ap in &profile.assets {
                let (cfg, state) = asset_config_and_state_from_profile(ap);
                let setpoint_kw = cfg.default_setpoint(&state);
                entries.push(AssetEntry {
                    id: ap.id().to_string(),
                    state,
                    setpoint_kw,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                    history: AssetHistoryBuffer::new(3600),
                });
                configs.push(cfg);
            }
        } else {
            // Fall back to legacy `devices` format
            let dev = &profile.devices;
            if let Some(c) = &dev.ev {
                let cfg = AssetConfig::Ev(EvCharger::from_config(c));
                let state = AssetState::Ev(EvCharger::initial_state(c));
                let sp = cfg.default_setpoint(&state);
                entries.push(AssetEntry {
                    id: c.id.clone(),
                    state,
                    setpoint_kw: sp,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                    history: AssetHistoryBuffer::new(3600),
                });
                configs.push(cfg);
            }
            if let Some(c) = &dev.heater {
                let cfg = AssetConfig::Heater(Heater::from_config(c));
                let state = AssetState::Heater(Heater::initial_state(c));
                let sp = cfg.default_setpoint(&state);
                entries.push(AssetEntry {
                    id: c.id.clone(),
                    state,
                    setpoint_kw: sp,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                    history: AssetHistoryBuffer::new(3600),
                });
                configs.push(cfg);
            }
            if let Some(c) = &dev.pv {
                let cfg = AssetConfig::Pv(PvInverter::from_config(c));
                let state = AssetState::Pv(PvInverter::initial_state(c));
                let sp = cfg.default_setpoint(&state);
                entries.push(AssetEntry {
                    id: c.id.clone(),
                    state,
                    setpoint_kw: sp,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                    history: AssetHistoryBuffer::new(3600),
                });
                configs.push(cfg);
            }
            if let Some(c) = &dev.battery {
                let cfg = AssetConfig::Battery(Battery::from_config(c));
                let state = AssetState::Battery(Battery::initial_state(c));
                let sp = cfg.default_setpoint(&state);
                entries.push(AssetEntry {
                    id: c.id.clone(),
                    state,
                    setpoint_kw: sp,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                    history: AssetHistoryBuffer::new(3600),
                });
                configs.push(cfg);
            }
            if dev.base_load_w > 0.0 {
                let c = BaseLoadConfig {
                    id: "base_load".to_string(),
                    baseline_kw: dev.base_load_w / 1000.0,
                };
                let cfg = AssetConfig::BaseLoad(BaseLoad::from_config(&c));
                let state = AssetState::BaseLoad(BaseLoad::initial_state(&c));
                let sp = cfg.default_setpoint(&state);
                entries.push(AssetEntry {
                    id: c.id.clone(),
                    state,
                    setpoint_kw: sp,
                    last_power_kw: 0.0,
                    energy: EnergyCounter::new(),
                    history: AssetHistoryBuffer::new(3600),
                });
                configs.push(cfg);
            }
        }

        Self {
            asset_configs: configs,
            assets: entries,
            grid: GridMeter::default(),
            last_tick: Utc::now(),
        }
    }

    /// Run one simulation tick.
    pub fn tick(
        &mut self,
        dt_s: f64,
        setpoints: HashMap<String, f64>,
        now: DateTime<Utc>,
        overrides: &UserOverrides,
    ) {
        let hour = now.format("%H").to_string().parse::<f64>().unwrap_or(12.0)
            + now.format("%M").to_string().parse::<f64>().unwrap_or(0.0) / 60.0;

        let irradiance = overrides.pv_irradiance.unwrap_or_else(|| {
            if hour >= 6.0 && hour <= 18.0 {
                let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
                angle.sin()
            } else {
                0.0
            }
        });

        let dt = chrono::Duration::milliseconds((dt_s * 1000.0) as i64);
        let mut total_kw = 0.0;

        for (cfg, entry) in self.asset_configs.iter_mut().zip(self.assets.iter_mut()) {
            // ── Set env fields in config before step() ────────────────────
            match cfg {
                AssetConfig::Pv(pv) => pv.irradiance = irradiance,
                AssetConfig::Heater(h) => {
                    h.ambient_temp_c = overrides.ambient_temp_c.unwrap_or(10.0)
                }
                _ => {}
            }

            // ── Apply UserOverride config mutations ───────────────────────
            match cfg {
                AssetConfig::Ev(ev) => {
                    if let Some(v) = overrides.ev_max_charge_kw {
                        ev.max_charge_kw = v;
                    }
                    if let Some(v) = overrides.ev_soc_target {
                        ev.soc_target = v;
                    }
                    if let Some(b) = overrides.ev_plugged {
                        if let AssetState::Ev(s) = &mut entry.state {
                            s.plugged = b;
                        }
                    }
                }
                AssetConfig::Heater(h) => {
                    if let Some(v) = overrides.heater_max_kw {
                        h.max_kw = v;
                    }
                    if let Some(v) = overrides.heater_temp_min_c {
                        h.temp_min_c = v;
                    }
                    if let Some(v) = overrides.heater_temp_max_c {
                        h.temp_max_c = v;
                    }
                }
                AssetConfig::Pv(pv) => {
                    if let Some(v) = overrides.pv_rated_kw {
                        pv.rated_kw = v;
                    }
                }
                AssetConfig::BaseLoad(bl) => {
                    if let Some(w) = overrides.base_load_w {
                        bl.baseline_kw = w / 1000.0;
                    }
                }
                _ => {}
            }

            // ── Dispatch physics ──────────────────────────────────────────
            let sp = setpoints
                .get(&entry.id)
                .copied()
                .unwrap_or_else(|| cfg.default_setpoint(&entry.state));
            let (new_state, actual_kw) = cfg.step(&entry.state, sp, dt);
            entry.state = new_state;
            entry.last_power_kw = actual_kw;
            entry.setpoint_kw = sp;
            entry.energy.integrate(actual_kw * 1000.0, dt_s);
            total_kw += actual_kw;
        }

        // ── Derive grid meter ─────────────────────────────────────────────
        let import_kw = total_kw.max(0.0);
        let export_kw = (-total_kw).max(0.0);
        let dt_h = dt_s / 3600.0;

        self.grid.net_power_w = total_kw * 1000.0;
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
            if let AssetState::Heater(s) = &e.state {
                Some(s.temperature_c)
            } else {
                None
            }
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
        for (entry, cfg) in self.iter_assets() {
            let values = cfg.state_values(&entry.state);
            assets_map.insert(
                entry.id.clone(),
                AssetSnapshot {
                    power_kw: entry.last_power_kw,
                    values,
                },
            );
        }

        SimSnapshot {
            ts: self.last_tick,
            grid: GridSnapshot {
                net_power_w: self.grid.net_power_w,
                voltage_v: self.grid.voltage_v,
                import_kwh: self.grid.import_kwh,
                export_kwh: self.grid.export_kwh,
            },
            assets: assets_map,
        }
    }
}

/// Convert a profile AssetProfile entry into (AssetConfig, initial AssetState).
fn asset_config_and_state_from_profile(ap: &AssetProfile) -> (AssetConfig, AssetState) {
    match ap {
        AssetProfile::Battery(c) => (
            AssetConfig::Battery(Battery::from_config(c)),
            AssetState::Battery(Battery::initial_state(c)),
        ),
        AssetProfile::Ev(c) => (
            AssetConfig::Ev(EvCharger::from_config(c)),
            AssetState::Ev(EvCharger::initial_state(c)),
        ),
        AssetProfile::Heater(c) => (
            AssetConfig::Heater(Heater::from_config(c)),
            AssetState::Heater(Heater::initial_state(c)),
        ),
        AssetProfile::Pv(c) => (
            AssetConfig::Pv(PvInverter::from_config(c)),
            AssetState::Pv(PvInverter::initial_state(c)),
        ),
        AssetProfile::BaseLoad(c) => (
            AssetConfig::BaseLoad(BaseLoad::from_config(c)),
            AssetState::BaseLoad(BaseLoad::initial_state(c)),
        ),
    }
}

fn default_history_buffer() -> AssetHistoryBuffer {
    AssetHistoryBuffer::new(3600)
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

/// Grid meter snapshot in the /sim response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridSnapshot {
    pub net_power_w: f64,
    pub voltage_v: f64,
    pub import_kwh: f64,
    pub export_kwh: f64,
}

/// API response for GET /sim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimSnapshot {
    pub ts: DateTime<Utc>,
    pub grid: GridSnapshot,
    pub assets: HashMap<String, AssetSnapshot>,
}
