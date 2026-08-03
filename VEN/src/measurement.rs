//! Generic MQTT transport for `MeasurementPort` — reusable for any single
//! real-measurement signal (PV power, baseline load power, ...). Device-agnostic:
//! all payload-format knowledge lives in `crate::measurement_translation`, passed
//! in as a `translate` function pointer. Same shape as `weather.rs`'s
//! `MqttWeatherAdapter`, minus the separate status-topic/heartbeat message (a
//! measurement's own freshness is judged purely by how recently a reading
//! arrived, via `entities::measurement::resolve_measured_kw`).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::controller::{MeasurementPort, MeasurementReading};

/// A reading is considered "transport alive" if seen within 2x this
/// interval — the expected publish cadence for a live meter/inverter feed.
const PUBLISH_HEARTBEAT_S: u64 = 60;

/// Pure helper for `is_alive()` — testable without a real `Instant`/clock wait.
fn alive_from_elapsed(elapsed: Option<Duration>, heartbeat_s: u64) -> bool {
    match elapsed {
        Some(e) => e < Duration::from_secs(heartbeat_s * 2),
        None => false,
    }
}

#[derive(Clone, Debug)]
pub struct MeasurementMqttConfig {
    pub broker_host: String,
    pub broker_port: u16,
    pub topic: String,
    pub client_id: String,
}

