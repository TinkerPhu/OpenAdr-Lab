// VtnPort trait — the boundary between application/service logic and the VTN HTTP client.
//
// Typed structs model only the fields currently consumed by the VEN codebase.
// Field names preserve the OpenADR 3 convention verbatim (camelCase) per project policy.
// OpenADR 3.1 introduces breaking field/type name changes; keeping the struct surface minimal
// reduces future migration cost.
#![allow(non_snake_case)]

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Port trait ─────────────────────────────────────────────────────────────────

use crate::controller::wire_reject::FetchOutcome;

#[async_trait]
pub trait VtnPort: Send + Sync {
    /// Each fetch returns the objects that parsed *and* a record of any the VTN
    /// sent that we refused, so a caller can surface them. One malformed object
    /// costs only itself -- see `controller::wire_reject`.
    async fn fetch_programs(&self) -> Result<FetchOutcome<OadrProgram>>;
    async fn fetch_events(&self) -> Result<FetchOutcome<OadrEvent>>;
    /// Returns full-fidelity typed reports: `id`/`reportName` accessed by field,
    /// every other VTN field preserved verbatim in `extra` (serde flatten) so
    /// state storage and the GET /reports route stay wire-shape pass-through.
    async fn fetch_reports(&self) -> Result<FetchOutcome<OadrReport>>;
    /// Submit or upsert a typed report body. Returns Ok(()) on success; errors are
    /// propagated from the VTN HTTP response.
    async fn upsert_report(&self, body: OadrReportBody) -> Result<()>;
    /// GB-49: `Some(reason)` when the last token this client obtained lacked
    /// VEN scopes. `None` means correctly provisioned, or no token fetched yet.
    async fn scope_warning(&self) -> Option<String> {
        None
    }
}

// ── OadrProgram ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrProgram {
    pub id: String,
    pub programName: String,
}

// ── Event types: the wire crate's, not ours ───────────────────────────────────
//
// These were hand-rolled here until 3.1b. The problem with describing a
// protocol to yourself is that the description silently drops whatever it does
// not mention -- which is how `event.duration`, `targets`, `payloadDescriptors`
// and `randomizeStart` all came to be missing at once. A type from the wire
// crate cannot forget a field, because the field is in the type.
//
// Aliased to the old names so call sites keep reading in this project's
// vocabulary (`dto` rule: upstream field names, one word per concept).
// `OadrIntervalPeriod` below stays ours: it is the *report* side, which we
// construct rather than parse.
pub use openleadr_wire::event::{
    Event as OadrEvent, EventInterval as OadrInterval, EventType, EventValuesMap as OadrPayload,
};
pub use openleadr_wire::values_map::Value as PayloadValue;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OadrIntervalPeriod {
    /// ISO 8601 datetime string, e.g. "2026-01-01T00:00:00Z"
    #[serde(default)]
    pub start: Option<String>,
    /// ISO 8601 duration string, e.g. "PT1H". Per the spec this is the duration
    /// of *one* interval, or the default duration of each interval when it
    /// appears on the event -- not an event span. `event.duration` is the
    /// control that repeats or truncates a sequence.
    #[serde(default)]
    pub duration: Option<String>,
    /// How far into the window a VEN may randomise its start, so a VTN can stop
    /// a whole fleet responding on the same instant. Carried but not yet
    /// honoured -- see `docs/reference/TECHNICAL_DEBTS.md`.
    #[serde(default)]
    pub randomizeStart: Option<String>,
}

impl OadrIntervalPeriod {
    /// The window one report interval covers.
    ///
    /// The one way to build an interval period on the report-out side: the
    /// RFC3339 formatting and the (never randomised) start were repeated at
    /// five call sites, which is five chances for them to drift apart.
    pub fn window(start: DateTime<Utc>, duration_iso: impl Into<String>) -> Self {
        Self {
            start: Some(start.to_rfc3339()),
            duration: Some(duration_iso.into()),
            // A report describes measurements already taken; there is nothing
            // to randomise. Only an *event* asks a VEN to stagger its start.
            randomizeStart: None,
        }
    }
}

