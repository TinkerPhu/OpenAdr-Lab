//! Grid meter derivation from a tick's total asset power — split out of
//! `mod.rs::tick()` to keep that file under the file-size cap (mirrors
//! `pv_preview.rs`).

use chrono::{DateTime, Utc};

use super::{power_model, unmodelled_load_at, SimState};

impl SimState {
    /// Derive `self.grid`'s import/export/voltage from this tick's summed
    /// modelled-asset power plus the configured unmodelled diurnal load —
    /// the gap between the two is exactly what `site-residual` reports.
    pub(super) fn derive_grid_meter(&mut self, total_kw: f64, now: DateTime<Utc>, dt_s: f64) {
        let meter_kw = total_kw + unmodelled_load_at(now, self.unmodelled_load_kw);
        let import_kw = meter_kw.max(0.0);
        let export_kw = (-meter_kw).max(0.0);
        let dt_h = dt_s / 3600.0;

        self.grid.net_power_w = meter_kw * 1000.0;
        self.grid.import_w = import_kw * 1000.0;
        self.grid.export_w = export_kw * 1000.0;
        self.grid.voltage_v = power_model::random_voltage();
        self.grid.import_kwh += import_kw * dt_h;
        self.grid.export_kwh += export_kw * dt_h;

        self.last_tick = now;
    }
}