impl MeasurementMqttConfig {
    /// `env_prefix` e.g. `"PV_MEASUREMENT"` or `"BASE_LOAD_MEASUREMENT"` ->
    /// reads `{env_prefix}_MQTT_HOST` (required, gates presence via `?`),
    /// `_MQTT_PORT` (default 1883), `_MQTT_ROOT` (default "openadr-lab"),
    /// `_MQTT_SITE_ID` (default "default"). `signal` (e.g. "pv",
    /// "base_load") is the fixed final topic segment for this call.
    pub fn from_env(env_prefix: &str, signal: &str) -> Option<Self> {
        let broker_host = std::env::var(format!("{env_prefix}_MQTT_HOST")).ok()?;
        let broker_port = std::env::var(format!("{env_prefix}_MQTT_PORT"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1883);
        let root = std::env::var(format!("{env_prefix}_MQTT_ROOT"))
            .unwrap_or_else(|_| "openadr-lab".into());
        let site_id = std::env::var(format!("{env_prefix}_MQTT_SITE_ID"))
            .unwrap_or_else(|_| "default".into());
        let topic = format!("{root}/measurement/{site_id}/{signal}");

        let ven_name = std::env::var("VEN_NAME").unwrap_or_else(|_| "ven-1".into());
        let client_id = format!("ven-measurement-{ven_name}-{signal}-{site_id}");

        Some(Self {
            broker_host,
            broker_port,
            topic,
            client_id,
        })
    }
}

/// MQTT-backed `MeasurementPort` adapter for a single signal. `latest_kw()`
/// never touches the network — it reads the cached snapshot kept fresh by a
/// background task that owns the MQTT subscription.
pub struct MqttMeasurementAdapter {
    rx: watch::Receiver<Option<MeasurementReading>>,
    last_seen: Arc<Mutex<Option<Instant>>>,
}

impl MqttMeasurementAdapter {
    /// Spawn the background MQTT subscription task and return the adapter.
    /// `translate` parses a raw payload into `(value_kw, reading's own
    /// timestamp)` — device-specific logic lives entirely in the function
    /// passed here (see `measurement_translation.rs`), never in this file.
    pub fn spawn(
        config: MeasurementMqttConfig,
        translate: fn(&[u8]) -> Result<MeasurementReading, String>,
    ) -> Self {
        let (tx, rx) = watch::channel(None);
        let last_seen: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

        let topic = config.topic.clone();
        let mut mqtt_options = MqttOptions::new(
            config.client_id,
            config.broker_host.clone(),
            config.broker_port,
        );
        mqtt_options.set_keep_alive(Duration::from_secs(30));
        let (client, mut eventloop) = AsyncClient::new(mqtt_options, 32);

        let last_seen_task = last_seen.clone();
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    // clean_session=true (rumqttc default) means the broker
                    // forgets subscriptions on every disconnect — must
                    // resubscribe on every ConnAck, not just once.
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        if let Err(e) = client.subscribe(&topic, QoS::AtLeastOnce).await {
                            warn!(error = %e, topic = %topic, "measurement adapter: subscribe failed");
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) if p.topic == topic => {
                        match translate(&p.payload) {
                            Ok(reading) => {
                                info!(topic = %topic, "measurement adapter: reading received");
                                let _ = tx.send(Some(reading));
                                *last_seen_task.lock().unwrap() = Some(Instant::now());
                            }
                            Err(e) => {
                                warn!(error = %e, topic = %topic, "measurement adapter: rejected malformed message");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        warn!(error = %e, topic = %topic, "measurement adapter: mqtt connection error, retrying");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Self { rx, last_seen }
    }
}

#[async_trait]
impl MeasurementPort for MqttMeasurementAdapter {
    async fn latest_kw(&self) -> Option<MeasurementReading> {
        *self.rx.borrow()
    }

    fn is_alive(&self) -> bool {
        let elapsed = self
            .last_seen
            .lock()
            .unwrap()
            .map(|seen_at| seen_at.elapsed());
        alive_from_elapsed(elapsed, PUBLISH_HEARTBEAT_S)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_none_when_host_unset() {
        // No env vars set for this unique prefix -> None.
        assert!(MeasurementMqttConfig::from_env("UNSET_TEST_PREFIX_XYZ", "pv").is_none());
    }

    #[test]
    fn from_env_builds_topic_from_defaults() {
        std::env::set_var("TESTPFX1_MQTT_HOST", "broker.local");
        let cfg = MeasurementMqttConfig::from_env("TESTPFX1", "pv").unwrap();
        assert_eq!(cfg.broker_host, "broker.local");
        assert_eq!(cfg.broker_port, 1883);
        assert_eq!(cfg.topic, "openadr-lab/measurement/default/pv");
        std::env::remove_var("TESTPFX1_MQTT_HOST");
    }

    #[test]
    fn from_env_honors_overrides() {
        std::env::set_var("TESTPFX2_MQTT_HOST", "broker.local");
        std::env::set_var("TESTPFX2_MQTT_PORT", "8883");
        std::env::set_var("TESTPFX2_MQTT_ROOT", "myroot");
        std::env::set_var("TESTPFX2_MQTT_SITE_ID", "site42");
        let cfg = MeasurementMqttConfig::from_env("TESTPFX2", "base_load").unwrap();
        assert_eq!(cfg.broker_port, 8883);
        assert_eq!(cfg.topic, "myroot/measurement/site42/base_load");
        std::env::remove_var("TESTPFX2_MQTT_HOST");
        std::env::remove_var("TESTPFX2_MQTT_PORT");
        std::env::remove_var("TESTPFX2_MQTT_ROOT");
        std::env::remove_var("TESTPFX2_MQTT_SITE_ID");
    }

    #[test]
    fn dead_when_no_reading_ever_received() {
        assert!(!alive_from_elapsed(None, PUBLISH_HEARTBEAT_S));
    }

    #[test]
    fn alive_within_2x_heartbeat() {
        let elapsed = Duration::from_secs(PUBLISH_HEARTBEAT_S * 2 - 1);
        assert!(alive_from_elapsed(Some(elapsed), PUBLISH_HEARTBEAT_S));
    }

    #[test]
    fn dead_past_2x_heartbeat() {
        let elapsed = Duration::from_secs(PUBLISH_HEARTBEAT_S * 2 + 1);
        assert!(!alive_from_elapsed(Some(elapsed), PUBLISH_HEARTBEAT_S));
    }
}
