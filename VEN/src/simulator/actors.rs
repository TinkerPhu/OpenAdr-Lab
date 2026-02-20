use serde::{Deserialize, Serialize};

use crate::profile::{EvConfig, HeaterConfig, PvConfig};

/// EV Charger: consumes power to charge battery (positive = import)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvCharger {
    pub soc: f64,           // 0.0..1.0
    pub plugged: bool,
    pub max_charge_kw: f64,
    pub battery_kwh: f64,
    pub soc_target: f64,
    pub current_kw: f64,    // actual charging power this tick
}

impl EvCharger {
    pub fn from_config(cfg: &EvConfig) -> Self {
        Self {
            soc: cfg.initial_soc,
            plugged: true,
            max_charge_kw: cfg.max_charge_kw,
            battery_kwh: cfg.battery_kwh,
            soc_target: cfg.soc_target.max(cfg.initial_soc),
            current_kw: cfg.max_charge_kw, // start charging at max
        }
    }

    /// Update state. `commanded_kw` is the desired charge rate from reactor.
    /// Positive = charging (import), negative = discharging/V2G (export).
    /// Returns actual power (positive = import, negative = export).
    pub fn update(&mut self, dt_s: f64, commanded_kw: f64) -> f64 {
        if !self.plugged {
            self.current_kw = 0.0;
            return 0.0;
        }

        // Clamp commanded power to device limits (bidirectional)
        let kw = commanded_kw.clamp(-self.max_charge_kw, self.max_charge_kw);

        // Hard stops at SoC bounds: can't charge above 100% or discharge below 0%
        let kw = if kw > 0.0 && self.soc >= 1.0 {
            0.0
        } else if kw < 0.0 && self.soc <= 0.0 {
            0.0
        } else {
            kw
        };

        // Update SOC: positive kw charges, negative discharges
        let dt_h = dt_s / 3600.0;
        self.soc += (kw * dt_h) / self.battery_kwh;
        self.soc = self.soc.clamp(0.0, 1.0);

        self.current_kw = kw;
        kw
    }

    pub fn power_w(&self) -> f64 {
        self.current_kw * 1000.0
    }
}

/// Heater: consumes power for heating (positive = import)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heater {
    pub temp_c: f64,
    pub max_kw: f64,
    pub temp_min_c: f64,
    pub temp_max_c: f64,
    pub current_kw: f64,
    pub ambient_temp_c: f64,
    thermal_mass: f64, // kWh per degree C
}

impl Heater {
    pub fn from_config(cfg: &HeaterConfig) -> Self {
        Self {
            temp_c: cfg.temp_initial_c,
            max_kw: cfg.max_kw,
            temp_min_c: cfg.temp_min_c,
            temp_max_c: cfg.temp_max_c,
            current_kw: 0.0,
            ambient_temp_c: 10.0, // fixed outdoor temp
            thermal_mass: 2.0,    // kWh/°C — typical small building
        }
    }

    /// Update state. `commanded_kw` is desired heating power from reactor.
    /// Returns actual power consumed (positive = import).
    pub fn update(&mut self, dt_s: f64, commanded_kw: f64) -> f64 {
        let dt_h = dt_s / 3600.0;

        // Ambient heat loss: proportional to temp difference
        let loss_rate_kw = (self.temp_c - self.ambient_temp_c) * 0.1; // 0.1 kW per degree
        let heat_loss_kwh = loss_rate_kw * dt_h;

        // Clamp commanded power
        let kw = commanded_kw.clamp(0.0, self.max_kw);
        let heat_input_kwh = kw * dt_h;

        // Net temperature change
        let net_energy_kwh = heat_input_kwh - heat_loss_kwh;
        self.temp_c += net_energy_kwh / self.thermal_mass;

        // Thermostat override: if below min, force max heating; if above max, stop
        if self.temp_c < self.temp_min_c {
            let forced_kw = self.max_kw;
            self.current_kw = forced_kw;
            return forced_kw;
        }
        if self.temp_c > self.temp_max_c {
            self.current_kw = 0.0;
            return 0.0;
        }

        self.current_kw = kw;
        kw
    }

