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
    /// The objective in force on the VEN and who set it: its `GET /plan/objective` body from
    /// the `objective` leaf, passed through unchanged (`dto`).
    pub objective: Option<serde_json::Value>,
    /// The plan the VEN adopted last: its `GET /plan` body from the `plan` leaf, unchanged.
    pub plan: Option<serde_json::Value>,
    /// The objective the fleet commands this VEN, as the broker holds it on
    /// `command/objective` (the BFF reads its own retained commands back); `None` once released.
    pub command: Option<ObjectiveCommand<String>>,
    /// When the objective this VEN reports last changed (not set by the first one seen): the
    /// moment the fleet forecast's `before` series looks back from (`fleet_forecast`).
    pub objective_changed_at: Option<DateTime<Utc>>,
}

impl VenLiveState {
    fn new(now: DateTime<Utc>) -> Self {
        Self {
            telemetry: None,
            state: None,
            received_at: now,
            objective: None,
            plan: None,
            command: None,
            objective_changed_at: None,
        }
    }

    /// Whether the fleet's command has not reached the VEN yet: one is in force on the broker
    /// and the VEN reports another objective, or reports it set before the command was issued.
    pub fn command_pending(&self) -> bool {
        let Some(command) = &self.command else {
            return false;
        };
        let Some(objective) = &self.objective else {
            return true;
        };
        let reported = objective.get("objective").and_then(|o| o.as_str());
        let set_at = objective
            .get("set_at")
            .and_then(|t| t.as_str())
            .and_then(|t| t.parse::<DateTime<Utc>>().ok());
        reported != Some(command.objective.as_str())
            || set_at.is_none_or(|t| t < command.requested_at)
    }

    /// The objectives this VEN accepts, from its own `objective` leaf.
    pub fn available_objectives(&self) -> Option<Vec<String>> {
        let list = self
            .objective
            .as_ref()?
            .get("available_objectives")?
            .as_array()?;
        Some(
            list.iter()
                .filter_map(|o| o.as_str().map(str::to_string))
                .collect(),
        )
    }
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

use lab_core::fleet_channel::{fleet_subscription, leaf, ObjectiveCommand};
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
    let entry = state
        .entry(ven.to_string())
        .or_insert_with(|| VenLiveState::new(now));
    entry.received_at = now;

    match leaf {
        leaf::TELEMETRY => keep_readable(&mut entry.telemetry, payload, ven, leaf::TELEMETRY),
        leaf::STATUS => {
            let parsed = serde_json::from_slice::<serde_json::Value>(payload).ok();
            entry.state = parsed
                .as_ref()
                .and_then(|v| v.get("state")?.as_str().map(str::to_string));
        }
        leaf::OBJECTIVE => {
            let before = objective_name(entry.objective.as_ref());
            keep_readable(&mut entry.objective, payload, ven, leaf::OBJECTIVE);
            let after = objective_name(entry.objective.as_ref());
            if before.is_some() && before != after {
                entry.objective_changed_at = Some(now);
            }
        }
        leaf::PLAN => keep_readable(&mut entry.plan, payload, ven, leaf::PLAN),
        // An empty retained message is how a command is withdrawn.
        leaf::COMMAND_OBJECTIVE if payload.is_empty() => entry.command = None,
        leaf::COMMAND_OBJECTIVE => match serde_json::from_slice(payload) {
            Ok(command) => entry.command = Some(command),
            Err(e) => tracing::warn!(ven, error = %e, "unreadable fleet objective command"),
        },
        // Other leaves (signals, trace) are published by the VEN and not
        // consumed here. Ignored rather than warned about: they are part
        // of the design, just not of the live state.
        _ => {}
    }
}

/// The objective's name in a VEN's `objective` body.
fn objective_name(body: Option<&serde_json::Value>) -> Option<String> {
    Some(body?.get("objective")?.as_str()?.to_string())
}

