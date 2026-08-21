//! Grid meter derivation from a tick's total asset power — split out of
//! `mod.rs::tick()` to keep that file under the file-size cap (mirrors
//! `pv_preview.rs`).

use chrono::{DateTime, Utc};

use super::{power_model, SimState};

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
        let dt_h = dt_s / 3600.0;

        self.grid.net_power_w = meter_kw * 1000.0;
        self.grid.import_w = import_kw * 1000.0;
        self.grid.export_w = export_kw * 1000.0;
        self.grid.voltage_v = power_model::random_voltage(&mut self.rng);
        self.grid.import_kwh += import_kw * dt_h;
        self.grid.export_kwh += export_kw * dt_h;

        self.last_tick = now;
    }
}
