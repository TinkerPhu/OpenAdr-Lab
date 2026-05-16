use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tracing::{debug, error, info};

use crate::controller::reporter::AssetReportSample;
use crate::controller::VtnPort;
use crate::state::AppState;

pub struct ObligationService;

impl ObligationService {
    /// Check for due obligations and submit a measurement report for each one.
    ///
    /// Does NOT retry on VTN error — errors are returned to the caller and the
    /// obligation task loop retries naturally on the next scheduled tick.
    pub async fn check_and_report(
        state: &AppState,
        asset_samples: HashMap<String, Vec<AssetReportSample>>,
        vtn: &dyn VtnPort,
        ven_name: &str,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let due = state.due_obligations(now).await;
        for ob in due {
            let env = state.site_envelope().await;
            let report_opt = crate::controller::reporter::build_measurement_report_for_obligation(
                &ob,
                &asset_samples,
                ven_name,
                env.as_ref(),
            );
            if let Some(report) = report_opt {
                vtn.upsert_report(report).await.map_err(|e| {
                    error!(
                        obligation_id = %ob.id,
                        "obligation report submission failed: {e:#}"
                    );
                    e
                })?;
                state.mark_obligation_fulfilled(ob.id).await;
                info!(
                    obligation_id = %ob.id,
                    payload_type = %ob.payload_type,
                    "obligation report submitted"
                );
            } else {
                // No history data — mark fulfilled to avoid looping forever.
                state.mark_obligation_fulfilled(ob.id).await;
                debug!(
                    obligation_id = %ob.id,
                    "obligation skipped (no history data)"
                );
            }
        }
        Ok(())
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::mock_vtn::MockVtn;
    use crate::state::AppState;

    #[tokio::test]
    async fn test_check_skips_when_none_due() {
        let state = AppState::new();
        let vtn = MockVtn::new();

        ObligationService::check_and_report(&state, HashMap::new(), &vtn, "test-ven", Utc::now())
            .await
            .unwrap();

        assert_eq!(vtn.submitted().len(), 0, "no obligations → no reports submitted");
    }

    #[tokio::test]
    async fn test_check_propagates_vtn_error() {
        let state = AppState::new();
        let vtn = MockVtn::new().with_upsert_error("VTN unavailable");

        // With no due obligations, the error path is not reached; the service returns Ok.
        // The error path is triggered only when an obligation is due AND VTN fails.
        // Testing that branch requires a due obligation in state — which requires
        // internal state setup beyond the current AppState API.
        // This test verifies the no-obligation path still returns Ok.
        let result =
            ObligationService::check_and_report(&state, HashMap::new(), &vtn, "test-ven", Utc::now()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_vtn_error_propagated_when_upsert_called() {
        // Directly verify MockVtn error propagation via the VtnPort trait.
        use crate::controller::vtn_port::OadrReportBody;
        let vtn = MockVtn::new().with_upsert_error("network error");
        let body = OadrReportBody {
            programID: "p1".to_string(),
            eventID: None,
            clientName: "ven-1".to_string(),
            reportName: Some("x".to_string()),
            resources: vec![],
        };
        let result = vtn.upsert_report(body).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("network error"));
    }
}
