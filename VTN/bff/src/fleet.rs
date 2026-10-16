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

use lab_core::fleet_channel::{fleet_subscription, leaf};
/// The topic shape, leaf names and broker settings are shared with the VEN.
pub use lab_core::fleet_channel::{parse_topic, BrokerConfig as FleetMqttConfig};

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
        leaf::TELEMETRY => {
            match serde_json::from_slice::<serde_json::Value>(payload) {
                Ok(body) => entry.telemetry = Some(body),
                // Keep the previous value rather than replacing it with
                // nothing: a garbled message should not erase what we know.
                Err(e) => tracing::warn!(ven, error = %e, "unreadable fleet telemetry"),
            }
        }
        leaf::STATUS => {
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

/// The site meter reading a fleet view sums, out of one VEN's telemetry body.
///
/// One function because two callers need the same answer: the live route and
/// the telemetry store. A second copy of this path expression is exactly the
/// shape `one-concept-one-function` exists to stop -- the copies would agree
/// until the day the VEN renames the field, and then disagree silently.
///
/// `None` means the VEN has not said, which is never the same as zero.
pub fn net_power_kw(telemetry: &serde_json::Value) -> Option<f64> {
    telemetry.get("grid")?.get("net_power_kw")?.as_f64()
}

/// Subscribe and keep `state` current until the process ends.
///
/// `store` is where the same messages are kept for later. It is optional
/// because the live view is useful without a database and must not be held
/// hostage to one — a deployment with no `DATABASE_URL` still gets a fleet
/// dashboard, just no history.
pub fn spawn_ingest(
    config: FleetMqttConfig,
    state: FleetState,
    status: Arc<RwLock<FleetIngestStatus>>,
    store: Option<crate::fleet_store::TelemetryWriter>,
) {
    use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};

    let mut opts = MqttOptions::new("vtn-bff-fleet", config.host.clone(), config.port);
    opts.set_keep_alive(std::time::Duration::from_secs(30));
    if let (Some(u), Some(p)) = (&config.username, &config.password) {
        opts.set_credentials(u.clone(), p.clone());
    }
    let (client, mut eventloop) = AsyncClient::new(opts, 128);
    let topic = fleet_subscription(&config.root);
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
                    if let Some((ven, ven_leaf)) = parse_topic(&root, &p.topic) {
                        let now = Utc::now();
                        apply_message(&mut *state.write().await, ven, ven_leaf, &p.payload, now);
                        // Store what was stored in the live state, from the
                        // live state -- so the history cannot end up holding a
                        // differently-parsed version of the same message.
                        if let (Some(store), leaf::TRACE) = (&store, ven_leaf) {
                            match serde_json::from_slice::<serde_json::Value>(&p.payload)
                                .ok()
                                .and_then(|body| {
                                    crate::fleet_store::TraceRow::from_message(ven, &body, now)
                                }) {
                                Some(row) => store.offer_trace(row),
                                None => tracing::warn!(ven, "unreadable fleet trace message"),
                            }
                        }
                        if let (Some(store), leaf::TELEMETRY) = (&store, ven_leaf) {
                            if let Some(body) = state
                                .read()
                                .await
                                .get(ven)
                                .and_then(|v| v.telemetry.as_ref())
                            {
                                store.offer(crate::fleet_store::TelemetryRow::from_message(
                                    ven, body, now,
                                ));
                            }
                        }
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
    fn apply_message_stores_telemetry_verbatim() {
        let mut state = BTreeMap::new();
        apply_message(
            &mut state,
            "ven-1",
            "telemetry",
            br#"{"venName":"ven-1","grid":{"net_power_kw":-2.5}}"#,
            now(),
        );
        let v = &state["ven-1"];
        assert_eq!(v.telemetry.as_ref().unwrap()["grid"]["net_power_kw"], -2.5);
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
    fn net_power_kw_reads_the_meter_out_of_a_telemetry_body() {
        let body = serde_json::json!({"grid": {"net_power_kw": -2.5}});
        assert_eq!(net_power_kw(&body), Some(-2.5));
    }

    /// Absent, non-numeric or differently shaped bodies all mean "this VEN has
    /// not said", which a sum must skip rather than read as zero.
    #[test]
    fn net_power_kw_is_absent_rather_than_zero_when_the_body_does_not_say() {
        for body in [
            serde_json::json!({}),
            serde_json::json!({"grid": {}}),
            serde_json::json!({"grid": {"net_power_kw": null}}),
            serde_json::json!({"grid": {"net_power_kw": "2.5"}}),
            serde_json::json!({"grid": 5}),
        ] {
            assert_eq!(net_power_kw(&body), None, "{body} should not yield a value");
        }
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
