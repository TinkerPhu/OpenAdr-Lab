// ── SimulatorPort trait and snapshot types ────────────────────────────────────
pub mod simulator_port;
pub use simulator_port::{AssetSnapshot, SimSnapshot, SimulatorPort};

// ── VtnPort trait and typed OpenADR structs ───────────────────────────────────
pub mod vtn_port;
#[cfg(test)]
pub use simulator_port::GridSnapshot;
pub use vtn_port::VtnPort;

// ── SolverPort trait and request type ─────────────────────────────────────────
pub mod solver_port;
pub use solver_port::{SolveRequest, SolverPort};

// ── AssetMilpContext port trait and contract types (R-23) ─────────────────────
pub mod asset_milp_port;
#[allow(unused_imports)]
// public re-export surface; consumers import via milp_planner::asset_port
pub use asset_milp_port::{
    AssetKind, AssetMilpContext, AssetMilpParams, BatteryScalars, EvScalars, HeaterScalars,
    MilpLoadMode,
};

// ── HistoryPort trait ──────────────────────────────────────────────────────────
pub mod history_port;
pub use history_port::HistoryPort;

// ── SettingsPort trait (WP4.2, BL-19) ─────────────────────────────────────────
pub mod settings_port;
pub use settings_port::SettingsPort;

// ── WeatherForecastPort trait ──────────────────────────────────────────────────
pub mod measurement_port;
pub mod weather_port;
pub use measurement_port::{MeasurementPort, MeasurementReading, NoopMeasurementPort};
pub use weather_port::{NoopWeatherPort, WeatherForecastPort};

// ── OpenADR interface ─────────────────────────────────────────────────────────
pub mod openadr_interface;
pub mod rate_schedule;

// ── Planning & dispatch ───────────────────────────────────────────────────────
pub mod arbiter;
pub mod capacity_forecast;
pub mod dispatcher;
pub mod envelope;
pub mod envelope_forecast;
pub mod milp_interactions;
pub mod milp_planner;
pub mod timeline;

// ── Monitoring & reporting ────────────────────────────────────────────────────
pub mod monitor;
pub(crate) mod report_intervals;
pub mod reporter;

// ── User requests ─────────────────────────────────────────────────────────────
pub mod user_request;

// ── Observability ─────────────────────────────────────────────────────────────
pub mod trace;
