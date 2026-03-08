pub mod actors;
pub mod energy;
pub mod persist;
pub mod power_model;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::SensorSnapshot;
use crate::profile::Profile;
use crate::reactor::Setpoints;
use crate::state::UserOverrides;
use actors::{Battery, EvCharger, Heater, PvInverter};
use energy::EnergyCounter;

/// Full simulator state — persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimState {
    pub ev: Option<EvCharger>,
    pub heater: Option<Heater>,
    pub pv: Option<PvInverter>,
    pub battery: Option<Battery>,
    pub base_load_w: f64,
    pub energy: EnergyCounter,
    pub net_power_w: f64,
    pub import_w: f64,
    pub export_w: f64,
    pub voltage_v: f64,
    pub last_tick: DateTime<Utc>,
}

impl SimState {
    /// Initialize from profile configuration.
    pub fn from_profile(profile: &Profile) -> Self {
        Self {
            ev: profile.devices.ev.as_ref().map(EvCharger::from_config),
            heater: profile.devices.heater.as_ref().map(Heater::from_config),
            pv: profile.devices.pv.as_ref().map(PvInverter::from_config),
            battery: profile.devices.battery.as_ref().map(Battery::from_config),
            base_load_w: profile.devices.base_load_w,
            energy: EnergyCounter::new(),
            net_power_w: 0.0,
            import_w: 0.0,
            export_w: 0.0,
            voltage_v: 230.0,
            last_tick: Utc::now(),
        }
    }

    /// Run one simulation tick.
    pub fn tick(&mut self, dt_s: f64, setpoints: &Setpoints, now: DateTime<Utc>, overrides: &UserOverrides) {
        let hour = now.format("%H").to_string().parse::<f64>().unwrap_or(12.0)
            + now.format("%M").to_string().parse::<f64>().unwrap_or(0.0) / 60.0;

        // Apply device spec overrides each tick (shadow profile values)
        if let (Some(ref mut ev), Some(max_kw)) = (&mut self.ev, overrides.ev_max_charge_kw) {
            ev.max_charge_kw = max_kw;
        }
        if let (Some(ref mut ev), Some(target)) = (&mut self.ev, overrides.ev_soc_target) {
            ev.soc_target = target;
        }
        if let (Some(ref mut ev), Some(plugged)) = (&mut self.ev, overrides.ev_plugged) {
            ev.plugged = plugged;
        }
        if let (Some(ref mut h), Some(max_kw)) = (&mut self.heater, overrides.heater_max_kw) {
            h.max_kw = max_kw;
        }
        if let (Some(ref mut h), Some(min)) = (&mut self.heater, overrides.heater_temp_min_c) {
            h.temp_min_c = min;
        }
        if let (Some(ref mut h), Some(max)) = (&mut self.heater, overrides.heater_temp_max_c) {
            h.temp_max_c = max;
        }
        if let (Some(ref mut h), Some(amb)) = (&mut self.heater, overrides.ambient_temp_c) {
            h.ambient_temp_c = amb;
        }
        if let (Some(ref mut pv), Some(rated)) = (&mut self.pv, overrides.pv_rated_kw) {
            pv.rated_kw = rated;
        }
        if let Some(base) = overrides.base_load_w {
            self.base_load_w = base;
        }

        // Update actors
        let ev_w = if let Some(ref mut ev) = self.ev {
            ev.update(dt_s, setpoints.ev_charge_kw) * 1000.0
        } else {
            0.0
        };

        let heater_w = if let Some(ref mut heater) = self.heater {
            heater.update(dt_s, setpoints.heater_kw) * 1000.0
        } else {
            0.0
        };

        let pv_gen_w = if let Some(ref mut pv) = self.pv {
            pv.update(dt_s, setpoints.pv_export_limit_kw, hour, overrides.pv_irradiance) * 1000.0
        } else {
            0.0
        };

        // Battery: commanded by setpoints.battery_kw (0.0 = hold in Stage 1)
        let battery_w = if let Some(ref mut bat) = self.battery {
            bat.update(dt_s, setpoints.battery_kw) * 1000.0
        } else {
            0.0
        };

        // Compute net power (battery positive = import, negative = export)
        let result = power_model::compute_net_power(self.base_load_w, ev_w + battery_w, heater_w, pv_gen_w);
        self.net_power_w = result.net_w;
        self.import_w = result.import_w;
        self.export_w = result.export_w;
        self.voltage_v = result.voltage_v;

        // Integrate energy
        self.energy.integrate(result.net_w, dt_s);

        self.last_tick = now;
    }

