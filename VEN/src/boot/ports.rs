//! Every outbound adapter this process talks to, built in one place.
//!
//! All five optional ones share a shape: an env-var config that is `None` on a
//! normal deployment, and a no-op implementation of the same port when it is.
//! That is deliberate — a VEN with no weather broker, no measurement feeds and
//! no fleet channel must behave exactly as it did before those features
//! existed, which a `None` port gives for free and an `Option<Arc<dyn …>>`
//! threaded through every consumer would not.

use std::sync::Arc;
use tracing::{error, info};

use crate::config::Config;
use crate::controller::{
    HistoryPort, MeasurementPort, NoopMeasurementPort, NoopWeatherPort, SettingsPort, SolverPort,
    VtnPort, WeatherForecastPort,
};
use crate::profile::Profile;
use crate::vtn::VtnClient;
use crate::{controller, fleet_telemetry, history_store, measurement, measurement_translation};

pub struct Ports {
    /// The concrete client as well as the port: `AppCtx` hands the concrete
    /// type to routes that drive a VTN interaction directly.
    pub vtn: VtnClient,
    pub vtn_port: Arc<dyn VtnPort>,
    pub solver: Arc<dyn SolverPort>,
    pub weather: Arc<dyn WeatherForecastPort>,
    pub pv_measurement: Arc<dyn MeasurementPort>,
    pub base_load_measurement: Arc<dyn MeasurementPort>,
    pub telemetry: Arc<dyn controller::telemetry_port::TelemetryPort>,
    /// `None` when `profile.history.enabled` is false or the SQLite file could
    /// not be opened — the run continues without history rather than failing.
    pub history: Option<Arc<dyn HistoryPort>>,
    /// The same store under its other port (WP4.2 comfort-curve settings).
    pub settings: Option<Arc<dyn SettingsPort>>,
}

impl Ports {
    pub fn build(cfg: &Config, profile: &Profile, data_dir: &str) -> Self {
        let vtn = VtnClient::new(
            cfg.vtn_base_url.clone(),
            cfg.client_id.clone(),
            cfg.client_secret.clone(),
            cfg.ven_name.clone(),
        );
        let store = open_history_store(profile, data_dir);
        Self {
            vtn_port: Arc::new(vtn.clone()),
            vtn,
            solver: Arc::new(controller::milp_planner::MilpSolver),
            weather: weather_port(),
            pv_measurement: measurement_port(
                "PV_MEASUREMENT",
                "pv",
                measurement_translation::parse_pv_measurement,
            ),
            base_load_measurement: measurement_port(
                "BASE_LOAD_MEASUREMENT",
                "base_load",
                measurement_translation::parse_base_load_measurement,
            ),
            telemetry: telemetry_port(&cfg.ven_name),
            history: store.clone().map(|s| s as Arc<dyn HistoryPort>),
            settings: store.map(|s| s as Arc<dyn SettingsPort>),
        }
    }
}

/// Weather forecast plugin (docs/architecture/weather_forecast.md): active
/// only when `WEATHER_MQTT_HOST` is set.
fn weather_port() -> Arc<dyn WeatherForecastPort> {
    match crate::weather::WeatherMqttConfig::from_env() {
        Some(cfg) => {
            info!(
                broker = %cfg.broker_host,
                site_id = %cfg.site_id,
                "weather forecast plugin: MQTT adapter configured"
            );
            Arc::new(crate::weather::MqttWeatherAdapter::spawn(cfg))
        }
        None => Arc::new(NoopWeatherPort),
    }
}

/// One real-measurement MQTT feed (real-measurement-mqtt). A second,
/// profile-level gate (`measurements.pv_enabled` / `.base_load_enabled`) is
/// checked at the tick-loop call site — both gates must allow a signal for it
/// to take effect.
fn measurement_port(
    env_prefix: &str,
    signal: &str,
    parse: measurement::MeasurementParser,
) -> Arc<dyn MeasurementPort> {
    match measurement::MeasurementMqttConfig::from_env(env_prefix, signal) {
        Some(cfg) => {
            info!(broker = %cfg.broker_host, topic = %cfg.topic, signal, "measurement feed: MQTT adapter configured");
            Arc::new(measurement::MqttMeasurementAdapter::spawn(cfg, parse))
        }
        None => Arc::new(NoopMeasurementPort),
    }
}

/// The live side channel a fleet view reads, separate from the reports the VTN
/// receives (fleet-monitor phase 0 §6). No `FLEET_MQTT_HOST` means this VEN is
/// not part of a monitored fleet — a normal deployment, so the port becomes a
/// no-op rather than an error.
fn telemetry_port(ven_name: &str) -> Arc<dyn controller::telemetry_port::TelemetryPort> {
    match fleet_telemetry::FleetMqttConfig::from_env(ven_name) {
        Some(cfg) => {
            info!(
                broker = %cfg.broker_host,
                port = cfg.broker_port,
                topic_root = %cfg.topic_root,
                "fleet telemetry: publishing"
            );
            Arc::new(fleet_telemetry::FleetMqttPublisher::spawn(cfg))
        }
        None => Arc::new(controller::telemetry_port::NoTelemetry),
    }
}

/// Phase 1 (A-1/WP1.2): persistent history store, gated by
/// `profile.history.enabled`. The same SQLite store also serves as the WP4.2
/// `SettingsPort`.
fn open_history_store(
    profile: &Profile,
    data_dir: &str,
) -> Option<Arc<history_store::SqliteHistoryStore>> {
    if !profile.history.enabled {
        return None;
    }
    let history_path = format!("{data_dir}/history.sqlite");
    match history_store::SqliteHistoryStore::open(&history_path) {
        Ok(store) => Some(Arc::new(store)),
        Err(e) => {
            error!(
                "history store open failed at {history_path}: {e} — history disabled for this run"
            );
            None
        }
    }
}
