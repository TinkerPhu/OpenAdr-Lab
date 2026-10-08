//! Publishing what this VEN is doing, for anyone watching the fleet.
//!
//! The VTN learns a VEN's state from reports, which arrive on the report
//! cadence and only describe intervals that have closed. That is the right
//! shape for a record and the wrong shape for a live view: a fleet operator
//! watching twenty sites react to a capacity limit cannot wait a minute to see
//! whether they did.
//!
//! So this is a second channel, and its job is different. Reports are the
//! record — durable, spec-shaped, carried by OpenADR. Telemetry is the live
//! feed — lossy by design (QoS 0, retained so a late subscriber sees the last
//! value), carrying the VEN's own vocabulary rather than the protocol's.
//!
//! Deliberately an outbound port with no reply. A VEN publishes what it is
//! doing; nothing it publishes here can change what it does. Anything that
//! *instructs* a VEN arrives as an OpenADR event through `VtnPort`, where the
//! spec's rules about targeting and priority apply — a side channel that could
//! also command would be a way around them.

use async_trait::async_trait;
use serde::Serialize;

use crate::controller::simulator_port::SimSnapshot;
use crate::controller::trace::ControllerEvent;

/// What a VEN publishes about itself, and where.
///
/// Every method is fire-and-forget: a publish that fails is logged and
/// dropped, never retried and never propagated. A VEN that cannot reach the
/// broker must keep planning, dispatching and reporting exactly as before —
/// the fleet view going dark is an observability problem, not an operational
/// one, and turning it into one would be the tail wagging the dog.
#[async_trait]
pub trait TelemetryPort: Send + Sync {
    /// The site's current state: a snapshot, published on the tick cadence.
    async fn publish_telemetry(&self, body: TelemetryBody<'_>);

    /// Whether a telemetry sample is due now.
    ///
    /// The cadence belongs to the publisher, not to the tick loop: the tick is
    /// the site's simulation step (1 s in production) and telemetry is an
    /// observation cadence (D-1: 5 s). Coupling them makes every sim-rate
    /// change a change in broker traffic and in how fast the telemetry store
    /// grows — which is how this was first written, and the reason 17 VENs put
    /// 862 rows a minute into Postgres the first time it ran live.
    ///
    /// Asked before the snapshot is serialised, so a sample that is not due
    /// costs nothing.
    fn sample_due(&self, _now: chrono::DateTime<chrono::Utc>) -> bool {
        true
    }

    /// One controller decision, published as it is made.
    ///
    /// Separate from telemetry because the two are different kinds of fact and
    /// want different delivery: a snapshot is replaced by the next one seconds
    /// later and may be dropped, a decision happens once and a fleet watching
    /// for "did this VEN see the event" cannot recover a lost one.
    async fn publish_trace(&self, _body: TraceBody<'_>) {}

    /// Whether this VEN's publisher is connected, for `/health` and the VEN
    /// UI's diagnostics (`ui-transparency`: a feed with no visible surface is
    /// an incomplete implementation).
    fn is_connected(&self) -> bool;

    /// Whether this VEN publishes telemetry at all.
    ///
    /// Distinct from `is_connected`, and the distinction is the point: "not
    /// configured to publish" and "configured but the broker is unreachable"
    /// look identical from a boolean, and only the second is worth showing as
    /// a fault.
    fn publishes(&self) -> bool {
        true
    }
}

/// The port when no broker is configured.
///
/// `FLEET_MQTT_HOST` unset means "this VEN does not publish telemetry", which
/// is the normal state for a VEN outside a monitored fleet — not an error and
/// not a degraded mode. Same two-gate pattern as the measurement adapters:
/// absent configuration yields a no-op rather than a failure.
pub struct NoTelemetry;

#[async_trait]
impl TelemetryPort for NoTelemetry {
    async fn publish_telemetry(&self, _body: TelemetryBody<'_>) {}

    fn is_connected(&self) -> bool {
        false
    }

    fn publishes(&self) -> bool {
        false
    }

    fn sample_due(&self, _now: chrono::DateTime<chrono::Utc>) -> bool {
        false
    }
}

/// What a telemetry message says.
///
/// The simulator snapshot as it already is, plus the name of the VEN that sent
/// it. Deliberately not a new shape: `/sim` serves this same snapshot to the
/// VEN UI, so a fleet consumer and a single-VEN consumer read the same
/// vocabulary (`dto` -- one word per concept, across every layer).
///
/// `grid.net_power_w` is the site meter reading and the value a fleet view
/// sums. It is published, not recomputed: the report path derives its own
/// series from the same snapshot's assets, and a second derivation here is
/// exactly the divergence F-9 records.
#[derive(Debug, Serialize)]
pub struct TelemetryBody<'a> {
    #[serde(flatten)]
    pub snapshot: &'a SimSnapshot,
    #[serde(rename = "venName")]
    pub ven_name: &'a str,
}

pub fn telemetry_body<'a>(ven_name: &'a str, snap: &'a SimSnapshot) -> TelemetryBody<'a> {
    TelemetryBody {
        snapshot: snap,
        ven_name,
    }
}

/// What a trace message says.
///
/// The `ControllerEvent` exactly as `/trace/events` serves it, plus the name
/// of the VEN that decided it — same rule as `telemetry_body`: one vocabulary
/// across the fleet channel and the VEN's own routes (`dto`).
#[derive(Debug, Serialize)]
pub struct TraceBody<'a> {
    #[serde(flatten)]
    pub event: &'a ControllerEvent,
    #[serde(rename = "venName")]
    pub ven_name: &'a str,
}

pub fn trace_body<'a>(ven_name: &'a str, event: &'a ControllerEvent) -> TraceBody<'a> {
    TraceBody { event, ven_name }
}

/// The JSON a body goes onto the wire as. A body that will not serialise is a bug in its own
/// definition, not a runtime condition: the caller logs it and publishes nothing.
pub fn to_wire<T: Serialize>(body: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::simulator_port::GridSnapshot;

    fn snapshot(net_power_w: f64) -> SimSnapshot {
        SimSnapshot {
            ts: chrono::Utc::now(),
            grid: GridSnapshot {
                net_power_w,
                voltage_v: 230.0,
                import_kwh: 1.0,
                export_kwh: 0.5,
                import_limit_kw: f64::MAX,
                export_limit_kw: -f64::MAX,
            },
            assets: Default::default(),
        }
    }

    #[test]
    fn telemetry_body_carries_the_sender_and_the_snapshot() {
        let body = serde_json::to_value(telemetry_body("ven-7", &snapshot(2500.0))).unwrap();
        assert_eq!(body["venName"], "ven-7");
        assert_eq!(body["grid"]["net_power_w"], 2500.0);
    }

    /// The `type` tag and the event id are what a reaction chain is assembled
    /// from, so both have to survive the trip onto the wire intact.
    #[test]
    fn trace_body_carries_the_kind_of_decision_and_the_event_it_was_about() {
        let event = crate::controller::trace::ControllerEvent::OpenAdrArrived {
            ts: chrono::Utc::now(),
            event_id: "ev-9f3".into(),
            modification_date_time: Some("2026-09-22T10:00:00+00:00".into()),
            event_name: "Peak DR".into(),
            signal_type: "PRICE".into(),
            value: 0.3,
            interval: 4,
        };
        let body = serde_json::to_value(trace_body("ven-7", &event)).unwrap();
        assert_eq!(body["type"], "OpenAdrArrived");
        assert_eq!(body["event_id"], "ev-9f3");
        assert_eq!(body["modification_date_time"], "2026-09-22T10:00:00+00:00");
        assert_eq!(body["venName"], "ven-7");
    }

    /// R-114: the typed bodies go onto the wire exactly as the `serde_json::Value` they replace
    /// did: the snapshot (or the event, with its `type` tag) plus `venName`.
    #[test]
    fn the_typed_bodies_are_the_snapshot_or_event_plus_the_ven_name_on_the_wire() {
        let with_name = |mut v: serde_json::Value| {
            v.as_object_mut()
                .unwrap()
                .insert("venName".into(), "ven-3".into());
            v
        };
        let wire = |s: String| serde_json::from_str::<serde_json::Value>(&s).unwrap();

        let snap = snapshot(1200.0);
        let expected = with_name(serde_json::to_value(&snap).unwrap());
        assert_eq!(
            wire(to_wire(&telemetry_body("ven-3", &snap)).unwrap()),
            expected
        );

        let event = ControllerEvent::RequestTransition {
            ts: chrono::Utc::now(),
            request_id: uuid::Uuid::new_v4(),
            asset_id: "ev".into(),
            from_status: "None".into(),
            to_status: "Active".into(),
        };
        let expected = with_name(serde_json::to_value(&event).unwrap());
        assert_eq!(
            wire(to_wire(&trace_body("ven-3", &event)).unwrap()),
            expected
        );
    }

    /// Export is negative and must stay negative: a fleet sum built from
    /// clamped values is wrong by exactly the export, which is the same bug
    /// F-3 fixed on the report side.
    #[test]
    fn telemetry_body_keeps_the_sign_of_exported_power() {
        let body = serde_json::to_value(telemetry_body("ven-1", &snapshot(-3100.0))).unwrap();
        assert_eq!(body["grid"]["net_power_w"], -3100.0);
    }
}
