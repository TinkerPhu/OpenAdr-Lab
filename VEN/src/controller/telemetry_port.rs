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

use crate::controller::simulator_port::SimSnapshot;

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
    async fn publish_telemetry(&self, body: serde_json::Value);

    /// One controller decision, published as it is made.
    ///
    /// Separate from telemetry because the two are different kinds of fact and
    /// want different delivery: a snapshot is replaced by the next one seconds
    /// later and may be dropped, a decision happens once and a fleet watching
    /// for "did this VEN see the event" cannot recover a lost one.
    async fn publish_trace(&self, _body: serde_json::Value) {}

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
    async fn publish_telemetry(&self, _body: serde_json::Value) {}

    fn is_connected(&self) -> bool {
        false
    }

    fn publishes(&self) -> bool {
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
pub fn telemetry_body(ven_name: &str, snap: &SimSnapshot) -> serde_json::Value {
    let mut body = serde_json::to_value(snap).unwrap_or_else(|e| {
        // A snapshot that will not serialise is a bug in its own definition,
        // not a runtime condition -- say so rather than publish nothing.
        tracing::error!(error = %e, "sim snapshot is not serialisable");
        serde_json::json!({})
    });
    if let Some(obj) = body.as_object_mut() {
        obj.insert("venName".into(), serde_json::json!(ven_name));
    }
    body
}

/// What a trace message says.
///
/// The `ControllerEvent` exactly as `/trace/events` serves it, plus the name
/// of the VEN that decided it — same rule as `telemetry_body`: one vocabulary
/// across the fleet channel and the VEN's own routes (`dto`).
pub fn trace_body(
    ven_name: &str,
    event: &crate::controller::trace::ControllerEvent,
) -> serde_json::Value {
    let mut body = serde_json::to_value(event).unwrap_or_else(|e| {
        tracing::error!(error = %e, "controller event is not serialisable");
        serde_json::json!({})
    });
    if let Some(obj) = body.as_object_mut() {
        obj.insert("venName".into(), serde_json::json!(ven_name));
    }
    body
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
        let body = telemetry_body("ven-7", &snapshot(2500.0));
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
        let body = trace_body("ven-7", &event);
        assert_eq!(body["type"], "OpenAdrArrived");
        assert_eq!(body["event_id"], "ev-9f3");
        assert_eq!(body["modification_date_time"], "2026-09-22T10:00:00+00:00");
        assert_eq!(body["venName"], "ven-7");
    }

    /// Export is negative and must stay negative: a fleet sum built from
    /// clamped values is wrong by exactly the export, which is the same bug
    /// F-3 fixed on the report side.
    #[test]
    fn telemetry_body_keeps_the_sign_of_exported_power() {
        let body = telemetry_body("ven-1", &snapshot(-3100.0));
        assert_eq!(body["grid"]["net_power_w"], -3100.0);
    }
}
