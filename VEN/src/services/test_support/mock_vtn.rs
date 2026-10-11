use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use std::sync::{Arc, Mutex};

use crate::controller::vtn_port::{OadrEvent, OadrProgram, OadrReport, OadrReportBody, VtnPort};
use crate::controller::wire_reject::FetchOutcome;
use crate::entities::DomainError;

/// Test double for VtnPort. Configurable responses; records upsert calls for assertions.
pub struct MockVtn {
    pub programs: Vec<OadrProgram>,
    pub events: Vec<OadrEvent>,
    pub reports: Vec<OadrReport>,
    /// Captures every body passed to upsert_report; inspect in test assertions.
    pub submitted_reports: Arc<Mutex<Vec<OadrReportBody>>>,
    /// When Some(msg), upsert_report returns Err with this message.
    pub upsert_error: Option<String>,
    /// GB-23: when Some((status, msg)), upsert_report returns an
    /// `anyhow::Error` downcastable to `DomainError::VtnRejected` carrying that status —
    /// lets obligation-service tests simulate a specific VTN HTTP status
    /// (e.g. 404) without a real HTTP round-trip. Takes precedence over
    /// `upsert_error` when both are set.
    pub upsert_error_status: Option<(StatusCode, String)>,
    /// GB-23: like `upsert_error_status`, but scoped to bodies whose
    /// `reportName` matches — every other `reportName` succeeds normally.
    /// Lets a single `MockVtn` simulate "this one obligation's report 404s,
    /// its sibling doesn't" within one `check_and_report` call.
    pub upsert_error_status_for: Option<(String, StatusCode, String)>,
    /// Captures every `(id, body)` passed to update_report.
    pub updated_reports: Arc<Mutex<Vec<(String, OadrReportBody)>>>,
    /// When Some(msg), update_report returns Err with this message.
    pub update_error: Option<String>,
    /// What `token_expires_at` answers.
    pub token_expires_at: Option<DateTime<Utc>>,
}

impl MockVtn {
    pub fn new() -> Self {
        Self {
            programs: vec![],
            events: vec![],
            reports: vec![],
            submitted_reports: Arc::new(Mutex::new(vec![])),
            upsert_error: None,
            upsert_error_status: None,
            upsert_error_status_for: None,
            updated_reports: Arc::new(Mutex::new(vec![])),
            update_error: None,
            token_expires_at: None,
        }
    }

    pub fn with_update_error(mut self, msg: &str) -> Self {
        self.update_error = Some(msg.to_string());
        self
    }

    pub fn with_token_expires_at(mut self, at: DateTime<Utc>) -> Self {
        self.token_expires_at = Some(at);
        self
    }

    /// Return a clone of every `(id, body)` update_report received.
    pub fn updated(&self) -> Vec<(String, OadrReportBody)> {
        self.updated_reports.lock().unwrap().clone()
    }

    pub fn with_events(mut self, events: Vec<OadrEvent>) -> Self {
        self.events = events;
        self
    }

    pub fn with_upsert_error(mut self, msg: &str) -> Self {
        self.upsert_error = Some(msg.to_string());
        self
    }

    /// GB-23: inject an upsert failure that carries a specific HTTP status,
    /// downcastable to `DomainError::VtnRejected` — distinct from `with_upsert_error`'s
    /// opaque string, so callers can simulate e.g. a 404 specifically.
    pub fn with_upsert_error_status(mut self, status: StatusCode, msg: &str) -> Self {
        self.upsert_error_status = Some((status, msg.to_string()));
        self
    }

    /// GB-23: like `with_upsert_error_status`, scoped to one `reportName` —
    /// every other submission through this same mock succeeds normally.
    pub fn with_upsert_error_status_for(
        mut self,
        report_name: &str,
        status: StatusCode,
        msg: &str,
    ) -> Self {
        self.upsert_error_status_for = Some((report_name.to_string(), status, msg.to_string()));
        self
    }

    /// Return a clone of all submitted report bodies for assertion.
    pub fn submitted(&self) -> Vec<OadrReportBody> {
        self.submitted_reports.lock().unwrap().clone()
    }
}

/// A mock hands back objects that are already typed, so nothing can have been
/// refused. Rejection behaviour is covered where it lives (`wire_reject`) and
/// at the task layer, not by every mock-driven test.
fn all_valid<T>(items: Vec<T>) -> FetchOutcome<T> {
    FetchOutcome {
        items,
        rejected: Vec::new(),
    }
}

