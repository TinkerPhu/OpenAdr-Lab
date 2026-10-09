//! Grid meter derivation from a tick's total asset power — split out of
//! `mod.rs::tick()` to keep that file under the file-size cap (mirrors
//! `pv_preview.rs`).

use chrono::{DateTime, Utc};

use super::{power_model, SimState};
use crate::entities::units::{dt_h_from_s, w_from_kw};

impl SimState {
    /// Derive `self.grid`'s import/export/voltage from this tick's summed
    /// modelled-asset power. The simulated meter is exactly that sum — the
    /// site's unmetered consumption is modelled as the `base_load` asset, not
    /// as a separate meter perturbation (see
    /// `docs/architecture/forecasting_model.md`).
    pub(super) fn derive_grid_meter(&mut self, total_kw: f64, now: DateTime<Utc>, dt_s: f64) {
        let meter_kw = total_kw;
        let import_kw = meter_kw.max(0.0);
        let export_kw = (-meter_kw).max(0.0);
        let dt_h = dt_h_from_s(dt_s);

        self.grid.net_power_w = w_from_kw(meter_kw);
        self.grid.import_w = w_from_kw(import_kw);
        self.grid.export_w = w_from_kw(export_kw);
        self.grid.voltage_v = power_model::random_voltage(&mut self.rng);
        self.grid.import_kwh += crate::entities::units::energy_kwh(import_kw, dt_h);
        self.grid.export_kwh += crate::entities::units::energy_kwh(export_kw, dt_h);

        self.last_tick = now;
    }
}
