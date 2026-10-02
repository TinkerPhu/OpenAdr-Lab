// ── SimulatorPort trait and snapshot types ────────────────────────────────────
pub mod simulator_port;
pub use simulator_port::{SimSnapshot, SimulatorPort};
// AssetSnapshot: only test code re-imports it via this path now that
// tasks/sim_tick/publish.rs's manual shiftable-runtime augmentation (its only
// non-test consumer) was deleted (shiftable-load-as-asset). Gated like
// `GridSnapshot` below rather than allow-listed.
#[cfg(test)]
pub use simulator_port::AssetSnapshot;

// ── VtnPort trait and typed OpenADR structs ───────────────────────────────────
pub mod vtn_port;
/// One rule for refusing a malformed object off the wire (see the module docs).
pub mod wire_reject;
#[cfg(test)]
pub use simulator_port::GridSnapshot;
pub use vtn_port::VtnPort;

// ── SolverPort trait and request type ─────────────────────────────────────────
pub mod solver_port;
pub use solver_port::{SolveRequest, SolverPort};

// ── AssetMilpContext port trait and contract types (R-23) ─────────────────────
pub mod asset_milp_port;
// No re-export here: every consumer imports these through
// `milp_planner::asset_port`, which is the one path the port documents. A
// second, unused path was kept alive only by `#[allow(unused_imports)]`.

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
pub mod dispatcher;
pub mod milp_interactions;
pub mod milp_planner;
pub mod timeline;
// `capacity_headroom`/`site_headroom` used to live here. They take `&SimState`
// and call `Asset::max_effort_setpoint` directly — see `simulator_port.rs`'s
// own note on why the flattened snapshot cannot answer PV's achievable range —
// so they were the domain ring importing infra. They now live in `simulator/`,
// beside the `forecast.rs` they already shared, and `controller/` imports
// neither `crate::assets` nor `crate::simulator` (checked by
// `scripts/audit_ven_architecture.py`).

// ── Monitoring & reporting ────────────────────────────────────────────────────
pub mod monitor;
pub mod report_accumulator;
pub(crate) mod report_intervals;
pub mod report_payload;
pub mod reporter;

// ── User requests ─────────────────────────────────────────────────────────────
pub mod user_request;

// ── Observability ─────────────────────────────────────────────────────────────
pub mod telemetry_port;
/// GB-49: what the VTN actually granted this VEN, read from its own token.
pub mod token_scopes;
pub mod trace;
