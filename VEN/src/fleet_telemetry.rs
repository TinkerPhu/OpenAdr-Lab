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

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lab_core::fleet_channel::{leaf, ven_topic_root, BrokerConfig};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde::Serialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::controller::telemetry_port::{
    to_wire, PlanBody, TelemetryBody, TelemetryPort, TraceBody,
};
use crate::entities::objective_setting::ObjectiveSetting;

/// Where this VEN publishes, and as whom.
#[derive(Clone, Debug)]
pub struct FleetMqttConfig {
    pub broker: BrokerConfig,
    /// Topic root, `openadr-lab/fleet/<ven_name>`.
    pub topic_root: String,
    pub client_id: String,
    /// Seconds between telemetry samples (D-1).
    pub telemetry_every_s: i64,
}

impl FleetMqttConfig {
    /// Read the configuration, or `None` when this VEN does not publish
    /// (`FLEET_MQTT_HOST` unset: not part of a monitored fleet, a normal
    /// deployment). The broker settings are read as every lab client reads them
    /// (`lab_core::fleet_channel::BrokerConfig`).
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
        let broker = BrokerConfig::from_vars(&get)?;
        Some(Self {
            topic_root: ven_topic_root(&broker.root, ven_name),
            client_id: format!("ven-fleet-{ven_name}"),
            broker,
            // D-1: 5 s, and deliberately not tied to the sim tick. The tick is
            // 1 s in production, so publishing per tick is five times the
            // broker traffic and five times the store growth for a fleet view
            // nobody watches at that resolution.
            telemetry_every_s: get("FLEET_TELEMETRY_EVERY_S")
                .and_then(|v| v.parse().ok())
                .filter(|v| *v > 0)
                .unwrap_or(5),
        })
    }
}

/// Whether enough time has passed since the last sample.
///
/// `i64::MIN` means nothing has been published yet, so the first tick after
/// startup always counts. Split out from the atomic bookkeeping around it
/// because the arithmetic -- including the clock going backwards -- is the part
/// worth pinning with tests.
fn cadence_reached(last_ms: i64, now_ms: i64, every_ms: i64) -> bool {
    if last_ms == i64::MIN {
        return true;
    }
    // A clock that jumped backwards would otherwise stop telemetry until real
    // time caught up. Publishing is the safer answer: a sample slightly too
    // soon costs one row, a silent feed costs the view.
    now_ms < last_ms || now_ms - last_ms >= every_ms
}

/// The newest objective this VEN published, re-sent on every connect: a
/// publish dropped while the broker was away, or a broker restarted without its
/// retained store, would otherwise leave the fleet view on an old objective
/// until the next change. (The plan leaf has no such copy: `tasks::fleet_plan`
/// publishes each adopted plan retained at QoS 1.)
#[derive(Default)]
struct LatestLeaves {
    objective: Option<Arc<ObjectiveSetting>>,
}

/// Commands queued for `tasks::fleet_command` before the broker's are dropped.
/// A command is a person's click; eight unconsumed ones means the consumer is
/// gone, not that it is busy.
const COMMAND_QUEUE: usize = 8;

/// Publishes to the lab broker and receives this VEN's objective commands;
/// reconnects on its own.
pub struct FleetMqttPublisher {
    client: AsyncClient,
    topic_root: String,
    connected: Arc<AtomicBool>,
    telemetry_every_ms: i64,
    /// When the last sample went out, as epoch milliseconds. `i64::MIN` means
    /// "never", so the first tick after startup always publishes.
    last_sample_ms: AtomicI64,
    latest: Arc<Mutex<LatestLeaves>>,
}

