use anyhow::Result;
use async_trait::async_trait;
use reqwest::StatusCode;
use std::sync::{Arc, Mutex};

use crate::controller::vtn_port::{OadrEvent, OadrProgram, OadrReport, OadrReportBody, VtnPort};
use crate::vtn::VtnHttpError;

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
    /// `anyhow::Error` downcastable to `VtnHttpError` carrying that status —
    /// lets obligation-service tests simulate a specific VTN HTTP status
    /// (e.g. 404) without a real HTTP round-trip. Takes precedence over
    /// `upsert_error` when both are set.
    pub upsert_error_status: Option<(StatusCode, String)>,
    /// GB-23: like `upsert_error_status`, but scoped to bodies whose
    /// `reportName` matches — every other `reportName` succeeds normally.
    /// Lets a single `MockVtn` simulate "this one obligation's report 404s,
    /// its sibling doesn't" within one `check_and_report` call.
    pub upsert_error_status_for: Option<(String, StatusCode, String)>,
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
        }
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
    /// downcastable to `VtnHttpError` — distinct from `with_upsert_error`'s
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

#[async_trait]
impl VtnPort for MockVtn {
    async fn fetch_programs(&self) -> Result<Vec<OadrProgram>> {
        Ok(self.programs.clone())
    }

    async fn fetch_events(&self) -> Result<Vec<OadrEvent>> {
        Ok(self.events.clone())
    }

    async fn fetch_reports(&self) -> Result<Vec<OadrReport>> {
        Ok(self.reports.clone())
    }

    async fn upsert_report(&self, body: OadrReportBody) -> Result<()> {
        if let Some((ref name, status, ref msg)) = self.upsert_error_status_for {
            if body.reportName.as_deref() == Some(name.as_str()) {
                return Err(anyhow::Error::new(VtnHttpError::new(status, msg.clone())));
            }
            self.submitted_reports.lock().unwrap().push(body);
            return Ok(());
        }
        if let Some((status, ref msg)) = self.upsert_error_status {
            return Err(anyhow::Error::new(VtnHttpError::new(status, msg.clone())));
        }
        if let Some(ref msg) = self.upsert_error {
            anyhow::bail!("{}", msg);
        }
        self.submitted_reports.lock().unwrap().push(body);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_vtn_records_submitted_report() {
        let mock = MockVtn::new();
        let body = OadrReportBody {
            programID: "prog-1".to_string(),
            eventID: None,
            clientName: "ven-1".to_string(),
            reportName: Some("ven-status".to_string()),
            resources: vec![],
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
            programID: "prog-1".to_string(),
            eventID: None,
            clientName: "ven-1".to_string(),
            reportName: Some("test".to_string()),
            resources: vec![],
        };
        let result = mock.upsert_report(body).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("vtn unavailable"));
    }

    #[tokio::test]
    async fn mock_vtn_returns_configured_status_error_downcastable() {
        let mock = MockVtn::new().with_upsert_error_status(StatusCode::NOT_FOUND, "gone");
        let body = OadrReportBody {
            programID: "prog-1".to_string(),
            eventID: None,
            clientName: "ven-1".to_string(),
            reportName: Some("test".to_string()),
            resources: vec![],
        };
        let err = mock.upsert_report(body).await.unwrap_err();
        let vtn_err = err
            .downcast_ref::<VtnHttpError>()
            .expect("must downcast to VtnHttpError");
        assert_eq!(vtn_err.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_mock_vtn_returns_configured_events() {
        let event = OadrEvent {
            id: "e1".into(),
            programID: "p1".into(),
            eventName: None,
            priority: None,
            createdDateTime: None,
            intervalPeriod: None,
            intervals: vec![],
            reportDescriptors: None,
        };
        let mock = MockVtn::new().with_events(vec![event]);
        let events = mock.fetch_events().await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "e1");
    }
}
