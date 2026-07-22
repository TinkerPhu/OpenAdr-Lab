//! MQTT adapter for `WeatherForecastPort` — see
//! `docs/architecture/weather_forecast.md` ("Wire contract") for the full
//! topic/schema/cadence specification this implements. Infra-layer adapter,
//! same shape as `vtn.rs` implementing `VtnPort`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::controller::WeatherForecastPort;
use crate::entities::weather::WeatherForecast;

/// Every status message that hasn't arrived within 2x this interval is
/// considered dead, per the wire contract's heartbeat cadence.
#[allow(dead_code)] // consumed by is_alive(), not yet called from production code
const STATUS_HEARTBEAT_S: u64 = 300;

#[derive(Clone, Debug)]
pub struct WeatherMqttConfig {
    pub broker_host: String,
    pub broker_port: u16,
    /// `<root>` in `<root>/weather/<site_id>/...`
    pub root: String,
    pub site_id: String,
}

impl WeatherMqttConfig {
    pub fn from_env() -> Option<Self> {
        let broker_host = std::env::var("WEATHER_MQTT_HOST").ok()?;
        let broker_port = std::env::var("WEATHER_MQTT_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1883);
        let root = std::env::var("WEATHER_MQTT_ROOT").unwrap_or_else(|_| "openadr-lab".into());
        let site_id = std::env::var("WEATHER_MQTT_SITE_ID").unwrap_or_else(|_| "default".into());
        Some(Self {
            broker_host,
            broker_port,
            root,
            site_id,
        })
    }

    fn forecast_topic(&self) -> String {
        format!("{}/weather/{}/forecast", self.root, self.site_id)
    }

    fn status_topic(&self) -> String {
        format!("{}/weather/{}/status", self.root, self.site_id)
    }
}

/// `WeatherStatusMessage` per the wire contract's Topic 2 schema.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct WeatherStatusMessage {
    pub schema_version: String,
    pub source_id: String,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub status: WeatherStatus,
    pub last_successful_fetch_at: Option<chrono::DateTime<chrono::Utc>>,
    pub consecutive_failures: Option<u32>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WeatherStatus {
    Ok,
    Stale,
    Error,
    Offline,
}

/// Validate a parsed `WeatherForecast` against the wire contract's numeric
/// bounds. Serde already enforces the required-field set (non-`Option`
/// fields fail to deserialize if absent); this catches out-of-range values
/// serde's type system can't. Rejects the *entire* message on any violation.
fn validate_forecast(f: &WeatherForecast) -> Result<(), String> {
    if !(-90.0..=90.0).contains(&f.location.latitude_deg) {
        return Err(format!(
            "latitude_deg out of range: {}",
            f.location.latitude_deg
        ));
    }
    if !(-180.0..=180.0).contains(&f.location.longitude_deg) {
        return Err(format!(
            "longitude_deg out of range: {}",
            f.location.longitude_deg
        ));
    }
    if f.samples.is_empty() {
        return Err("samples must not be empty".to_string());
    }
    for s in &f.samples {
        if s.age_h > 240 {
            return Err(format!("age_h out of range: {}", s.age_h));
        }
        if !(-60.0..=60.0).contains(&s.temperature_c) {
            return Err(format!("temperature_c out of range: {}", s.temperature_c));
        }
        if !(0.0..=1500.0).contains(&s.ghi_w_m2) {
            return Err(format!("ghi_w_m2 out of range: {}", s.ghi_w_m2));
        }
        if let Some(w) = s.wind_speed_kmh {
            if !(0.0..=300.0).contains(&w) {
                return Err(format!("wind_speed_kmh out of range: {w}"));
            }
        }
        if let Some(r) = s.rain_prob_pct {
            if !(0.0..=100.0).contains(&r) {
                return Err(format!("rain_prob_pct out of range: {r}"));
            }
        }
        if let Some(sn) = s.new_snowfall_cm {
            if !(0.0..=200.0).contains(&sn) {
                return Err(format!("new_snowfall_cm out of range: {sn}"));
            }
        }
        if let Some(v) = s.irradiance_variability {
            if !(0.0..=1.0).contains(&v) {
                return Err(format!("irradiance_variability out of range: {v}"));
            }
        }
    }
    Ok(())
}

/// Parse+validate an inbound `forecast` topic payload. Standalone (no
/// `rumqttc` event-loop dependency) so it's unit-testable without a broker.
pub fn parse_forecast_message(payload: &[u8]) -> Result<WeatherForecast, String> {
    let forecast: WeatherForecast =
        serde_json::from_slice(payload).map_err(|e| format!("parse error: {e}"))?;
    validate_forecast(&forecast)?;
    Ok(forecast)
}

/// Parse an inbound `status` topic payload.
pub fn parse_status_message(payload: &[u8]) -> Result<WeatherStatusMessage, String> {
    serde_json::from_slice(payload).map_err(|e| format!("parse error: {e}"))
}

/// MQTT-backed `WeatherForecastPort` adapter. `latest()` never touches the
/// network — it reads the cached snapshot kept fresh by a background task
/// that owns the MQTT subscription.
pub struct MqttWeatherAdapter {
    rx: watch::Receiver<Option<WeatherForecast>>,
    #[allow(dead_code)] // read by is_alive(), not yet called from production code
    last_status: std::sync::Arc<Mutex<Option<(WeatherStatusMessage, Instant)>>>,
}

impl MqttWeatherAdapter {
    /// Spawn the background MQTT subscription task and return the adapter.
    pub fn spawn(config: WeatherMqttConfig) -> Self {
        let (tx, rx) = watch::channel(None);
        let last_status: std::sync::Arc<Mutex<Option<(WeatherStatusMessage, Instant)>>> =
            std::sync::Arc::new(Mutex::new(None));

        let forecast_topic = config.forecast_topic();
        let status_topic = config.status_topic();
        // MQTT client IDs must be unique per broker connection — a duplicate
        // ID gets the previous holder disconnected ("Connection closed by
        // peer abruptly"). Multiple VEN instances legitimately share one
        // site_id (same physical site, e.g. ven-1/2/3 all monitoring
        // Zunzgen), so site_id alone collides; disambiguate with VEN_NAME
        // (already required per instance — see config.rs).
        let ven_name = std::env::var("VEN_NAME").unwrap_or_else(|_| "ven-1".into());
        let client_id = format!("ven-weather-{}-{}", ven_name, config.site_id);

        let mut mqtt_options =
            MqttOptions::new(client_id, config.broker_host.clone(), config.broker_port);
        mqtt_options.set_keep_alive(Duration::from_secs(30));
        // rumqttc defaults to a 10 KB incoming packet cap, which a real
        // multi-sample forecast payload exceeds (observed 11389 B against
        // the production feed vs. the BDD fixture's single-sample ~300 B
        // message, which never surfaced this). Outgoing stays default —
        // this adapter never publishes.
        mqtt_options.set_max_packet_size(256 * 1024, 10 * 1024);
        let (client, mut eventloop) = AsyncClient::new(mqtt_options, 32);

        let last_status_task = last_status.clone();
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    // rumqttc's default MqttOptions is clean_session=true, so the
                    // broker forgets our subscriptions on every disconnect — must
                    // resubscribe on every ConnAck (initial connect AND reconnect
                    // after a broker restart/network blip), not just once before
                    // the loop, or the adapter silently stops receiving forever
                    // after the first reconnect.
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        if let Err(e) = client.subscribe(&forecast_topic, QoS::AtLeastOnce).await {
                            warn!(error = %e, topic = %forecast_topic, "weather adapter: subscribe failed");
                        }
                        if let Err(e) = client.subscribe(&status_topic, QoS::AtLeastOnce).await {
                            warn!(error = %e, topic = %status_topic, "weather adapter: subscribe failed");
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        if p.topic == forecast_topic {
                            match parse_forecast_message(&p.payload) {
                                Ok(forecast) => {
                                    info!(source = %forecast.source_id, "weather adapter: forecast received");
                                    let _ = tx.send(Some(forecast));
                                }
                                Err(e) => {
                                    warn!(error = %e, "weather adapter: rejected malformed forecast message");
                                }
                            }
                        } else if p.topic == status_topic {
                            match parse_status_message(&p.payload) {
                                Ok(status) => {
                                    *last_status_task.lock().unwrap() =
                                        Some((status, Instant::now()));
                                }
                                Err(e) => {
                                    warn!(error = %e, "weather adapter: rejected malformed status message");
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        warn!(error = %e, "weather adapter: mqtt connection error, retrying");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Self { rx, last_status }
    }

    /// Whether the configured source has been heard from (status heartbeat)
    /// within 2x the documented heartbeat interval. `false` if no status
    /// message has ever been received.
    #[allow(dead_code)] // liveness surfacing (health endpoint) not yet wired
    pub fn is_alive(&self) -> bool {
        match self.last_status.lock().unwrap().as_ref() {
            Some((_, seen_at)) => seen_at.elapsed() < Duration::from_secs(STATUS_HEARTBEAT_S * 2),
            None => false,
        }
    }
}

#[async_trait]
impl WeatherForecastPort for MqttWeatherAdapter {
    async fn latest(&self) -> Option<WeatherForecast> {
        self.rx.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MESSAGE: &str = r#"{
        "schema_version": "1.0.0",
        "source_id": "srf_meteo",
        "location": { "latitude_deg": 47.4491, "longitude_deg": 7.8081 },
        "fetched_at": "2026-07-19T05:54:48Z",
        "samples": [
            { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 16.0, "ghi_w_m2": 97.0 }
        ]
    }"#;

    #[test]
    fn parses_valid_forecast_message() {
        let f = parse_forecast_message(VALID_MESSAGE.as_bytes()).unwrap();
        assert_eq!(f.source_id, "srf_meteo");
        assert_eq!(f.samples.len(), 1);
    }

    #[test]
    fn rejects_message_missing_required_field() {
        let missing_fetched_at = r#"{
            "schema_version": "1.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 47.4491, "longitude_deg": 7.8081 },
            "samples": []
        }"#;
        assert!(parse_forecast_message(missing_fetched_at.as_bytes()).is_err());
    }

    #[test]
    fn rejects_out_of_range_temperature() {
        let bad = r#"{
            "schema_version": "1.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 47.4491, "longitude_deg": 7.8081 },
            "fetched_at": "2026-07-19T05:54:48Z",
            "samples": [
                { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 999.0, "ghi_w_m2": 97.0 }
            ]
        }"#;
        assert!(parse_forecast_message(bad.as_bytes()).is_err());
    }

    #[test]
    fn rejects_out_of_range_ghi() {
        let bad = r#"{
            "schema_version": "1.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 47.4491, "longitude_deg": 7.8081 },
            "fetched_at": "2026-07-19T05:54:48Z",
            "samples": [
                { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 16.0, "ghi_w_m2": -5.0 }
            ]
        }"#;
        assert!(parse_forecast_message(bad.as_bytes()).is_err());
    }

    #[test]
    fn ignores_unknown_fields_forward_compatibly() {
        let with_extra = r#"{
            "schema_version": "1.0.0",
            "source_id": "srf_meteo",
            "location": { "latitude_deg": 47.4491, "longitude_deg": 7.8081 },
            "fetched_at": "2026-07-19T05:54:48Z",
            "samples": [
                { "valid_at": "2026-07-19T06:00:00Z", "age_h": 1, "temperature_c": 16.0, "ghi_w_m2": 97.0,
                  "some_future_field": "icing_risk_pct" }
            ],
            "a_totally_new_top_level_field": 42
        }"#;
        assert!(parse_forecast_message(with_extra.as_bytes()).is_ok());
    }

    #[test]
    fn parses_valid_status_message() {
        let json = r#"{ "schema_version": "1.0.0", "source_id": "srf_meteo",
            "ts": "2026-07-19T06:05:00Z", "status": "ok",
            "last_successful_fetch_at": "2026-07-19T05:54:48Z", "consecutive_failures": 0 }"#;
        let s = parse_status_message(json.as_bytes()).unwrap();
        assert_eq!(s.status, WeatherStatus::Ok);
    }

    #[test]
    fn rejects_status_message_missing_required_field() {
        let missing_status = r#"{ "schema_version": "1.0.0", "source_id": "srf_meteo",
            "ts": "2026-07-19T06:05:00Z" }"#;
        assert!(parse_status_message(missing_status.as_bytes()).is_err());
    }
}