impl FleetMqttPublisher {
    /// Connect and start the event loop. The receiver yields every payload
    /// that arrives on `command/objective`, for `tasks::fleet_command`.
    ///
    /// `rumqttc` reconnects internally, so the loop is spawned once and
    /// outlives any broker restart. `connected` tracks what the loop last saw,
    /// which is what `/health` and the VEN UI report.
    pub fn spawn(config: FleetMqttConfig) -> (Self, mpsc::Receiver<Vec<u8>>) {
        let broker = &config.broker;
        let status_topic = format!("{}/{}", config.topic_root, leaf::STATUS);
        let (client, mut eventloop) = AsyncClient::new(mqtt_options(&config, &status_topic), 32);
        let connected = Arc::new(AtomicBool::new(false));
        let latest = Arc::new(Mutex::new(LatestLeaves::default()));
        let (command_tx, command_rx) = mpsc::channel(COMMAND_QUEUE);

        let loop_connected = connected.clone();
        let loop_client = client.clone();
        let loop_latest = latest.clone();
        let topic_root = config.topic_root.clone();
        let command_topic = format!("{topic_root}/{}", leaf::COMMAND_OBJECTIVE);
        let (host, port) = (broker.host.clone(), broker.port);
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        if !loop_connected.swap(true, Ordering::Relaxed) {
                            info!(host, port, "fleet telemetry broker connected");
                        }
                        // On EVERY connect, not just the first. A broker
                        // restart drops us, which makes the broker publish our
                        // retained last will on our behalf -- correctly, since
                        // at that instant we really are gone. rumqttc then
                        // reconnects on its own and telemetry resumes, but the
                        // retained `offline` stays until something overwrites
                        // it. Announcing only at start-up left VENs marked
                        // offline in the fleet view while they were publishing
                        // every five seconds, which is precisely the
                        // "gone vs quiet" distinction this topic exists to
                        // make -- and the one signal an operator would trust
                        // to call a site dead.
                        announce_online(loop_client.clone(), status_topic.clone());
                        // A clean session forgets subscriptions, so this too
                        // runs on every connect; a retained command arrives
                        // right after it. `try_`: this task is the one that
                        // drains the request queue, so it must not wait on it.
                        if let Err(e) = loop_client.try_subscribe(&command_topic, QoS::AtLeastOnce)
                        {
                            warn!(topic = command_topic, error = %e, "could not subscribe to fleet commands");
                        }
                        republish_latest(&loop_client, &topic_root, &loop_latest);
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) if p.topic == command_topic => {
                        if command_tx.try_send(p.payload.to_vec()).is_err() {
                            warn!(
                                topic = command_topic,
                                "fleet command dropped: consumer not keeping up"
                            );
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

        // Nothing is announced here: the ConnAck handler above fires for the
        // first connection too, so doing it now would both duplicate that and
        // race it -- this point runs before the socket is up, and the old call
        // worked only because rumqttc queues publishes.
        let publisher = Self {
            client,
            topic_root: config.topic_root,
            connected,
            telemetry_every_ms: config.telemetry_every_s * 1000,
            last_sample_ms: AtomicI64::new(i64::MIN),
            latest,
        };
        (publisher, command_rx)
    }

    fn leaf_topic(&self, name: &str) -> String {
        format!("{}/{name}", self.topic_root)
    }

    fn remember(&self, update: impl FnOnce(&mut LatestLeaves)) {
        update(&mut self.latest.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

/// The client's connection settings: keep-alive, credentials, the packet limit the `plan` leaf
/// needs (rumqttc's 10 KiB default is far under it, `MAX_PACKET_BYTES`), and the last will.
fn mqtt_options(config: &FleetMqttConfig, status_topic: &str) -> MqttOptions {
    let broker = &config.broker;
    let mut opts = MqttOptions::new(config.client_id.clone(), broker.host.clone(), broker.port);
    opts.set_keep_alive(std::time::Duration::from_secs(30));
    opts.set_max_packet_size(
        lab_core::fleet_channel::MAX_PACKET_BYTES,
        lab_core::fleet_channel::MAX_PACKET_BYTES,
    );
    if let (Some(u), Some(p)) = (&broker.username, &broker.password) {
        opts.set_credentials(u.clone(), p.clone());
    }
    // Last will: if this VEN dies without saying goodbye, the broker tells
    // subscribers on its behalf. A fleet view must distinguish "quiet" from
    // "gone", and a VEN that has crashed cannot make that distinction
    // itself -- which is exactly why the broker holds the message.
    opts.set_last_will(rumqttc::LastWill::new(
        status_topic,
        serde_json::json!({"state": "offline"}).to_string(),
        QoS::AtLeastOnce,
        true,
    ));
    opts
}

/// Say we are here, retained, so a subscriber that connects later still learns
/// this VEN exists. The last will replaces it if we vanish.
///
/// Spawned rather than awaited: the caller is inside the event-loop poll, and
/// `publish` hands the packet to that same loop. Awaiting it there would wait
/// on the task doing the waiting.
fn announce_online(client: AsyncClient, topic: String) {
    tokio::spawn(async move {
        let body = serde_json::json!({"state": "online"}).to_string();
        if let Err(e) = client.publish(&topic, QoS::AtLeastOnce, true, body).await {
            warn!(topic, error = %e, "could not announce fleet status");
        }
    });
}

/// Re-send the newest objective after a (re)connect.
fn republish_latest(client: &AsyncClient, topic_root: &str, latest: &Mutex<LatestLeaves>) {
    let objective = latest
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .objective
        .clone();
    if let Some(o) = objective {
        publish_leaf_detached(
            client.clone(),
            format!("{topic_root}/{}", leaf::OBJECTIVE),
            o,
        );
    }
}

/// Serialise `body` and queue it, retained at QoS 1, from a task of its own:
/// the caller never waits on serialisation or on the broker (D10).
fn publish_leaf_detached<T: Serialize + Send + Sync + 'static>(
    client: AsyncClient,
    topic: String,
    body: Arc<T>,
) {
    tokio::spawn(async move {
        try_publish_retained(&client, &topic, body.as_ref());
    });
}

/// Queue one retained leaf without waiting; `false` (and a warning) when it was
/// dropped because rumqttc's request queue is full or closed. The newest body
/// is re-sent on the next connect, so a drop delays the leaf, it does not lose
/// it.
fn try_publish_retained<T: Serialize>(client: &AsyncClient, topic: &str, body: &T) -> bool {
    let payload = match to_wire(body) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(topic, error = %e, "fleet leaf is not serialisable");
            return false;
        }
    };
    match client.try_publish(topic, QoS::AtLeastOnce, true, payload) {
        Ok(()) => true,
        Err(e) => {
            warn!(topic, error = %e, "fleet leaf publish dropped");
            false
        }
    }
}

#[async_trait]
impl TelemetryPort for FleetMqttPublisher {
    async fn publish_telemetry(&self, body: TelemetryBody<'_>) {
        let topic = self.leaf_topic(leaf::TELEMETRY);
        let Ok(payload) = to_wire(&body).inspect_err(|e| {
            tracing::error!(error = %e, "sim snapshot is not serialisable");
        }) else {
            return;
        };
        // QoS 0 and retained: a dropped sample is replaced by the next one a
        // few seconds later, so redelivery would cost more than it is worth --
        // but a subscriber joining mid-stream should not have to wait for the
        // next tick to see anything.
        if let Err(e) = self
            .client
            .publish(&topic, QoS::AtMostOnce, true, payload)
            .await
        {
            // Debug, not warn: the publish queue filling while the broker is
            // away is the expected state of an offline VEN, and one line per
            // tick per VEN would bury everything else.
            tracing::debug!(topic, error = %e, "fleet telemetry publish dropped");
        }
    }

