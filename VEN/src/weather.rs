//! MQTT adapter for `WeatherForecastPort` — see
//! `docs/architecture/weather_forecast.md` ("Wire contract") for the full
//! topic/schema/cadence specification this implements. Infra-layer adapter,
//! same shape as `vtn.rs` implementing `VtnPort`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::controller::WeatherForecastPort;
use crate::entities::weather::WeatherForecast;
pub use lab_forecast::weather_wire::{
    parse_forecast_message, parse_status_message, WeatherStatusMessage,
};

/// Every status message that hasn't arrived within 2x this interval is
/// considered dead, per the wire contract's heartbeat cadence.
const STATUS_HEARTBEAT_S: u64 = 300;

/// Pure helper for `is_alive()` — testable without a real `Instant`/clock wait.
fn alive_from_elapsed(elapsed: Option<Duration>, heartbeat_s: u64) -> bool {
    match elapsed {
        Some(e) => e < Duration::from_secs(heartbeat_s * 2),
        None => false,
    }
}

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
        lab_forecast::feed_topics::weather_forecast_topic(&self.root, &self.site_id)
    }

    fn status_topic(&self) -> String {
        lab_forecast::feed_topics::weather_status_topic(&self.root, &self.site_id)
    }
}

/// MQTT-backed `WeatherForecastPort` adapter. `latest()` never touches the
/// network — it reads the cached snapshot kept fresh by a background task
/// that owns the MQTT subscription.
pub struct MqttWeatherAdapter {
    rx: watch::Receiver<Option<WeatherForecast>>,
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
        // Hometown), so site_id alone collides; disambiguate with VEN_NAME
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
}

#[async_trait]
impl WeatherForecastPort for MqttWeatherAdapter {
    async fn latest(&self) -> Option<WeatherForecast> {
        self.rx.borrow().clone()
    }

    /// Whether the configured source has been heard from (status heartbeat)
    /// within 2x the documented heartbeat interval. `false` if no status
    /// message has ever been received.
    fn is_alive(&self) -> bool {
        let elapsed = self
            .last_status
            .lock()
            .unwrap()
            .as_ref()
            .map(|(_, seen_at)| seen_at.elapsed());
        alive_from_elapsed(elapsed, STATUS_HEARTBEAT_S)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_when_no_status_ever_received() {
        assert!(!alive_from_elapsed(None, STATUS_HEARTBEAT_S));
    }

    #[test]
    fn alive_within_2x_heartbeat() {
        let elapsed = Duration::from_secs(STATUS_HEARTBEAT_S * 2 - 1);
        assert!(alive_from_elapsed(Some(elapsed), STATUS_HEARTBEAT_S));
    }

    #[test]
    fn dead_past_2x_heartbeat() {
        let elapsed = Duration::from_secs(STATUS_HEARTBEAT_S * 2 + 1);
        assert!(!alive_from_elapsed(Some(elapsed), STATUS_HEARTBEAT_S));
    }
}