/// What a payload type is called on the wire.
///
/// `EventType` gets its spelling from serde's `SCREAMING_SNAKE_CASE` rename,
/// so serde is asked for it rather than a second table being maintained beside
/// it -- the kind of duplicate that drifts silently. The `Private(_)` variant
/// carries its own string and round-trips unchanged.
pub trait EventTypeName {
    fn wire_name(&self) -> String;
}

impl EventTypeName for openleadr_wire::report::ReportType {
    fn wire_name(&self) -> String {
        wire_name_of(self)
    }
}

impl EventTypeName for openleadr_wire::report::ReadingType {
    fn wire_name(&self) -> String {
        wire_name_of(self)
    }
}

/// Ask serde for a wire spelling, once, for every enum that has one.
fn wire_name_of<T: serde::Serialize + std::fmt::Debug>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        other => {
            debug_assert!(false, "{v:?} did not serialise to a string: {other:?}");
            String::new()
        }
    }
}

impl EventTypeName for EventType {
    fn wire_name(&self) -> String {
        wire_name_of(self)
    }
}

/// `OadrPayload::numeric` used to live here as an inherent method. The type is
/// now the wire crate's `EventValuesMap`, so it becomes an extension trait --
/// same single reader, same reason for existing.
pub trait PayloadValues {
    /// This payload's first value as a number, when it has one.
    ///
    /// The one place a payload value becomes an `f64`. It matters more now
    /// than it did as a DTO method: the wire value is an enum with *separate*
    /// `Number` and `Integer` variants, and `EventType::Simple`'s declared kind
    /// is `Integer`. A reader matching only `Number` drops every SIMPLE window
    /// silently -- exactly the class of failure this migration keeps finding.
    fn numeric(&self) -> Option<f64>;

    /// This payload's first value as text, when it is text.
    ///
    /// The alert payload types carry a human-readable message here; every
    /// other type carries a number, and asking for text from one of those
    /// returns `None` rather than a stringified number.
    fn text(&self) -> Option<&str>;
}

