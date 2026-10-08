use serde::{Deserialize, Serialize};

/// Tracks cumulative energy import and export in kWh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyCounter {
    pub import_kwh: f64,
    pub export_kwh: f64,
}

impl Default for EnergyCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl EnergyCounter {
    pub fn new() -> Self {
        Self {
            import_kwh: 0.0,
            export_kwh: 0.0,
        }
    }

    /// Integrate power over a time step.
    /// `net_w`: positive = import, negative = export
    /// `dt_s`: time step in seconds
    pub fn integrate(&mut self, net_w: f64, dt_s: f64) {
        let dt_h = crate::entities::units::dt_h_from_s(dt_s);
        let energy_kwh =
            crate::entities::units::energy_kwh(crate::entities::units::kw_from_w(net_w), dt_h);

        if energy_kwh > 0.0 {
            self.import_kwh += energy_kwh;
        } else {
            self.export_kwh += -energy_kwh;
        }
    }
}
