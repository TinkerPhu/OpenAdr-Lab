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

// ── HistoryPort trait ──────────────────────────────────────────────────────────
pub mod history_port;
pub use history_port::HistoryPort;

// ── SettingsPort trait (WP4.2, BL-19) ─────────────────────────────────────────
pub mod settings_port;
pub use settings_port::SettingsPort;

// ── WeatherForecastPort trait ──────────────────────────────────────────────────
pub mod weather_port;
pub use weather_port::{NoopWeatherPort, WeatherForecastPort};

// ── OpenADR interface ─────────────────────────────────────────────────────────
pub mod openadr_interface;

// ── Planning & dispatch ───────────────────────────────────────────────────────
pub mod dispatcher;
pub mod envelope;
pub mod milp_interactions;
pub mod milp_planner;
pub mod timeline;

// ── Monitoring & reporting ────────────────────────────────────────────────────
pub mod monitor;
pub(crate) mod report_intervals;
pub mod reporter;
pub mod residual;

// ── User requests ─────────────────────────────────────────────────────────────
pub mod user_request;

// ── Observability ─────────────────────────────────────────────────────────────
pub mod trace;
