//! Grid connection limits — split out of `schema.rs` to keep that file under
//! the file-size cap (same pattern as `weather_pv.rs`/`polling.rs`).

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct GridConfig {
    /// Physical import limit at the meter or main breaker (kW).
    /// Default: 25.0 kW — typical residential 3-phase 32 A supply.
    #[serde(default = "super::defaults::default_max_import_kw")]
    pub max_import_kw: f64,
    /// Physical export limit (inverter / grid-tie maximum) (kW).
    /// Default: 10.0 kW.
    #[serde(default = "super::defaults::default_max_export_kw")]
    pub max_export_kw: f64,
}
