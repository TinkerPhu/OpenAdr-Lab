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
}

// ── OadrProgram ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrProgram {
    pub id: String,
    pub programName: String,
}

// ── OadrEvent and nested types ────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrEvent {
    pub id: String,
    pub programID: String,
    #[serde(default)]
    pub eventName: Option<String>,
    /// OpenADR 3 priority — lower numbers are higher priority (0 = highest). Optional.
    #[serde(default)]
    pub priority: Option<i64>,
    /// OpenADR 3 event creation timestamp (ISO 8601), used to break priority ties. Optional.
    #[serde(default)]
    pub createdDateTime: Option<String>,
    /// OpenADR 3.1 event-level duration. When it exceeds the sum of the interval
    /// durations the interval sequence repeats to fill it, and `"P9999Y"` means
    /// loop indefinitely (User Guide, "Looping intervals"). 3.0 expressed the
    /// same thing through `intervalPeriod.duration`, which still works.
    #[serde(default)]
    pub duration: Option<String>,
    /// Event-level interval period — used for looping price events (e.g. duration = "P9999Y").
    #[serde(default)]
    pub intervalPeriod: Option<OadrIntervalPeriod>,
    #[serde(default)]
    pub intervals: Vec<OadrInterval>,
    #[serde(default)]
    pub reportDescriptors: Option<Vec<OadrReportDescriptor>>,
    /// 3.1 targets: a flat list of strings, replacing 3.0's typed objects. The
    /// VTN filters on these and redacts them per caller, so what arrives here is
    /// already this VEN's view; it is carried so the UI can show who an event
    /// addresses. Empty (or absent) means every VEN.
    #[serde(default)]
    pub targets: Option<Vec<String>>,
    /// What the event's payload values mean -- quantity, unit, currency. Under
    /// `wire-contracts` a value whose unit is only in the reader's head is a
    /// bug, so this is read rather than assumed.
    #[serde(default)]
    pub payloadDescriptors: Option<Vec<OadrEventPayloadDescriptor>>,
}

/// Declares the meaning of one payload type carried by an event.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OadrEventPayloadDescriptor {
    pub payloadType: String,
    /// Unit of measure, e.g. `"KWH"`, `"KW"`, `"PERCENT"`. Absent means the
    /// payload type's own definition applies.
    #[serde(default)]
    pub units: Option<String>,
    /// Currency for price payloads, e.g. `"EUR"`.
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OadrInterval {
    /// Required by the spec. Carried so a report can name the interval it is
    /// reporting on -- dropping it made that impossible to express.
    pub id: i64,
    #[serde(default)]
    pub intervalPeriod: Option<OadrIntervalPeriod>,
    #[serde(default)]
    pub payloads: Vec<OadrPayload>,
}

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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrPayload {
    /// Payload type, e.g. "PRICE", "EXPORT_PRICE", "GHG", "MAX_POWER"
    pub r#type: String,
    /// Mixed-type array depending on payload type; internal use only.
    #[serde(default)]
    pub values: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OadrReportDescriptor {
    pub payloadType: String,
    #[serde(default)]
    pub readingType: Option<String>,
    /// Number of *intervals* that elapse between reports -- not seconds.
    /// `-1` (and absent) means "same as `numIntervals`".
    #[serde(default)]
    pub frequency: Option<i64>,
    /// Number of intervals a single report covers. `-1` (and absent) means
    /// every interval of the event.
    #[serde(default)]
    pub numIntervals: Option<i64>,
    /// Spec default true (historical data); false requests a forecast.
    #[serde(default)]
    pub historical: Option<bool>,
}

/// A minimal event for tests; set the fields the test is actually about with
/// `..OadrEvent::test_event(id, program_id)`.
///
/// Exists because adding a protocol field the VEN must carry previously meant
/// editing every fixture by hand -- churn expensive enough that "just add the
/// field" kept losing to "leave it out", which is how `event.duration` and
/// `randomizeStart` came to be silently dropped in the first place.
#[cfg(test)]
impl OadrEvent {
    pub fn test_event(id: &str, program_id: &str) -> Self {
        Self {
            id: id.to_string(),
            programID: program_id.to_string(),
            eventName: None,
            priority: None,
            createdDateTime: None,
            duration: None,
            intervalPeriod: None,
            intervals: Vec::new(),
            reportDescriptors: None,
            targets: None,
            payloadDescriptors: None,
        }
    }
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
        assert_eq!(event.id, "evt-001");
        assert_eq!(event.programID, "prog-001");
        assert_eq!(event.eventName.as_deref(), Some("test-event"));
        assert_eq!(event.intervals.len(), 1);
        let period = event.intervals[0].intervalPeriod.as_ref().unwrap();
        assert_eq!(period.start.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(period.duration.as_deref(), Some("PT1H"));
        let payload = &event.intervals[0].payloads[0];
        assert_eq!(payload.r#type, "PRICE");
        let desc = &event.reportDescriptors.as_ref().unwrap()[0];
        assert_eq!(desc.payloadType, "USAGE");
    }

    #[test]
    fn test_oadr_event_absent_optional_fields_are_none() {
        let json = r#"{ "id": "evt-002", "programID": "prog-001" }"#;
        let event: OadrEvent = serde_json::from_str(json).expect("deserialization failed");
        assert_eq!(event.id, "evt-002");
        assert!(event.eventName.is_none());
        assert!(event.intervals.is_empty());
        assert!(event.reportDescriptors.is_none());
    }

    #[test]
    fn test_oadr_event_unknown_fields_do_not_panic() {
        let json = r#"{
            "id": "evt-003",
            "programID": "prog-001",
            "unknownFieldFromFutureVersion": "ignored",
            "anotherUnknown": 42
        }"#;
        let event: OadrEvent = serde_json::from_str(json).expect("unknown fields must be ignored");
        assert_eq!(event.id, "evt-003");
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
