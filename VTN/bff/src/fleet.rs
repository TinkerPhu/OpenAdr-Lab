//! Receiving what the fleet publishes about itself.
//!
//! The inbound half of `openadr-lab/fleet/<venName>/…`, whose outbound half is
//! the VEN's `fleet_telemetry` module. The BFF subscribes once and holds the
//! latest message per VEN, so a fleet view can answer "what is every site
//! doing right now" without asking twenty VENs.
//!
//! This is the *live* source, and it is deliberately the lossy one. The VTN's
//! reports remain the record: durable, spec-shaped, and describing intervals
//! that have closed. Telemetry is retained at QoS 0, so a subscriber sees the
//! last value immediately and a dropped sample is simply replaced a few
//! seconds later. Anything that must be exact reads the reports.
//!
//! What this deliberately does **not** do: interpret. It stores what a VEN
//! said about itself and nothing more. A VEN's own capability, flexibility or
//! forecast is the VEN's to state (`asset-competence-assurance`), and a second
//! opinion computed here would be a competing authority for the same question.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::RwLock;

/// The last thing one VEN said, and when we heard it.
#[derive(Clone, Debug, Serialize)]
pub struct VenLiveState {
    /// The VEN's own telemetry body, passed through unchanged (`dto`).
    pub telemetry: Option<serde_json::Value>,
    /// `"online"` / `"offline"` from the status topic. `offline` is published
    /// by the broker's last-will when a VEN dies without saying goodbye, which
    /// is the only way to tell "gone" from "quiet".
    pub state: Option<String>,
    /// When the BFF received the last message from this VEN. The BFF's clock,
    /// not the VEN's: comparing the two is how §6.3 judges clock offset.
    pub received_at: DateTime<Utc>,
}

/// Everything the fleet has told us, keyed by VEN name.
///
/// `BTreeMap` so a listing is in a stable order without the caller sorting --
/// a fleet view shows the same VEN in the same row between refreshes.
pub type FleetState = Arc<RwLock<BTreeMap<String, VenLiveState>>>;

/// How the fleet subscription is doing, for `/api/health`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct FleetIngestStatus {
    pub enabled: bool,
    pub connected: bool,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// Split `openadr-lab/fleet/<ven>/<leaf>` into the VEN and the leaf.
///
/// Returns `None` for anything that is not shaped like a fleet topic, so a
/// stray publish on a neighbouring topic cannot invent a VEN. The root is a
/// parameter because it is configurable on the publishing side, and the two
/// have to agree.
pub fn parse_topic<'a>(root: &str, topic: &'a str) -> Option<(&'a str, &'a str)> {
    let prefix = format!("{root}/fleet/");
    let rest = topic.strip_prefix(&prefix)?;
    let mut parts = rest.splitn(2, '/');
    let ven = parts.next().filter(|v| !v.is_empty())?;
    let leaf = parts.next().filter(|v| !v.is_empty())?;
    Some((ven, leaf))
}

/// Apply one received message to the fleet state.
///
/// Pure so the routing rules are testable without a broker: which topics are
/// recognised, what an unparseable body does, and what a `status` message
/// means are all decisions worth pinning.
pub fn apply_message(
    state: &mut BTreeMap<String, VenLiveState>,
    ven: &str,
    leaf: &str,
    payload: &[u8],
    now: DateTime<Utc>,
) {
    let entry = state.entry(ven.to_string()).or_insert(VenLiveState {
        telemetry: None,
        state: None,
        received_at: now,
    });
    entry.received_at = now;

    match leaf {
        "telemetry" => {
            match serde_json::from_slice::<serde_json::Value>(payload) {
                Ok(body) => entry.telemetry = Some(body),
                // Keep the previous value rather than replacing it with
                // nothing: a garbled message should not erase what we know.
                Err(e) => tracing::warn!(ven, error = %e, "unreadable fleet telemetry"),
            }
        }
        "status" => {
            let parsed = serde_json::from_slice::<serde_json::Value>(payload).ok();
            entry.state = parsed
                .as_ref()
                .and_then(|v| v.get("state")?.as_str().map(str::to_string));
        }
        // Other leaves (signals, trace, plan) are published by the VEN and not
        // yet consumed here. Ignored rather than warned about: they are part
        // of the design, just not of this step.
        _ => {}
    }
}

/// Where the BFF subscribes, and as whom.
#[derive(Clone, Debug)]
pub struct FleetMqttConfig {
    pub broker_host: String,
    pub broker_port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub root: String,
}

impl FleetMqttConfig {
    /// `FLEET_MQTT_HOST` gates the whole feature, exactly as it does on the
    /// publishing side: unset means this deployment has no fleet feed, which
    /// is a configuration and not a fault.
    pub fn from_env() -> Option<Self> {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    fn from_vars(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let broker_host = get("FLEET_MQTT_HOST").filter(|v| !v.is_empty())?;
        let non_empty = |k: &str| get(k).filter(|v| !v.is_empty());
        Some(Self {
            broker_host,
            broker_port: get("FLEET_MQTT_PORT")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1884),
            username: non_empty("FLEET_MQTT_USERNAME"),
            password: non_empty("FLEET_MQTT_PASSWORD"),
            root: get("FLEET_MQTT_ROOT").unwrap_or_else(|| "openadr-lab".into()),
        })
    }

