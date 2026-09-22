//! Publishing this VEN's live state to the lab broker.
//!
//! The outbound half of `openadr-lab/fleet/<venName>/…`. Same shape as
//! `measurement.rs` and `weather.rs`, which subscribe on the *house* broker;
//! this publishes on `lab-mqtt`, the lab's own. The split is by direction of
//! travel: the house broker carries what the site measures, `lab-mqtt` carries
//! what the lab generates (phase 0 §6.1).
//!
//! Why a side channel at all, when the VTN already receives reports: a report
//! describes intervals that have *closed*, on the report cadence. Watching
//! twenty sites react to a capacity limit needs the value now, not after the
//! interval containing it has finished. The two are different jobs and this is
//! deliberately the lossy one — QoS 0, retained, no delivery guarantee.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tracing::{info, warn};

use crate::controller::telemetry_port::TelemetryPort;

/// Where this VEN publishes, and as whom.
#[derive(Clone, Debug)]
pub struct FleetMqttConfig {
    pub broker_host: String,
    pub broker_port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    /// Topic root, `openadr-lab/fleet/<ven_name>`.
    pub topic_root: String,
    pub client_id: String,
}

impl FleetMqttConfig {
    /// Read the configuration, or `None` when this VEN does not publish.
    ///
    /// `FLEET_MQTT_HOST` is the gate: absent means "not part of a monitored
    /// fleet", which is a normal deployment and not a degraded one. The port
    /// defaults to **1884**, the lab broker — `:1883` is always the house
    /// broker and `:1884` always this one, so the number alone says which is
    /// meant (§6.1).
    pub fn from_env(ven_name: &str) -> Option<Self> {
        Self::from_vars(ven_name, |k| std::env::var(k).ok())
    }

    /// The parsing, over any source of variables.
    ///
    /// Separate from `from_env` so it can be tested without touching process
    /// state: these names are fixed rather than prefixed, so tests that set
    /// them would collide with each other under a parallel runner. Same
    /// reasoning as the `determinism` rule's injectable clock -- an ambient
    /// dependency that tests must fight is one that should be a parameter.
    fn from_vars(ven_name: &str, get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let broker_host = get("FLEET_MQTT_HOST").filter(|v| !v.is_empty())?;
        let broker_port = get("FLEET_MQTT_PORT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1884);
        let root = get("FLEET_MQTT_ROOT").unwrap_or_else(|| "openadr-lab".into());
        // Empty is treated as unset: the test broker runs anonymous and its
        // compose file passes "" rather than omitting the variable.
        let non_empty = |k: &str| get(k).filter(|v| !v.is_empty());
        Some(Self {
            broker_host,
            broker_port,
            username: non_empty("FLEET_MQTT_USERNAME"),
            password: non_empty("FLEET_MQTT_PASSWORD"),
            topic_root: format!("{root}/fleet/{ven_name}"),
            client_id: format!("ven-fleet-{ven_name}"),
        })
    }
}

/// Publishes to the lab broker; reconnects on its own.
pub struct FleetMqttPublisher {
    client: AsyncClient,
    topic_root: String,
    connected: Arc<AtomicBool>,
}

impl FleetMqttPublisher {
    /// Connect and start the event loop.
    ///
    /// `rumqttc` reconnects internally, so the loop is spawned once and
    /// outlives any broker restart. `connected` tracks what the loop last saw,
    /// which is what `/health` and the VEN UI report.
    pub fn spawn(config: FleetMqttConfig) -> Self {
        let mut opts = MqttOptions::new(
            config.client_id.clone(),
            config.broker_host.clone(),
            config.broker_port,
        );
        opts.set_keep_alive(std::time::Duration::from_secs(30));
        if let (Some(u), Some(p)) = (&config.username, &config.password) {
            opts.set_credentials(u.clone(), p.clone());
        }
        // Last will: if this VEN dies without saying goodbye, the broker tells
        // subscribers on its behalf. A fleet view must distinguish "quiet" from
        // "gone", and a VEN that has crashed cannot make that distinction
        // itself -- which is exactly why the broker holds the message.
        let status_topic = format!("{}/status", config.topic_root);
        opts.set_last_will(rumqttc::LastWill::new(
            status_topic.clone(),
            serde_json::json!({"state": "offline"}).to_string(),
            QoS::AtLeastOnce,
            true,
        ));

        let (client, mut eventloop) = AsyncClient::new(opts, 32);
        let connected = Arc::new(AtomicBool::new(false));

        let loop_connected = connected.clone();
        let host = config.broker_host.clone();
        let port = config.broker_port;
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        if !loop_connected.swap(true, Ordering::Relaxed) {
                            info!(host, port, "fleet telemetry broker connected");
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        if loop_connected.swap(false, Ordering::Relaxed) {
                            warn!(host, port, error = %e, "fleet telemetry broker lost");
                        }
                        // rumqttc retries on its own; this only paces the log.
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });

        let publisher = Self {
            client,
            topic_root: config.topic_root,
            connected,
        };
        publisher.announce_online();
        publisher
    }

    /// Say we are here, retained, so a subscriber that connects later still
    /// learns this VEN exists. The last will above replaces it if we vanish.
    fn announce_online(&self) {
        let client = self.client.clone();
        let topic = format!("{}/status", self.topic_root);
        tokio::spawn(async move {
            let body = serde_json::json!({"state": "online"}).to_string();
            if let Err(e) = client.publish(&topic, QoS::AtLeastOnce, true, body).await {
                warn!(topic, error = %e, "could not announce fleet status");
            }
        });
    }
}

#[async_trait]
impl TelemetryPort for FleetMqttPublisher {
    async fn publish_telemetry(&self, body: serde_json::Value) {
        let topic = format!("{}/telemetry", self.topic_root);
        // QoS 0 and retained: a dropped sample is replaced by the next one a
        // few seconds later, so redelivery would cost more than it is worth --
        // but a subscriber joining mid-stream should not have to wait for the
        // next tick to see anything.
        if let Err(e) = self
            .client
            .publish(&topic, QoS::AtMostOnce, true, body.to_string())
            .await
        {
            // Debug, not warn: the publish queue filling while the broker is
            // away is the expected state of an offline VEN, and one line per
            // tick per VEN would bury everything else.
            tracing::debug!(topic, error = %e, "fleet telemetry publish dropped");
        }
    }

    async fn publish_trace(&self, body: serde_json::Value) {
        let topic = format!("{}/trace", self.topic_root);
        // QoS 1 and *not* retained, unlike telemetry: a decision is not a
        // state to catch up on -- a subscriber joining later must not be told
        // about an event arrival from an hour ago as though it just happened --
        // but neither is it replaced a few seconds later, so losing one
        // silently breaks the reaction chain it belongs to (§6.3).
        if let Err(e) = self
            .client
            .publish(&topic, QoS::AtLeastOnce, false, body.to_string())
            .await
        {
            tracing::debug!(topic, error = %e, "fleet trace publish dropped");
        }
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    /// The gate: no host means no publisher, which is a normal deployment for
    /// a VEN outside a monitored fleet -- not an error, not degraded.
    #[test]
    fn config_is_absent_without_a_host() {
        assert!(FleetMqttConfig::from_vars("ven-1", vars(&[])).is_none());
        assert!(
            FleetMqttConfig::from_vars("ven-1", vars(&[("FLEET_MQTT_HOST", "")])).is_none(),
            "an empty host is unset, not a broker named the empty string"
        );
    }

    #[test]
    fn config_defaults_to_the_lab_broker_port() {
        let c = FleetMqttConfig::from_vars("ven-7", vars(&[("FLEET_MQTT_HOST", "lab-mqtt")]))
            .expect("host is set");
        assert_eq!(c.broker_port, 1884, ":1884 is always the lab broker");
        assert_eq!(c.topic_root, "openadr-lab/fleet/ven-7");
        assert_eq!(c.client_id, "ven-fleet-ven-7");
    }

    /// The test broker runs anonymous and its compose file passes empty
    /// strings rather than omitting the variables. Empty credentials must mean
    /// "anonymous", not "log in as the user named ''".
    #[test]
    fn empty_credentials_are_treated_as_absent() {
        let c = FleetMqttConfig::from_vars(
            "ven-1",
            vars(&[
                ("FLEET_MQTT_HOST", "lab-mqtt"),
                ("FLEET_MQTT_USERNAME", ""),
                ("FLEET_MQTT_PASSWORD", ""),
            ]),
        )
        .unwrap();
        assert!(c.username.is_none());
        assert!(c.password.is_none());
    }

    #[test]
    fn credentials_are_carried_when_set() {
        let c = FleetMqttConfig::from_vars(
            "ven-1",
            vars(&[
                ("FLEET_MQTT_HOST", "lab-mqtt"),
                ("FLEET_MQTT_PORT", "1885"),
                ("FLEET_MQTT_USERNAME", "ven"),
                ("FLEET_MQTT_PASSWORD", "secret"),
            ]),
        )
        .unwrap();
        assert_eq!(c.broker_port, 1885);
        assert_eq!(c.username.as_deref(), Some("ven"));
        assert_eq!(c.password.as_deref(), Some("secret"));
    }
}