    /// Build a SensorSnapshot for backward compatibility with /sensors endpoint.
    pub fn to_sensor_snapshot(&self) -> SensorSnapshot {
        SensorSnapshot {
            id: Uuid::new_v4(),
            ts: self.last_tick,
            temperature_c: self.heater.as_ref().map(|h| h.temp_c),
            power_w: Some(self.net_power_w),
            voltage_v: Some(self.voltage_v),
            raw: serde_json::json!({
                "source": "simulator",
                "import_w": self.import_w,
                "export_w": self.export_w,
                "base_load_w": self.base_load_w,
            }),
        }
    }

    /// Build a SimSnapshot for the /sim endpoint.
    pub fn to_sim_snapshot(&self) -> SimSnapshot {
        SimSnapshot {
            ts: self.last_tick,
            net_power_w: self.net_power_w,
            import_w: self.import_w,
            export_w: self.export_w,
            voltage_v: self.voltage_v,
            base_load_w: self.base_load_w,
            import_kwh: self.energy.import_kwh,
            export_kwh: self.energy.export_kwh,
            ev: self.ev.as_ref().map(|ev| EvSnapshot {
                soc: ev.soc,
                plugged: ev.plugged,
                current_kw: ev.current_kw,
                max_charge_kw: ev.max_charge_kw,
                soc_target: ev.soc_target,
                battery_kwh: ev.battery_kwh,
            }),
            heater: self.heater.as_ref().map(|h| HeaterSnapshot {
                temp_c: h.temp_c,
                current_kw: h.current_kw,
                max_kw: h.max_kw,
                temp_min_c: h.temp_min_c,
                temp_max_c: h.temp_max_c,
            }),
            pv: self.pv.as_ref().map(|pv| PvSnapshot {
                irradiance: pv.irradiance,
                export_limit_kw: pv.export_limit_kw,
                current_kw: pv.current_kw,
                rated_kw: pv.rated_kw,
            }),
            battery: self.battery.as_ref().map(|b| BatterySnapshot {
                soc: b.soc,
                current_kw: b.current_kw,
                capacity_kwh: b.capacity_kwh,
                max_charge_kw: b.max_charge_kw,
                max_discharge_kw: b.max_discharge_kw,
                min_soc: b.min_soc,
            }),
        }
    }
}

/// API response for GET /sim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimSnapshot {
    pub ts: DateTime<Utc>,
    pub net_power_w: f64,
    pub import_w: f64,
    pub export_w: f64,
    pub voltage_v: f64,
    pub base_load_w: f64,
    pub import_kwh: f64,
    pub export_kwh: f64,
    pub ev: Option<EvSnapshot>,
    pub heater: Option<HeaterSnapshot>,
    pub pv: Option<PvSnapshot>,
    pub battery: Option<BatterySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvSnapshot {
    pub soc: f64,
    pub plugged: bool,
    pub current_kw: f64,
    pub max_charge_kw: f64,
    pub soc_target: f64,
    pub battery_kwh: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaterSnapshot {
    pub temp_c: f64,
    pub current_kw: f64,
    pub max_kw: f64,
    pub temp_min_c: f64,
    pub temp_max_c: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvSnapshot {
    pub irradiance: f64,
    pub export_limit_kw: Option<f64>, // active export cap; None = no limit
    pub current_kw: f64,
    pub rated_kw: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatterySnapshot {
    pub soc: f64,
    pub current_kw: f64,    // positive = charging (import), negative = discharging (export)
    pub capacity_kwh: f64,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub min_soc: f64,
}
