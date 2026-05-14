use anyhow::Result;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use crate::controller::vtn_port::{OadrEvent, OadrProgram, OadrReport, VtnPort};

/// Test double for VtnPort. Configurable responses; records upsert calls for assertions.
pub struct MockVtn {
    pub programs: Vec<OadrProgram>,
    pub events: Vec<OadrEvent>,
    pub reports: Vec<OadrReport>,
    /// Captures every body passed to upsert_report; inspect in test assertions.
    pub submitted_reports: Arc<Mutex<Vec<serde_json::Value>>>,
    /// When Some(msg), upsert_report returns Err with this message.
    pub upsert_error: Option<String>,
}

impl MockVtn {
    pub fn new() -> Self {
        Self {
            programs: vec![],
            events: vec![],
            reports: vec![],
            submitted_reports: Arc::new(Mutex::new(vec![])),
            upsert_error: None,
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

    /// Return a clone of all submitted report bodies for assertion.
    pub fn submitted(&self) -> Vec<serde_json::Value> {
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

    async fn upsert_report(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        if let Some(ref msg) = self.upsert_error {
            anyhow::bail!("{}", msg);
        }
        self.submitted_reports.lock().unwrap().push(body.clone());
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_vtn_records_submitted_report() {
        let mock = MockVtn::new();
        let body = serde_json::json!({ "reportName": "ven-status" });
        mock.upsert_report(body.clone()).await.unwrap();
        assert_eq!(mock.submitted().len(), 1);
        assert_eq!(mock.submitted()[0]["reportName"], "ven-status");
    }

    #[tokio::test]
    async fn test_mock_vtn_returns_configured_error() {
        let mock = MockVtn::new().with_upsert_error("vtn unavailable");
        let result = mock.upsert_report(serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("vtn unavailable"));
    }

    #[tokio::test]
    async fn test_mock_vtn_returns_configured_events() {
        let event = OadrEvent {
            id: "e1".into(),
            programID: "p1".into(),
            eventName: None,
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