    /// The one subscription: every leaf of every VEN.
    pub fn subscription(&self) -> String {
        format!("{}/fleet/+/#", self.root)
    }
}

/// Subscribe and keep `state` current until the process ends.
pub fn spawn_ingest(
    config: FleetMqttConfig,
    state: FleetState,
    status: Arc<RwLock<FleetIngestStatus>>,
) {
    use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};

    let mut opts = MqttOptions::new(
        "vtn-bff-fleet",
        config.broker_host.clone(),
        config.broker_port,
    );
    opts.set_keep_alive(std::time::Duration::from_secs(30));
    if let (Some(u), Some(p)) = (&config.username, &config.password) {
        opts.set_credentials(u.clone(), p.clone());
    }
    let (client, mut eventloop) = AsyncClient::new(opts, 128);
    let topic = config.subscription();
    let root = config.root.clone();

    tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    // Subscribe on every (re)connect, not once at startup: a
                    // broker restart drops subscriptions, and a silent feed
                    // that looks connected is the worst of both.
                    match client.subscribe(&topic, QoS::AtLeastOnce).await {
                        Ok(()) => {
                            let mut s = status.write().await;
                            s.connected = true;
                            s.last_error = None;
                            tracing::info!(topic, "fleet ingest subscribed");
                        }
                        Err(e) => {
                            let mut s = status.write().await;
                            s.connected = false;
                            s.last_error = Some(format!("subscribe failed: {e}"));
                            tracing::warn!(topic, error = %e, "fleet ingest subscribe failed");
                        }
                    }
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    if let Some((ven, leaf)) = parse_topic(&root, &p.topic) {
                        let now = Utc::now();
                        apply_message(&mut *state.write().await, ven, leaf, &p.payload, now);
                        status.write().await.last_message_at = Some(now);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    let mut s = status.write().await;
                    if s.connected {
                        tracing::warn!(error = %e, "fleet ingest disconnected");
                    }
                    s.connected = false;
                    s.last_error = Some(e.to_string());
                    drop(s);
                    // rumqttc reconnects on its own; this only paces the log.
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn parse_topic_reads_the_ven_and_the_leaf() {
        assert_eq!(
            parse_topic("openadr-lab", "openadr-lab/fleet/ven-7/telemetry"),
            Some(("ven-7", "telemetry"))
        );
    }

    /// A stray publish on a neighbouring topic must not invent a VEN.
    #[test]
    fn parse_topic_rejects_anything_not_shaped_like_a_fleet_topic() {
        for topic in [
            "openadr-lab/measurement/ven-7/pv",
            "openadr-lab/fleet/ven-7",
            "openadr-lab/fleet//telemetry",
            "other-root/fleet/ven-7/telemetry",
            "openadr-lab/fleet/",
        ] {
            assert!(
                parse_topic("openadr-lab", topic).is_none(),
                "{topic} should not parse as a fleet topic"
            );
        }
    }

    #[test]
    fn apply_message_stores_telemetry_verbatim() {
        let mut state = BTreeMap::new();
        apply_message(
            &mut state,
            "ven-1",
            "telemetry",
            br#"{"venName":"ven-1","grid":{"net_power_w":-2500.0}}"#,
            now(),
        );
        let v = &state["ven-1"];
        assert_eq!(
            v.telemetry.as_ref().unwrap()["grid"]["net_power_w"],
            -2500.0
        );
        assert_eq!(v.received_at, now());
    }

    /// A garbled message must not erase what we already knew: stale is more
    /// useful than absent, and the receive time still says how stale.
    #[test]
    fn apply_message_keeps_the_last_good_telemetry_when_a_body_is_unreadable() {
        let mut state = BTreeMap::new();
        apply_message(&mut state, "ven-1", "telemetry", br#"{"a":1}"#, now());
        apply_message(&mut state, "ven-1", "telemetry", b"not json", now());
        assert_eq!(state["ven-1"].telemetry.as_ref().unwrap()["a"], 1);
    }

    /// The broker publishes this on a VEN's behalf when it dies without
    /// saying goodbye -- the only way to tell "gone" from "quiet".
    #[test]
    fn apply_message_records_the_last_will() {
        let mut state = BTreeMap::new();
        apply_message(
            &mut state,
            "ven-4",
            "status",
            br#"{"state":"offline"}"#,
            now(),
        );
        assert_eq!(state["ven-4"].state.as_deref(), Some("offline"));
    }

    #[test]
    fn apply_message_ignores_leaves_this_step_does_not_consume() {
        let mut state = BTreeMap::new();
        apply_message(&mut state, "ven-1", "plan", br#"{"slots":[]}"#, now());
        // The VEN is known to exist and to have spoken; nothing is claimed
        // about its telemetry.
        assert!(state["ven-1"].telemetry.is_none());
        assert_eq!(state["ven-1"].received_at, now());
    }
}