/// Replace `slot` with the leaf's JSON body; keep the previous one when the body is unreadable,
/// so a garbled message does not erase what we know.
fn keep_readable(slot: &mut Option<serde_json::Value>, payload: &[u8], ven: &str, leaf: &str) {
    match serde_json::from_slice::<serde_json::Value>(payload) {
        Ok(body) => *slot = Some(body),
        Err(e) => tracing::warn!(ven, leaf, error = %e, "unreadable fleet leaf"),
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

/// The ingest client's connection settings. A VEN's plan leaf is far over rumqttc's 10 KiB
/// default (`MAX_PACKET_BYTES`); over it, the connection that carries every VEN is dropped.
fn mqtt_options(config: &FleetMqttConfig) -> rumqttc::MqttOptions {
    let mut opts = rumqttc::MqttOptions::new("vtn-bff-fleet", config.host.clone(), config.port);
    opts.set_keep_alive(std::time::Duration::from_secs(30));
    opts.set_max_packet_size(
        lab_core::fleet_channel::MAX_PACKET_BYTES,
        lab_core::fleet_channel::MAX_PACKET_BYTES,
    );
    if let (Some(u), Some(p)) = (&config.username, &config.password) {
        opts.set_credentials(u.clone(), p.clone());
    }
    opts
}

/// Subscribe and keep `state` current until the process ends. Returns the client, which also
/// publishes the fleet's objective commands (`fleet_command`).
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
) -> rumqttc::AsyncClient {
    use rumqttc::{AsyncClient, Event, Packet, QoS};

    let (client, mut eventloop) = AsyncClient::new(mqtt_options(&config), 128);
    let publisher = client.clone();
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
    publisher
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    /// A VEN's plan leaf is hundreds of kB; at rumqttc's 10 KiB default the BFF would drop the
    /// connection carrying every VEN each time one arrived.
    #[test]
    fn the_ingest_client_accepts_packets_as_large_as_a_plan() {
        let config =
            FleetMqttConfig::from_vars(|k| (k == "FLEET_MQTT_HOST").then(|| "lab-mqtt".into()))
                .unwrap();
        assert_eq!(
            mqtt_options(&config).max_packet_size(),
            lab_core::fleet_channel::MAX_PACKET_BYTES
        );
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
    fn apply_message_ignores_leaves_it_does_not_consume() {
        let mut state = BTreeMap::new();
        apply_message(&mut state, "ven-1", "trace", br#"{"kind":"x"}"#, now());
        // The VEN is known to exist and to have spoken; nothing is claimed
        // about its telemetry.
        assert!(state["ven-1"].telemetry.is_none());
        assert_eq!(state["ven-1"].received_at, now());
    }

    fn objective_body(objective: &str, set_at: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "objective": objective, "source": "fleet", "set_at": set_at,
            "profile_objective": "min_cost",
            "available_objectives": ["min_cost", "opportunistic"],
        }))
        .unwrap()
    }

    /// The objective and plan leaves are kept verbatim; a garbled body keeps the previous one.
    #[test]
    fn apply_message_keeps_the_objective_and_the_plan_verbatim() {
        let mut state = BTreeMap::new();
        let body = objective_body("opportunistic", "2026-10-16T12:00:00Z");
        apply_message(&mut state, "ven-1", "objective", &body, now());
        apply_message(&mut state, "ven-1", "plan", br#"{"slots":[1]}"#, now());
        apply_message(&mut state, "ven-1", "objective", b"not json", now());
        apply_message(&mut state, "ven-1", "plan", b"not json", now());
        let v = &state["ven-1"];
        assert_eq!(v.objective, Some(serde_json::from_slice(&body).unwrap()));
        assert_eq!(v.plan, Some(serde_json::json!({"slots": [1]})));
        assert_eq!(
            v.available_objectives(),
            Some(vec!["min_cost".to_string(), "opportunistic".to_string()])
        );
    }

    /// The broker hands the BFF its own retained command back; an empty one withdraws it, a
    /// garbled one keeps the command known before.
    #[test]
    fn apply_message_reads_the_command_and_its_withdrawal() {
        let mut state = BTreeMap::new();
        let command = br#"{"objective":"opportunistic","requested_at":"2026-10-16T12:00:00Z"}"#;
        apply_message(&mut state, "ven-1", "command/objective", command, now());
        apply_message(&mut state, "ven-1", "command/objective", b"garbled", now());
        assert_eq!(
            state["ven-1"]
                .command
                .as_ref()
                .map(|c| c.objective.as_str()),
            Some("opportunistic")
        );
        apply_message(&mut state, "ven-1", "command/objective", b"", now());
        assert!(
            state["ven-1"].command.is_none(),
            "an empty command is no command"
        );
    }

    /// Pending while the VEN reports another objective, or the commanded one set before the
    /// command; not pending once it reports the command applied, or with no command at all.
    #[test]
    fn a_command_is_pending_until_the_ven_reports_it_applied() {
        let mut state = BTreeMap::new();
        let command = br#"{"objective":"opportunistic","requested_at":"2026-10-16T12:00:00Z"}"#;
        apply_message(&mut state, "ven-1", "command/objective", command, now());
        assert!(
            state["ven-1"].command_pending(),
            "no objective reported yet"
        );
        let stale = objective_body("opportunistic", "2026-10-16T11:00:00Z");
        apply_message(&mut state, "ven-1", "objective", &stale, now());
        assert!(
            state["ven-1"].command_pending(),
            "set before the command was issued"
        );
        let other = objective_body("min_cost", "2026-10-16T12:00:05Z");
        apply_message(&mut state, "ven-1", "objective", &other, now());
        assert!(
            state["ven-1"].command_pending(),
            "another objective is in force"
        );
        let applied = objective_body("opportunistic", "2026-10-16T12:00:01Z");
        apply_message(&mut state, "ven-1", "objective", &applied, now());
        assert!(!state["ven-1"].command_pending());
        apply_message(&mut state, "ven-1", "command/objective", b"", now());
        assert!(!state["ven-1"].command_pending(), "released");
    }
}
