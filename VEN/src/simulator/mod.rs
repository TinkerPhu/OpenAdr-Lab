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
    AssetConfig, AssetHistoryBuffer, AssetState, BaseLoad, Battery, BatteryState, EvCharger,
    EvState, Grid, Heater, PvInverter,
};
use crate::models::SensorSnapshot;
use crate::profile::{AssetProfile, BaseLoadConfig, Profile};
use energy::EnergyCounter;

/// Tracks the user-induced irradiance perturbation between ticks.
///
/// While the user drags the irradiance slider, the offset is set to
/// `slider_position − natural_irradiance`. After release the offset decays
/// exponentially (EMA with factor `pv_alpha`) until it reaches zero, at which
/// point the simulation resumes tracking the sin model with no lag.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PvSmoothingState {
    /// Current perturbation above (or below) the natural sin model. Zero = no override.
    pub irradiance_offset: f64,
}

/// Tracks the user-induced base load perturbation between ticks.
///
/// While the user drags the base load slider, the offset is set to
/// `slider_value − baseline_kw_profile`. After release the offset decays
/// exponentially (EMA with factor `base_load_alpha`) until it reaches zero.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaseLoadSmoothingState {
    /// Perturbation above (or below) the profile baseline (kW). Zero = no override.
    pub load_offset_kw: f64,
}

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
    /// Grid virtual asset — implements the full `Asset` trait (id, current_state,
    /// history, capability). Updated each tick with net power + VTN limits.
    /// Not part of `asset_configs` / `assets` (Grid is read-only, not dispatched).
    #[serde(skip, default)]
    pub grid_asset: Grid,
    /// PV irradiance EMA state for Behaviour B smoothing. Ephemeral — resets on restart.
    #[serde(skip, default)]
    pub pv_smoothing: PvSmoothingState,
    /// Base load EMA state for Behaviour B smoothing. Ephemeral — resets on restart.
    #[serde(skip, default)]
    pub base_load_smoothing: BaseLoadSmoothingState,
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
            grid_asset: Grid::new(),
            pv_smoothing: PvSmoothingState::default(),
            base_load_smoothing: BaseLoadSmoothingState::default(),
            last_tick: Utc::now(),
        }
    }

    /// Run one simulation tick.
    ///
    /// Inject parameters implement Behaviour B (pv_irradiance + EMA smoothing) and
    /// Behaviour C (frozen env/state while active, snap-back on release):
    /// - `pv_irradiance_override`: if Some, freeze PV irradiance; if None and was active,
    ///   EMA-blend back to natural model.
    /// - `pv_alpha`: EMA factor for PV blend-back (0.0–1.0; default 0.1).
    /// - `ambient_temp_c_override`: if Some, override heater ambient temp; else use 10.0°C.
    /// - `base_load_kw_override`: if Some, one-shot: captures offset then cleared by sim loop.
    /// - `base_load_alpha`: EMA factor for base load blend-back (0.0–1.0; default 0.1).
    /// - `ev_plugged_override`: if Some, hold EV plugged state; else let physics drive it.
    pub fn tick(
        &mut self,
        dt_s: f64,
        setpoints: HashMap<String, f64>,
        now: DateTime<Utc>,
        pv_irradiance_override: Option<f64>,
        pv_alpha: f64,
        ambient_temp_c_override: Option<f64>,
        heater_temp_min_override: Option<f64>,
        heater_temp_max_override: Option<f64>,
        base_load_kw_override: Option<f64>,
        base_load_alpha: f64,
        ev_plugged_override: Option<bool>,
        ev_soc_target_override: Option<f64>,
    ) {
        let hour = now.format("%H").to_string().parse::<f64>().unwrap_or(12.0)
            + now.format("%M").to_string().parse::<f64>().unwrap_or(0.0) / 60.0;

        let natural_irradiance = if hour >= 6.0 && hour <= 18.0 {
            let angle = std::f64::consts::PI * (hour - 6.0) / 12.0;
            angle.sin()
        } else {
            0.0
        };

        // Alpha decay factor shared by all Behaviour B controls.
        // Converts per-plan-step alpha (one step = 300 s) to a per-tick factor so the
        // offset reaches (1−alpha) × original after exactly one plan step, matching
        // the forecast formula exp(−t/tau_s).
        const PLAN_STEP_S: f64 = 300.0;

        // Behaviour B — PV perturbation overlay.
        if let Some(forced) = pv_irradiance_override {
            // Re-capture offset every tick while the user is dragging.
            self.pv_smoothing.irradiance_offset = forced - natural_irradiance;
        } else {
            let per_tick_factor = (1.0 - pv_alpha).powf(dt_s / PLAN_STEP_S);
            self.pv_smoothing.irradiance_offset *= per_tick_factor;
            if self.pv_smoothing.irradiance_offset.abs() < 0.005 {
                self.pv_smoothing.irradiance_offset = 0.0;
            }
        }
        let irradiance = (natural_irradiance + self.pv_smoothing.irradiance_offset).clamp(0.0, 1.0);

        let dt = chrono::Duration::milliseconds((dt_s * 1000.0) as i64);
        let mut total_kw = 0.0;

        for (cfg, entry) in self.asset_configs.iter_mut().zip(self.assets.iter_mut()) {
            // ── Apply environment and Behaviour C state injections ────────
            match cfg {
                AssetConfig::Pv(pv) => {
                    pv.irradiance = irradiance;
                    pv.irradiance_offset = self.pv_smoothing.irradiance_offset;
                    pv.pv_alpha = pv_alpha;
                }
                AssetConfig::Heater(h) => {
                    // Behaviour C: ambient temp — hold override or use default.
                    h.ambient_temp_c = ambient_temp_c_override.unwrap_or(10.0);
                    // Behaviour C: comfort band — hold override or snap to profile defaults.
                    h.temp_min_c = heater_temp_min_override.unwrap_or(h.temp_min_c_profile);
                    h.temp_max_c = heater_temp_max_override.unwrap_or(h.temp_max_c_profile);
                }
                AssetConfig::BaseLoad(bl) => {
                    // Behaviour B: base load — one-shot sets offset; EMA decays it back.
                    if let Some(forced_kw) = base_load_kw_override {
                        self.base_load_smoothing.load_offset_kw =
                            forced_kw - bl.baseline_kw_profile;
                    } else {
                        let per_tick_factor =
                            (1.0 - base_load_alpha).powf(dt_s / PLAN_STEP_S);
                        self.base_load_smoothing.load_offset_kw *= per_tick_factor;
                        if self.base_load_smoothing.load_offset_kw.abs() < 0.005 {
                            self.base_load_smoothing.load_offset_kw = 0.0;
                        }
                    }
                    bl.baseline_kw =
                        (bl.baseline_kw_profile + self.base_load_smoothing.load_offset_kw)
                            .max(0.0);
                }
                AssetConfig::Ev(ev) => {
                    // Behaviour C: ev_plugged — hold override or snap back to profile default
                    // (plugged=true) when released. Without snap-back, releasing the inject
                    // leaves the EV permanently unplugged because there is no physics to
                    // re-plug it.
                    if let AssetState::Ev(s) = &mut entry.state {
                        s.plugged = ev_plugged_override.unwrap_or(true);
                    }
                    // Behaviour C: ev_soc_target — override BMS charge ceiling.
                    ev.soc_target = ev_soc_target_override.unwrap_or(ev.soc_target_profile);
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