impl PayloadValues for OadrPayload {
    fn numeric(&self) -> Option<f64> {
        match self.values.first()? {
            PayloadValue::Number(n) => Some(*n),
            // `Integer` is not an afterthought: `EventType::Simple`'s declared
            // value kind *is* Integer, so a reader that only matched `Number`
            // would drop every load-shed level and say nothing.
            PayloadValue::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match self.values.first()? {
            PayloadValue::String(s) => Some(s),
            _ => None,
        }
    }
}

/// Build events for tests from the JSON a fixture actually cares about.
///
/// The wire `Event` requires `id`, `createdDateTime` and `modificationDateTime`
/// -- fields no test is about, and which the lenient DTO did not have. Rather
/// than spell them out in ~30 fixtures (and in every fixture written after
/// this), they are merged in where absent. A fixture that *does* care about one
/// states it and keeps it: this fills gaps, it does not overwrite.
///
/// `createdDateTime` defaults to `MIN_UTC` deliberately. That is the value the
/// old DTO path fell back to when the field was absent, so priority
/// tie-breaking in `rate_schedule` behaves as it always did for fixtures that
/// do not set it.
#[cfg(test)]
pub fn events_from_json(value: serde_json::Value) -> Vec<OadrEvent> {
    use serde_json::{json, Value};

    let epoch = "0001-01-01T00:00:00Z";
    let mut arr = match value {
        Value::Array(a) => a,
        one => vec![one],
    };
    for (i, ev) in arr.iter_mut().enumerate() {
        let Some(obj) = ev.as_object_mut() else {
            continue;
        };
        obj.entry("id").or_insert_with(|| json!(format!("evt-{i}")));
        obj.entry("createdDateTime").or_insert_with(|| json!(epoch));
        obj.entry("modificationDateTime")
            .or_insert_with(|| json!(epoch));
        obj.entry("programID").or_insert_with(|| json!("prog-test"));
        // `interval.id` is required by the schema and is never what a timing
        // or payload fixture is about; number them in declaration order.
        if let Some(Value::Array(intervals)) = obj.get_mut("intervals") {
            for (n, iv) in intervals.iter_mut().enumerate() {
                if let Some(io_) = iv.as_object_mut() {
                    io_.entry("id").or_insert_with(|| json!(n));
                }
            }
        }
    }
    serde_json::from_value(Value::Array(arr)).expect("test fixture is not a valid 3.1 event")
}

// ── OadrReport ────────────────────────────────────────────────────────────────

/// Typed report with full wire fidelity: `id` and `reportName` are the fields
/// the VEN accesses (409 upsert resolution); everything else the VTN sent is
/// preserved verbatim in `extra` and round-trips on serialization, keeping the
/// GET /reports pass-through intact without `serde_json::Value` on the port.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrReport {
    pub id: String,
    #[serde(default)]
    pub reportName: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── OadrReportBody and nested types ──────────────────────────────────────────

/// Top-level envelope for a report submission to the VTN.
/// `reportName` is optional per the OpenADR 3 spec (VTN field `report_name: Option<String>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eventID: Option<String>,
    pub clientName: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reportName: Option<String>,
    pub resources: Vec<OadrReportResource>,
    /// What this report's values mean. Mandatory here even though the spec
    /// marks it optional: no value leaves this lab whose quantity and unit are
    /// not declared on the wire (`wire-contracts`). Derived, never hand-set --
    /// see `controller::report_payload::descriptors_for`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payloadDescriptors: Vec<crate::controller::report_payload::OadrReportPayloadDescriptor>,
}

/// A named resource (site meter, individual asset) within a report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportResource {
    pub resourceName: String,
    pub intervals: Vec<OadrReportInterval>,
}

/// A single measurement interval with an optional time window and one or more payload values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportInterval {
    pub id: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intervalPeriod: Option<OadrIntervalPeriod>,
    pub payloads: Vec<OadrReportPayload>,
}

/// A single typed value (or set of values) within an interval.
/// `values` is intentionally `Vec<serde_json::Value>` — OpenADR 3 defines it as a
/// heterogeneous array (numbers for power, strings for state/SoC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportPayload {
    pub r#type: String,
    pub values: Vec<serde_json::Value>,
}