    async fn publish_trace(&self, body: TraceBody<'_>) {
        let topic = self.leaf_topic(leaf::TRACE);
        let Ok(payload) = to_wire(&body).inspect_err(|e| {
            tracing::error!(error = %e, "controller event is not serialisable");
        }) else {
            return;
        };
        // QoS 1 and *not* retained, unlike telemetry: a decision is not a
        // state to catch up on -- a subscriber joining later must not be told
        // about an event arrival from an hour ago as though it just happened --
        // but neither is it replaced a few seconds later, so losing one
        // silently breaks the reaction chain it belongs to (§6.3).
        if let Err(e) = self
            .client
            .publish(&topic, QoS::AtLeastOnce, false, payload)
            .await
        {
            tracing::debug!(topic, error = %e, "fleet trace publish dropped");
        }
    }

    async fn publish_plan(&self, body: PlanBody<'_>) {
        let topic = self.leaf_topic(leaf::PLAN);
        let Ok(payload) = to_wire(&body).inspect_err(|e| {
            tracing::error!(error = %e, "plan is not serialisable");
        }) else {
            return;
        };
        // QoS 1 and retained: unlike a decision this is state, so a subscriber joining later should
        // find the plan the site is working to; but losing one adoption silently would leave the
        // fleet view on a stale plan until the next replan, so it is delivered at least once.
        if let Err(e) = self
            .client
            .publish(&topic, QoS::AtLeastOnce, true, payload)
            .await
        {
            tracing::debug!(topic, error = %e, "fleet plan publish dropped");
        }
    }

