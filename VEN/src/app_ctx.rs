//! `AppCtx` — the shared handle every route handler receives, built once in
//! `main.rs` at startup. Split out of `main.rs` (file-size cap); still
//! re-exported there as `crate::AppCtx`.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use crate::assets::ControlDescriptor;
use crate::entities::asset::PlanTrigger;
use crate::entities::planner_params::PlannerObjective;
use crate::planner_events::PlannerEventTx;
use crate::simulator::SimState;
use crate::state::AppState;
use crate::vtn::VtnClient;
use crate::{controller, services};

#[derive(Clone)]
pub struct AppCtx {
    pub state: AppState,
    pub vtn: VtnClient,
    pub metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
    pub trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
    /// Pre-computed simulator schema (asset → control descriptors).
    /// Built once at startup from `profile`; route handlers access it without
    /// touching the raw `Profile` type or acquiring any lock.
    pub sim_schema: Arc<HashMap<String, Vec<ControlDescriptor>>>,
    pub sim: Arc<Mutex<SimState>>,
    pub active_objective: Arc<RwLock<PlannerObjective>>,
    pub planner_event_tx: PlannerEventTx,
    /// Persistent history store (Phase 1, A-1) — `None` when `profile.history.enabled`
    /// is false or the store failed to open.
    pub history: Option<Arc<dyn controller::HistoryPort>>,
    /// WP4.3 (BL-20): notification fan-out (ring + SSE broadcast + persistence).
    pub notifier: services::notify::Notifier,
    /// WP4.2 (BL-19): per-asset user-settings persistence (comfort curves).
    pub settings: Option<Arc<dyn controller::SettingsPort>>,
    /// Fleet telemetry publisher, so `/health` can say whether the live feed
    /// is reaching the broker. `NoTelemetry` when this VEN does not publish.
    pub telemetry: Arc<dyn controller::telemetry_port::TelemetryPort>,
    /// Weather forecast plugin port (docs/architecture/weather_forecast.md).
    /// Always present — `NoopWeatherPort` when no MQTT broker is configured,
    /// so consumers never need `Option<Arc<dyn WeatherForecastPort>>`.
    pub weather: Arc<dyn controller::WeatherForecastPort>,
    /// Weather-sourced PV forecast config (weather-forecast-visibility).
    /// `None` when the profile has no `weather_pv` section — the `/weather`
    /// route's `derived` field is `null` in that case, not an error.
    pub weather_pv_params: Option<crate::entities::asset_params::PvForecastParams>,
    /// Real-measurement MQTT feeds (real-measurement-mqtt). Always present —
    /// `NoopMeasurementPort` when no MQTT broker is configured for a given
    /// signal, so consumers never need `Option<Arc<dyn MeasurementPort>>`.
    pub pv_measurement: Arc<dyn controller::MeasurementPort>,
    pub pv_measurement_enabled: bool,
    pub base_load_measurement: Arc<dyn controller::MeasurementPort>,
    pub base_load_measurement_enabled: bool,
    /// R-59: VTN-communication-loss curtailment debounce window, seconds.
    /// `None` when the profile has no `comms_loss:` section —
    /// `/health`/`/vtn/status` report `comms_loss_active: false`
    /// unconditionally in that case. Only the primitive `debounce_s` is
    /// carried (not the full `CommsLossConfig`) because `routes/` may not
    /// import `crate::profile` types (AB-06, `tests/architecture.rs`).
    pub comms_loss_debounce_s: Option<u64>,
    /// Base-load heuristics learner config, resolved once from the profile
    /// at startup (`services::heuristics::HeuristicsConfig` — all-`Copy`
    /// primitives, safe to clone into `AppCtx`). Threading a single shared
    /// value into both `tasks::heuristics_job` and
    /// `routes::debug::preload_heuristics` closes a latent drift risk where
    /// each independently called `HeuristicsConfig::default()`.
    pub heuristics_config: services::heuristics::HeuristicsConfig,
    /// The site's physical grid import/export rating (`profile.grid`), the
    /// same clamp the tick applies to its capacity curves — carried as
    /// primitives so `GET /flexibility/capacity?start=` clamps identically
    /// without `routes/` importing `crate::profile`.
    pub grid_max_import_kw: f64,
    pub grid_max_export_kw: f64,
}