// ── Contract tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oadr_event_deserializes_from_fixture() {
        let json = r#"{
            "id": "evt-001",
            "programID": "prog-001",
            "createdDateTime": "2026-01-01T00:00:00Z",
            "modificationDateTime": "2026-01-01T00:00:00Z",
            "eventName": "test-event",
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": { "start": "2026-01-01T00:00:00Z", "duration": "PT1H" },
                    "payloads": [{ "type": "PRICE", "values": [0.25] }]
                }
            ],
            "reportDescriptors": [
                { "payloadType": "USAGE", "readingType": "DIRECT_READ", "frequency": 900 }
            ]
        }"#;
        let event: OadrEvent = serde_json::from_str(json).expect("deserialization failed");
        assert_eq!(event.id.as_str(), "evt-001");
        assert_eq!(event.content.program_id.as_str(), "prog-001");
        assert_eq!(event.content.event_name.as_deref(), Some("test-event"));
        let intervals = event.content.intervals.as_ref().unwrap();
        assert_eq!(intervals.len(), 1);
        let period = intervals[0].interval_period.as_ref().unwrap();
        assert_eq!(period.start.to_rfc3339(), "2026-01-01T00:00:00+00:00");
        assert_eq!(
            period.duration.as_ref().unwrap().to_string(),
            "P0Y0M0DT1H0M0S"
        );
        let payload = &intervals[0].payloads[0];
        assert_eq!(payload.value_type.wire_name(), "PRICE");
        let desc = &event.content.report_descriptors.as_ref().unwrap()[0];
        assert_eq!(desc.payload_type.wire_name(), "USAGE");
    }

    /// 3.1 makes `createdDateTime` and `modificationDateTime` required, so an
    /// event that omits them is refused rather than defaulted. That is the
    /// point of the strict types: the VEN stops inventing values a peer did
    /// not send. `wire_reject` makes a refusal cost only that object.
    #[test]
    fn an_event_missing_required_timestamps_is_refused() {
        let json = r#"{ "id": "evt-002", "programID": "prog-001" }"#;
        let err = serde_json::from_str::<OadrEvent>(json)
            .expect_err("3.1 requires createdDateTime and modificationDateTime");
        assert!(
            err.to_string().contains("createdDateTime")
                || err.to_string().contains("created_date_time"),
            "got {err}"
        );
    }

    #[test]
    fn optional_fields_stay_absent_when_the_event_omits_them() {
        let event = events_from_json(serde_json::json!([{
            "id": "evt-002", "programID": "prog-001"
        }]))
        .remove(0);
        assert!(event.content.event_name.is_none());
        assert!(event.content.intervals.is_none());
        assert!(event.content.report_descriptors.is_none());
    }

    #[test]
    fn test_oadr_event_unknown_fields_do_not_panic() {
        let json = r#"{
            "id": "evt-003",
            "programID": "prog-001",
            "unknownFieldFromFutureVersion": "ignored",
            "anotherUnknown": 42
        }"#;
        let event = events_from_json(serde_json::json!({
            "id": "evt-003",
            "programID": "prog-001",
            "unknownFieldFromFutureVersion": "ignored",
            "anotherUnknown": 42
        }))
        .remove(0);
        assert_eq!(event.id.as_str(), "evt-003");
        let _ = json;
    }

    #[test]
    fn test_oadr_program_deserializes_from_fixture() {
        let json = r#"{ "id": "prog-001", "programName": "DR-Program-A" }"#;
        let prog: OadrProgram = serde_json::from_str(json).expect("deserialization failed");
        assert_eq!(prog.id, "prog-001");
        assert_eq!(prog.programName, "DR-Program-A");
    }

    #[test]
    fn test_oadr_program_unknown_fields_ignored() {
        let json = r#"{ "id": "prog-002", "programName": "X", "futureField": true }"#;
        let prog: OadrProgram = serde_json::from_str(json).expect("unknown fields must be ignored");
        assert_eq!(prog.programName, "X");
    }

    #[test]
    fn test_oadr_report_deserializes_from_fixture() {
        let json = r#"{ "id": "rep-001", "reportName": "ven-status" }"#;
        let report: OadrReport = serde_json::from_str(json).expect("deserialization failed");
        assert_eq!(report.id, "rep-001");
        assert_eq!(report.reportName.as_deref(), Some("ven-status"));
    }

    #[test]
    fn oadr_report_preserves_unknown_fields_and_null_name_roundtrip() {
        // R-10: the pass-through contract — every VTN field survives the typed
        // struct, including ones the VEN never accesses and a null reportName.
        let json = serde_json::json!({
            "id": "rep-002",
            "reportName": null,
            "programID": "prog-1",
            "clientName": "ven-1",
            "payloadDescriptors": [{"payloadType": "USAGE", "units": "KW"}],
            "createdDateTime": "2026-07-16T12:00:00Z"
        });
        let report: OadrReport = serde_json::from_value(json.clone()).expect("must deserialize");
        assert!(report.reportName.is_none());
        assert_eq!(
            serde_json::to_value(&report).expect("must serialize"),
            json,
            "wire shape must round-trip unchanged"
        );
    }

    #[test]
    fn test_oadr_report_body_round_trips_with_event_id() {
        let body = OadrReportBody {
            payloadDescriptors: Vec::new(),
            eventID: Some("evt-abc".to_string()),
            clientName: "ven-1".to_string(),
            reportName: Some("auto-ven-1-evt-abc".to_string()),
            resources: vec![OadrReportResource {
                resourceName: "ven-1-meter".to_string(),
                intervals: vec![OadrReportInterval {
                    id: 0,
                    intervalPeriod: None,
                    payloads: vec![
                        OadrReportPayload {
                            r#type: "USAGE".to_string(),
                            values: vec![serde_json::json!(4500.0)],
                        },
                        OadrReportPayload {
                            r#type: "OPERATING_STATE".to_string(),
                            values: vec![serde_json::json!("ACTIVE")],
                        },
                    ],
                }],
            }],
        };

        let value = serde_json::to_value(&body).expect("serialize failed");
        assert_eq!(value["eventID"], "evt-abc");
        assert_eq!(value["clientName"], "ven-1");
        assert_eq!(value["reportName"], "auto-ven-1-evt-abc");
        // Verify round-trip preserves reportName as Some
        assert_eq!(value["resources"][0]["resourceName"], "ven-1-meter");
        assert_eq!(value["resources"][0]["intervals"][0]["id"], 0);
        assert!(value["resources"][0]["intervals"][0]
            .get("intervalPeriod")
            .is_none());
        assert_eq!(
            value["resources"][0]["intervals"][0]["payloads"][0]["type"],
            "USAGE"
        );
        assert!(
            (value["resources"][0]["intervals"][0]["payloads"][0]["values"][0]
                .as_f64()
                .unwrap()
                - 4500.0)
                .abs()
                < 1e-9
        );

        let restored: OadrReportBody = serde_json::from_value(value).expect("deserialize failed");
        // 3.1 removed `programID` from a report: `eventID` is its only object
        // link. Assert it is gone from the wire, not merely unread.
        assert!(
            !serde_json::to_string(&restored)
                .unwrap()
                .contains("programID"),
            "3.1 reports must not carry programID"
        );
        assert_eq!(restored.eventID.as_deref(), Some("evt-abc"));
        assert_eq!(restored.reportName.as_deref(), Some("auto-ven-1-evt-abc"));
        assert_eq!(
            restored.resources[0].intervals[0].payloads[0].r#type,
            "USAGE"
        );
    }

    #[test]
    fn test_oadr_report_body_absent_event_id_not_serialized() {
        let body = OadrReportBody {
            payloadDescriptors: Vec::new(),
            eventID: None,
            clientName: "ven-1".to_string(),
            reportName: Some("status-ven-1".to_string()),
            resources: vec![],
        };
        let value = serde_json::to_value(&body).expect("serialize failed");
        // eventID must be absent (not null) when None
        assert!(
            value.get("eventID").is_none(),
            "eventID must be absent when None"
        );
    }

    #[test]
    fn test_oadr_report_body_absent_report_name_not_serialized() {
        let body = OadrReportBody {
            payloadDescriptors: Vec::new(),
            eventID: Some("evt-1".to_string()),
            clientName: "ven-1".to_string(),
            reportName: None,
            resources: vec![],
        };
        let value = serde_json::to_value(&body).expect("serialize failed");
        assert!(
            value.get("reportName").is_none(),
            "reportName must be absent when None"
        );
    }

    #[test]
    fn test_oadr_report_interval_period_serialized_when_some() {
        let interval = OadrReportInterval {
            id: 0,
            intervalPeriod: Some(OadrIntervalPeriod {
                start: Some("2026-01-01T10:00:00Z".to_string()),
                duration: Some("PT15M".to_string()),
                ..Default::default()
            }),
            payloads: vec![],
        };
        let value = serde_json::to_value(&interval).expect("serialize failed");
        assert_eq!(value["intervalPeriod"]["start"], "2026-01-01T10:00:00Z");
        assert_eq!(value["intervalPeriod"]["duration"], "PT15M");
    }
}
