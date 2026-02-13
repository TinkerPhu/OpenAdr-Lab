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
use actors::{EvCharger, Heater, PvInverter};
use energy::EnergyCounter;

/// Full simulator state — persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimState {
    pub ev: Option<EvCharger>,
    pub heater: Option<Heater>,
    pub pv: Option<PvInverter>,
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
    pub fn tick(&mut self, dt_s: f64, setpoints: &Setpoints, now: DateTime<Utc>) {
        let hour = now.format("%H").to_string().parse::<f64>().unwrap_or(12.0)
            + now.format("%M").to_string().parse::<f64>().unwrap_or(0.0) / 60.0;

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
            pv.update(dt_s, setpoints.pv_curtailment, hour) * 1000.0
        } else {
            0.0
        };

        // Compute net power
        let result = power_model::compute_net_power(self.base_load_w, ev_w, heater_w, pv_gen_w);
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
            }),
            heater: self.heater.as_ref().map(|h| HeaterSnapshot {
                temp_c: h.temp_c,
                current_kw: h.current_kw,
                max_kw: h.max_kw,
            }),
            pv: self.pv.as_ref().map(|pv| PvSnapshot {
                irradiance: pv.irradiance,
                curtailment: pv.curtailment,
                current_kw: pv.current_kw,
                rated_kw: pv.rated_kw,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvSnapshot {
    pub soc: f64,
    pub plugged: bool,
    pub current_kw: f64,
    pub max_charge_kw: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaterSnapshot {
    pub temp_c: f64,
    pub current_kw: f64,
    pub max_kw: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvSnapshot {
    pub irradiance: f64,
    pub curtailment: f64,
    pub current_kw: f64,
    pub rated_kw: f64,
}
