//! `AppCtx` — everything the routes may use, built once in `boot/serve.rs` at startup. Split out
//! of `main.rs` (file-size cap); still re-exported there as `crate::AppCtx`.
//!
//! No handler takes the whole of it (R-110). Each part has a type of its own and an
//! `impl FromRef<AppCtx>`, so a handler extracts exactly the parts it uses
//! (`State(state): State<AppState>, State(roster): State<Roster>`) and its signature says what it
//! can reach.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::FromRef;
use tokio::sync::RwLock;

use crate::assets::ControlDescriptor;
use crate::entities::asset::PlanTriggerSignal;
use crate::entities::asset_params::PvForecastParams;
use crate::entities::planner_params::PlannerObjective;
use crate::planner_events::PlannerEventTx;
use crate::state::AppState;
use crate::vtn::VtnClient;
use crate::{controller, services};

pub type MetricsHandle = Arc<metrics_exporter_prometheus::PrometheusHandle>;
pub type PlanTriggerTx = Arc<tokio::sync::watch::Sender<PlanTriggerSignal>>;
/// Pre-computed simulator schema (asset → control descriptors), built once at startup from the
/// profile, so a route reads it without the raw `Profile` type or any lock.
pub type SimSchema = Arc<HashMap<String, Vec<ControlDescriptor>>>;
/// The asset roster through its port (add/cancel a shiftable load, reset/configure an asset).
pub type Roster = Arc<dyn controller::SimRosterPort>;
/// Site headroom / capacity-curve computations through their port.
pub type Headroom = Arc<dyn controller::HeadroomPort>;
/// What the asset routes read about the simulated assets, as plain data.
pub type SimRead = Arc<dyn controller::SimReadPort>;
pub type ActiveObjective = Arc<RwLock<PlannerObjective>>;
/// Persistent history store (Phase 1, A-1) — `None` when `profile.history.enabled` is false or the
/// store failed to open.
pub type History = Option<Arc<dyn controller::HistoryPort>>;
/// WP4.2 (BL-19): per-asset user-settings persistence (comfort curves).
pub type Settings = Option<Arc<dyn controller::SettingsPort>>;
/// Fleet telemetry publisher, so `/health` can say whether the live feed reaches the broker.
/// `NoTelemetry` when this VEN does not publish.
pub type Telemetry = Arc<dyn controller::telemetry_port::TelemetryPort>;
/// Weather forecast plugin port (docs/architecture/weather_forecast.md). Always present —
/// `NoopWeatherPort` when no MQTT broker is configured.
pub type Weather = Arc<dyn controller::WeatherForecastPort>;
/// Weather-sourced PV forecast config (weather-forecast-visibility). `None` when the profile has
/// no `weather_pv` section — the `/weather` route's `derived` field is `null` then, not an error.
pub type WeatherPvParams = Option<PvForecastParams>;

/// Real-measurement MQTT feeds (real-measurement-mqtt). The ports are always present —
/// `NoopMeasurementPort` when no broker is configured for a signal.
#[derive(Clone)]
pub struct MeasurementFeeds {
    pub pv: Arc<dyn controller::MeasurementPort>,
    pub pv_enabled: bool,
    pub base_load: Arc<dyn controller::MeasurementPort>,
    pub base_load_enabled: bool,
}

/// The site's physical grid import/export rating (`profile.grid`), the same clamp the tick
/// applies to its capacity curves — carried as primitives so `GET /flexibility/capacity?start=`
/// clamps identically without `routes/` importing `crate::profile`.
#[derive(Clone, Copy)]
pub struct GridRating {
    pub max_import_kw: f64,
    pub max_export_kw: f64,
}

/// R-59: VTN-communication-loss curtailment debounce window, seconds. `None` when the profile has
/// no `comms_loss:` section — `/health`/`/vtn/status` report `comms_loss_active: false` then. Only
/// the primitive is carried (not `CommsLossConfig`): `routes/` may not import `crate::profile`.
#[derive(Clone, Copy)]
pub struct CommsLoss {
    pub debounce_s: Option<u64>,
}

#[derive(Clone)]
pub struct AppCtx {
    pub state: AppState,
    pub vtn: VtnClient,
    pub metrics_handle: MetricsHandle,
    pub trigger_tx: PlanTriggerTx,
    pub sim_schema: SimSchema,
    pub roster: Roster,
    pub headroom: Headroom,
    pub sim_read: SimRead,
    pub active_objective: ActiveObjective,
    pub planner_event_tx: PlannerEventTx,
    pub history: History,
    /// WP4.3 (BL-20): notification fan-out (ring + SSE broadcast + persistence).
    pub notifier: services::notify::Notifier,
    pub settings: Settings,
    pub telemetry: Telemetry,
    pub weather: Weather,
    pub weather_pv_params: WeatherPvParams,
    pub measurements: MeasurementFeeds,
    pub comms_loss: CommsLoss,
    pub grid_rating: GridRating,
}

/// One `FromRef<AppCtx>` per part: the part a handler names is all it gets.
macro_rules! parts {
    ($($field:ident: $ty:ty),* $(,)?) => {
        $(impl FromRef<AppCtx> for $ty {
            fn from_ref(ctx: &AppCtx) -> Self {
                ctx.$field.clone()
            }
        })*
    };
}

parts! {
    state: AppState,
    vtn: VtnClient,
    metrics_handle: MetricsHandle,
    trigger_tx: PlanTriggerTx,
    sim_schema: SimSchema,
    roster: Roster,
    headroom: Headroom,
    sim_read: SimRead,
    active_objective: ActiveObjective,
    planner_event_tx: PlannerEventTx,
    history: History,
    notifier: services::notify::Notifier,
    settings: Settings,
    telemetry: Telemetry,
    weather: Weather,
    weather_pv_params: WeatherPvParams,
    measurements: MeasurementFeeds,
    comms_loss: CommsLoss,
    grid_rating: GridRating,
}