/// What the real client's `vtn::http_error` returns for a non-2xx answer.
fn rejected(status: StatusCode, msg: &str) -> anyhow::Error {
    anyhow::Error::new(DomainError::VtnRejected {
        status: status.as_u16(),
        message: msg.to_string(),
    })
}

#[async_trait]
impl VtnPort for MockVtn {
    async fn fetch_programs(&self) -> Result<FetchOutcome<OadrProgram>> {
        Ok(all_valid(self.programs.clone()))
    }

    async fn fetch_events(&self) -> Result<FetchOutcome<OadrEvent>> {
        Ok(all_valid(self.events.clone()))
    }

    async fn fetch_reports(&self) -> Result<FetchOutcome<OadrReport>> {
        Ok(all_valid(self.reports.clone()))
    }

    async fn upsert_report(&self, body: OadrReportBody) -> Result<()> {
        if let Some((ref name, status, ref msg)) = self.upsert_error_status_for {
            if body.reportName.as_deref() == Some(name.as_str()) {
                return Err(rejected(status, msg));
            }
            self.submitted_reports.lock().unwrap().push(body);
            return Ok(());
        }
        if let Some((status, ref msg)) = self.upsert_error_status {
            return Err(rejected(status, msg));
        }
        if let Some(ref msg) = self.upsert_error {
            anyhow::bail!("{}", msg);
        }
        self.submitted_reports.lock().unwrap().push(body);
        Ok(())
    }

    /// Answers like the VTN does: the body it was sent, under the id it was sent to.
    async fn update_report(&self, id: &str, body: OadrReportBody) -> Result<OadrReport> {
        if let Some(ref msg) = self.update_error {
            anyhow::bail!("{}", msg);
        }
        let mut extra = match serde_json::to_value(&body)? {
            serde_json::Value::Object(fields) => fields,
            _ => unreachable!("a report body serialises to an object"),
        };
        extra.remove("reportName");
        let report = OadrReport {
            id: id.to_string(),
            reportName: body.reportName.clone(),
            extra,
        };
        self.updated_reports
            .lock()
            .unwrap()
            .push((id.to_string(), body));
        Ok(report)
    }

    async fn token_expires_at(&self, _now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.token_expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_vtn_records_submitted_report() {
        let mock = MockVtn::new();
        let body = OadrReportBody {
            eventID: None,
            clientName: "ven-1".to_string(),
            reportName: Some("ven-status".to_string()),
            resources: vec![],
            payloadDescriptors: Vec::new(),
        };
        mock.upsert_report(body).await.unwrap();
        assert_eq!(mock.submitted().len(), 1);
        assert_eq!(
            mock.submitted()[0].reportName.as_deref(),
            Some("ven-status")
        );
    }

    #[tokio::test]
    async fn test_mock_vtn_returns_configured_error() {
        let mock = MockVtn::new().with_upsert_error("vtn unavailable");
        let body = OadrReportBody {
            eventID: None,
            clientName: "ven-1".to_string(),
            reportName: Some("test".to_string()),
            resources: vec![],
            payloadDescriptors: Vec::new(),
        };
        let result = mock.upsert_report(body).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("vtn unavailable"));
    }

    #[tokio::test]
    async fn mock_vtn_returns_configured_status_error_downcastable() {
        let mock = MockVtn::new().with_upsert_error_status(StatusCode::NOT_FOUND, "gone");
        let body = OadrReportBody {
            eventID: None,
            clientName: "ven-1".to_string(),
            reportName: Some("test".to_string()),
            resources: vec![],
            payloadDescriptors: Vec::new(),
        };
        let err = mock.upsert_report(body).await.unwrap_err();
        assert!(matches!(
            err.downcast_ref::<DomainError>(),
            Some(DomainError::VtnRejected { status: 404, .. })
        ));
    }

    #[tokio::test]
    async fn test_mock_vtn_returns_configured_events() {
        let event = lab_core::test_fixtures::events_from_json(serde_json::json!([{
            "id": "e1", "programID": "p1"
        }]))
        .remove(0);
        let mock = MockVtn::new().with_events(vec![event]);
        let events = mock.fetch_events().await.unwrap().items;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id.as_str(), "e1");
    }
}
