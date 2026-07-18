//! WP-T5 (G-5) — outcome of a VEN-initiated report submission (`POST
//! /reports` / `PUT /reports/:id`), tracked beyond the aggregate
//! `reports_sent_total` counter so a client can ask "was this accepted"
//! after the request that triggered it has finished.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportSubmissionRecord {
    pub report_name: Option<String>,
    pub event_id: Option<String>,
    pub client_name: String,
    pub vtn_accepted: bool,
    pub submitted_at: DateTime<Utc>,
    pub error: Option<String>,
}

impl ReportSubmissionRecord {
    pub fn accepted(
        report_name: Option<String>,
        event_id: Option<String>,
        client_name: impl Into<String>,
        submitted_at: DateTime<Utc>,
    ) -> Self {
        Self {
            report_name,
            event_id,
            client_name: client_name.into(),
            vtn_accepted: true,
            submitted_at,
            error: None,
        }
    }

    pub fn rejected(
        report_name: Option<String>,
        event_id: Option<String>,
        client_name: impl Into<String>,
        submitted_at: DateTime<Utc>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            report_name,
            event_id,
            client_name: client_name.into(),
            vtn_accepted: false,
            submitted_at,
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn accepted_sets_true_and_no_error() {
        let r = ReportSubmissionRecord::accepted(
            Some("report-1".into()),
            Some("evt-1".into()),
            "ven-1",
            ts(),
        );
        assert!(r.vtn_accepted);
        assert_eq!(r.error, None);
        assert_eq!(r.client_name, "ven-1");
    }

    #[test]
    fn rejected_sets_false_and_carries_error() {
        let r = ReportSubmissionRecord::rejected(
            None,
            Some("evt-2".into()),
            "ven-1",
            ts(),
            "vtn unreachable",
        );
        assert!(!r.vtn_accepted);
        assert_eq!(r.error.as_deref(), Some("vtn unreachable"));
        assert_eq!(r.report_name, None);
    }

    #[test]
    fn serde_roundtrip_uses_snake_case_fields() {
        let r = ReportSubmissionRecord::accepted(Some("report-1".into()), None, "ven-1", ts());
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"report_name\":\"report-1\""));
        assert!(json.contains("\"vtn_accepted\":true"));
        assert!(json.contains("\"client_name\":\"ven-1\""));
        let back: ReportSubmissionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
