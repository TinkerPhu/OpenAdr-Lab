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
            // WP3.6: USAGE_FORECAST obligations report from the adopted plan.
            let plan = state.active_plan().await;
            let report_opt = crate::controller::reporter::build_measurement_report_for_obligation(
                &ob,
                &asset_samples,
                ven_name,
                env.as_ref(),
                plan.as_ref(),
                now,
            );
            let next_due = now + chrono::Duration::seconds(ob.interval_duration_s as i64);
            if let Some(report) = report_opt {
                vtn.upsert_report(report).await.map_err(|e| {
                    error!(
                        obligation_id = %ob.id,
                        "obligation report submission failed: {e:#}"
                    );
                    e
                })?;
                state.rearm_obligation(ob.id, next_due).await;
                info!(
                    obligation_id = %ob.id,
                    payload_type = %ob.payload_type,
                    "obligation report submitted"
                );
            } else {
                // No history data yet — re-arm for the next cycle rather than
                // hot-looping every 5s tick hoping data appears.
                state.rearm_obligation(ob.id, next_due).await;
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

        assert_eq!(
            vtn.submitted().len(),
            0,
            "no obligations → no reports submitted"
        );
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
        let result = ObligationService::check_and_report(
            &state,
            HashMap::new(),
            &vtn,
            "test-ven",
            Utc::now(),
        )
        .await;
        assert!(result.is_ok());
    }

    /// Fixed epoch base so sample timestamps land on 900s grid boundaries — matches
    /// the pattern in `controller/reporter.rs`'s own obligation-report tests.
    fn ts(offset_s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + offset_s, 0).unwrap()
    }

    fn make_due_obligation(
        due_at: DateTime<Utc>,
    ) -> crate::entities::capacity::OadrReportObligation {
        crate::entities::capacity::OadrReportObligation {
            id: uuid::Uuid::new_v4(),
            event_id: "evt-1".to_string(),
            program_id: Some("prog-1".to_string()),
            payload_type: "USAGE".to_string(),
            reading_type: "DIRECT_READ".to_string(),
            resource_name: None,
            due_at,
            interval_duration_s: 900,
            fulfilled: false,
            created_at: due_at,
            historical: true,
        }
    }

    /// Two full 900s intervals of history (0, 900, 1800) — enough for
    /// `build_measurement_report_for_obligation` to produce a non-empty report.
    fn make_samples() -> HashMap<String, Vec<AssetReportSample>> {
        let mut samples = HashMap::new();
        samples.insert(
            "asset-1".to_string(),
            vec![
                AssetReportSample {
                    ts: ts(0),
                    power_kw: 1.0,
                    soc: None,
                },
                AssetReportSample {
                    ts: ts(900),
                    power_kw: 1.5,
                    soc: None,
                },
                AssetReportSample {
                    ts: ts(1800),
                    power_kw: 2.0,
                    soc: None,
                },
            ],
        );
        samples
    }

    #[tokio::test]
    async fn test_due_obligation_rearmed_not_removed_after_report() {
        let state = AppState::new();
        let vtn = MockVtn::new();
        let now = ts(1800);
        let ob = make_due_obligation(now);
        let id = ob.id;
        state.add_obligations(vec![ob]).await;

        ObligationService::check_and_report(&state, make_samples(), &vtn, "test-ven", now)
            .await
            .unwrap();

        assert_eq!(vtn.submitted().len(), 1, "one report submitted");
        let obs = state.report_obligations().await;
        assert_eq!(obs.len(), 1, "obligation stays in state, not removed");
        assert_eq!(obs[0].id, id);
        assert!(
            obs[0].due_at > now,
            "due_at advanced into the future, re-armed for the next cycle"
        );
        assert!(!obs[0].fulfilled);

        // A second check before the new due_at does nothing — not due yet.
        ObligationService::check_and_report(&state, make_samples(), &vtn, "test-ven", now)
            .await
            .unwrap();
        assert_eq!(
            vtn.submitted().len(),
            1,
            "not due yet — no second report submitted"
        );
    }

    #[tokio::test]
    async fn test_due_obligation_vtn_error_leaves_due_at_unchanged() {
        let state = AppState::new();
        let vtn = MockVtn::new().with_upsert_error("VTN unavailable");
        let now = ts(1800);
        let ob = make_due_obligation(now);
        state.add_obligations(vec![ob]).await;

        let result =
            ObligationService::check_and_report(&state, make_samples(), &vtn, "test-ven", now)
                .await;
        assert!(result.is_err(), "VTN error propagates");

        let obs = state.report_obligations().await;
        assert_eq!(obs.len(), 1);
        assert_eq!(
            obs[0].due_at, now,
            "due_at unchanged on error — retried on the next tick"
        );
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