    pub fn power_w(&self) -> f64 {
        self.current_kw * 1000.0
    }
}

/// PV Inverter: generates power (negative = export)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvInverter {
    pub rated_kw: f64,
    pub irradiance: f64,     // 0.0..1.0 current solar irradiance factor
    pub curtailment: f64,    // 0.0..1.0 fraction curtailed (0 = no curtailment)
    pub current_kw: f64,     // actual output (positive value, sign applied in power model)
}

impl PvInverter {
    pub fn from_config(cfg: &PvConfig) -> Self {
        Self {
            rated_kw: cfg.rated_kw,
            irradiance: 0.0,
            curtailment: 0.0,
            current_kw: 0.0,
        }
    }

    /// Update state. `curtailment_fraction` 0.0 = full output, 1.0 = fully curtailed.
    /// `irradiance_override` overrides the time-based sin model when Some(v).
    /// Returns actual generation in kW (positive value — sign convention applied by power_model).
    pub fn update(&mut self, _dt_s: f64, curtailment_fraction: f64, hour_of_day: f64, irradiance_override: Option<f64>) -> f64 {
        self.irradiance = irradiance_override.unwrap_or_else(|| {
            // Sinusoidal irradiance: sun from 6am to 6pm
            if hour_of_day >= 6.0 && hour_of_day <= 18.0 {
                let angle = std::f64::consts::PI * (hour_of_day - 6.0) / 12.0;
                angle.sin()
            } else {
                0.0
            }
        });

        self.curtailment = curtailment_fraction.clamp(0.0, 1.0);
        let output = self.rated_kw * self.irradiance * (1.0 - self.curtailment);
        self.current_kw = output;
        output
    }

    /// Power in watts (positive = generation)
    pub fn generation_w(&self) -> f64 {
        self.current_kw * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ev_charges_and_stops_at_full() {
        let cfg = EvConfig {
            max_charge_kw: 10.0,
            initial_soc: 0.99,
            battery_kwh: 10.0,
            soc_target: 1.0,
        };
        let mut ev = EvCharger::from_config(&cfg);
        // Charge at max for enough time to reach 100%
        for _ in 0..1000 {
            ev.update(1.0, 10.0);
        }
        assert!((ev.soc - 1.0).abs() < 0.001);
        // At full SoC, charging command returns 0
        assert_eq!(ev.update(1.0, 10.0), 0.0);
    }

    #[test]
    fn ev_discharges_v2g_and_stops_at_empty() {
        let cfg = EvConfig {
            max_charge_kw: 10.0,
            initial_soc: 0.01,
            battery_kwh: 10.0,
            soc_target: 1.0,
        };
        let mut ev = EvCharger::from_config(&cfg);
        // Discharge at -10 kW until empty
        for _ in 0..1000 {
            ev.update(1.0, -10.0);
        }
        assert!((ev.soc - 0.0).abs() < 0.001);
        // At empty SoC, discharge command returns 0
        assert_eq!(ev.update(1.0, -10.0), 0.0);
    }

    #[test]
    fn pv_sinusoidal_model() {
        let cfg = PvConfig { rated_kw: 10.0 };
        let mut pv = PvInverter::from_config(&cfg);
        // Noon = peak
        let kw = pv.update(1.0, 0.0, 12.0, None);
        assert!((kw - 10.0).abs() < 0.01);
        // Midnight = zero
        let kw = pv.update(1.0, 0.0, 0.0, None);
        assert_eq!(kw, 0.0);
        // Full curtailment = zero
        let kw = pv.update(1.0, 1.0, 12.0, None);
        assert_eq!(kw, 0.0);
        // Manual irradiance override
        let kw = pv.update(1.0, 0.0, 0.0, Some(1.0)); // midnight but forced full sun
        assert!((kw - 10.0).abs() < 0.01);
    }
}