    /// True at most once per configured interval.
    ///
    /// Claims the slot as it answers (compare-and-swap) rather than answering
    /// and leaving the caller to record it: a check that does not claim is a
    /// check two callers can both pass.
    fn sample_due(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        let now_ms = now.timestamp_millis();
        let last = self.last_sample_ms.load(Ordering::Relaxed);
        if !cadence_reached(last, now_ms, self.telemetry_every_ms) {
            return false;
        }
        self.last_sample_ms
            .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn publish_objective(&self, setting: &ObjectiveSetting) {
        let setting = Arc::new(setting.clone());
        self.remember(|l| l.objective = Some(setting.clone()));
        publish_leaf_detached(
            self.client.clone(),
            self.leaf_topic(leaf::OBJECTIVE),
            setting,
        );
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
        assert_eq!(c.broker.port, 1884, ":1884 is always the lab broker");
        assert_eq!(c.topic_root, "openadr-lab/fleet/ven-7");
        assert_eq!(c.client_id, "ven-fleet-ven-7");
    }

    /// The tick is the site's simulation step and telemetry is an observation
    /// cadence. D-1 fixes the second at 5 s; production ticks at 1 s.
    #[test]
    fn config_defaults_to_the_five_second_telemetry_cadence() {
        let c = FleetMqttConfig::from_vars("ven-1", vars(&[("FLEET_MQTT_HOST", "lab-mqtt")]))
            .expect("host is set");
        assert_eq!(c.telemetry_every_s, 5);
    }

    #[test]
    fn telemetry_cadence_is_configurable_and_rejects_nonsense() {
        let with = |v: &str| {
            FleetMqttConfig::from_vars(
                "ven-1",
                vars(&[
                    ("FLEET_MQTT_HOST", "lab-mqtt"),
                    ("FLEET_TELEMETRY_EVERY_S", v),
                ]),
            )
            .unwrap()
            .telemetry_every_s
        };
        assert_eq!(with("30"), 30);
        // A zero or negative interval would mean "publish every tick", which
        // is the behaviour this exists to stop -- so it falls back.
        assert_eq!(with("0"), 5);
        assert_eq!(with("-1"), 5);
        assert_eq!(with("soon"), 5);
    }

    #[test]
    fn cadence_publishes_the_first_sample_immediately() {
        assert!(cadence_reached(i64::MIN, 1_000, 5_000));
    }

    #[test]
    fn cadence_holds_a_sample_back_until_the_interval_has_passed() {
        assert!(!cadence_reached(1_000, 2_000, 5_000));
        assert!(!cadence_reached(1_000, 5_999, 5_000));
        assert!(cadence_reached(1_000, 6_000, 5_000));
    }

    /// A clock that jumped backwards must not silence the feed until real time
    /// catches up: one extra row is cheaper than a dark fleet view.
    #[test]
    fn cadence_publishes_when_the_clock_goes_backwards() {
        assert!(cadence_reached(10_000, 1_000, 5_000));
    }

    fn publisher(capacity: usize) -> (FleetMqttPublisher, rumqttc::EventLoop) {
        let (client, eventloop) =
            AsyncClient::new(MqttOptions::new("test", "localhost", 1884), capacity);
        let publisher = FleetMqttPublisher {
            client,
            topic_root: "openadr-lab/fleet/ven-7".into(),
            connected: Arc::new(AtomicBool::new(false)),
            telemetry_every_ms: 5_000,
            last_sample_ms: AtomicI64::new(i64::MIN),
            latest: Arc::new(Mutex::new(LatestLeaves::default())),
        };
        (publisher, eventloop)
    }

    /// A plan leaf is tens of kB; rumqttc's 10 KiB default would fail it and drop the
    /// connection (`MAX_PACKET_BYTES`).
    #[test]
    fn the_client_accepts_and_sends_packets_as_large_as_a_plan() {
        let config =
            FleetMqttConfig::from_vars("ven-7", vars(&[("FLEET_MQTT_HOST", "lab-mqtt")])).unwrap();
        let opts = mqtt_options(&config, "openadr-lab/fleet/ven-7/status");
        assert_eq!(
            opts.max_packet_size(),
            lab_core::fleet_channel::MAX_PACKET_BYTES
        );
    }

    #[test]
    fn leaves_live_under_this_vens_topic_root() {
        let (p, _loop) = publisher(1);
        assert_eq!(
            p.leaf_topic(leaf::OBJECTIVE),
            "openadr-lab/fleet/ven-7/objective"
        );
        assert_eq!(p.leaf_topic(leaf::PLAN), "openadr-lab/fleet/ven-7/plan");
    }

    /// The `objective` leaf carries exactly what `GET /plan/objective` serves:
    /// both serialise the same `ObjectiveSetting`.
    #[test]
    fn objective_leaf_body_is_the_objective_setting() {
        use crate::entities::planner_params::PlannerObjective;
        let setting = ObjectiveSetting::from_profile(PlannerObjective::MinCost);
        let body: serde_json::Value = serde_json::from_str(&to_wire(&setting).unwrap()).unwrap();
        assert_eq!(body, serde_json::to_value(&setting).unwrap());
        assert_eq!(body["objective"], "min_cost");
        assert_eq!(body["source"], "profile");
    }

    /// With the request queue full (broker away, event loop not draining) the
    /// publish returns at once and reports the drop, instead of waiting.
    #[test]
    fn a_full_request_queue_drops_the_leaf_without_waiting() {
        let (p, _loop) = publisher(1);
        assert!(try_publish_retained(&p.client, "t/objective", &1));
        let started = std::time::Instant::now();
        assert!(!try_publish_retained(&p.client, "t/objective", &2));
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
    }

    /// Each publish is remembered, so the next connect re-sends the newest one.
    #[tokio::test]
    async fn the_newest_objective_is_kept_for_the_next_connect() {
        use crate::entities::planner_params::PlannerObjective;
        let (p, _loop) = publisher(1);
        let mut setting = ObjectiveSetting::from_profile(PlannerObjective::MinCost);
        p.publish_objective(&setting);
        setting.set(
            PlannerObjective::Opportunistic,
            crate::entities::objective_setting::ObjectiveSource::Fleet,
            chrono::Utc::now(),
        );
        p.publish_objective(&setting);
        let latest = p.latest.lock().unwrap();
        assert_eq!(
            latest.objective.as_ref().map(|o| o.objective),
            Some(PlannerObjective::Opportunistic)
        );
    }
}
