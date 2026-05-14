// VtnPort trait — the boundary between application/service logic and the VTN HTTP client.
//
// Typed structs model only the fields currently consumed by the VEN codebase.
// Field names preserve the OpenADR 3 convention verbatim (camelCase) per project policy.
// OpenADR 3.1 introduces breaking field/type name changes; keeping the struct surface minimal
// reduces future migration cost.
#![allow(non_snake_case)]

use async_trait::async_trait;
use anyhow::Result;
use serde::{Deserialize, Serialize};

// ── Port trait ─────────────────────────────────────────────────────────────────

#[async_trait]
pub trait VtnPort: Send + Sync {
    async fn fetch_programs(&self) -> Result<Vec<OadrProgram>>;
    async fn fetch_events(&self) -> Result<Vec<OadrEvent>>;
    async fn fetch_reports(&self) -> Result<Vec<OadrReport>>;
    /// Submit or upsert a report. Body and response stay as raw JSON because the
    /// full report shape is constructed by controller/reporter.rs and is not typed here.
    async fn upsert_report(&self, body: serde_json::Value) -> Result<serde_json::Value>;
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
    /// Event-level interval period — used for looping price events (e.g. duration = "P9999Y").
    #[serde(default)]
    pub intervalPeriod: Option<OadrIntervalPeriod>,
    #[serde(default)]
    pub intervals: Vec<OadrInterval>,
    #[serde(default)]
    pub reportDescriptors: Option<Vec<OadrReportDescriptor>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrInterval {
    #[serde(default)]
    pub intervalPeriod: Option<OadrIntervalPeriod>,
    #[serde(default)]
    pub payloads: Vec<OadrPayload>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrIntervalPeriod {
    /// ISO 8601 datetime string, e.g. "2026-01-01T00:00:00Z"
    #[serde(default)]
    pub start: Option<String>,
    /// ISO 8601 duration string, e.g. "PT1H"
    #[serde(default)]
    pub duration: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrPayload {
    /// Payload type, e.g. "PRICE", "EXPORT_PRICE", "GHG", "MAX_POWER"
    pub r#type: String,
    /// Mixed-type array depending on payload type; internal use only.
    #[serde(default)]
    pub values: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrReportDescriptor {
    pub payloadType: String,
    #[serde(default)]
    pub readingType: Option<String>,
    /// Reporting frequency in seconds.
    #[serde(default)]
    pub frequency: Option<i64>,
}

// ── OadrReport ────────────────────────────────────────────────────────────────

/// Minimal typed report — only `id` and `reportName` are accessed by field.
/// Full report bodies are passed as `serde_json::Value` through `upsert_report`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrReport {
    pub id: String,
    pub reportName: String,
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
        assert_eq!(report.reportName, "ven-status");
    }
}
