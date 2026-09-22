use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::controller::history_port::record_report_sent;
use crate::controller::reporter::AssetReportSample;
use crate::controller::{HistoryPort, VtnPort};
use crate::state::AppState;
use crate::vtn::VtnHttpError;

pub struct ObligationService;

impl ObligationService {
    /// Check for due obligations and submit a measurement report for each one.
    ///
    /// GB-23: a 404 from the VTN on a specific obligation's `upsert_report`
    /// call is treated as confirmation that obligation's source event/program
    /// is gone — the obligation is dropped (design.md D2) instead of being
    /// re-armed for another retry. Every other VTN error still does NOT
    /// retry-loop internally: it's returned to the caller and the obligation
    /// task loop retries naturally on the next scheduled tick, with `due_at`
    /// unchanged.
    ///
    /// R-43 (design.md D3): `history`, when present, receives one
    /// `ReportSent` row per successful submission (including the case where
    /// the obligation had no history data — no row is appended there, since
    /// nothing was actually sent to the VTN).
    pub async fn check_and_report(
        state: &AppState,
        asset_samples: HashMap<String, Vec<AssetReportSample>>,
        vtn: &dyn VtnPort,
        ven_name: &str,
        now: DateTime<Utc>,
        history: Option<Arc<dyn HistoryPort>>,
    ) -> Result<()> {
        let due = state.due_obligations(now).await;
        for ob in due {
            let env = state.site_envelope().await;
            // WP3.6: USAGE_FORECAST obligations report from the adopted plan.
            let plan = state.active_plan().await;
            // WP5.4: BASELINE obligations report the event-blind heuristic forecast.
            let heuristics = state.asset_heuristics().await;
            // openspec/changes/flexibility-capacity-forecast/: STORAGE_MAX_CHARGE_POWER /
            // STORAGE_MAX_DISCHARGE_POWER obligations report the sustained-commitment
            // capacity curve, re-derived fresh every tick (see `state::capacity_curves`).
            let curves = state.capacity_curves().await;
            let report_opt = crate::controller::reporter::build_measurement_report_for_obligation(
                &ob,
                &asset_samples,
                ven_name,
                env.as_ref(),
                plan.as_ref(),
                &heuristics,
                curves.as_ref(),
                now,
            );
            let next_due = now + chrono::Duration::seconds(ob.submit_every_s as i64);
            if let Some(mut report) = report_opt {
                // D-3: a measurement report sends the whole remembered
                // window, not only what closed since the last submission. A
                // `PUT` replaces the object on the VTN, so sending just the
                // new intervals would leave the series two intervals wide for
                // ever.
                //
                // A *forecast* report is the opposite: it says what the VEN now
                // expects, and every submission supersedes the last one whole.
                // Accumulating it would leave intervals from a forecast the VEN
                // has since changed its mind about sitting in the same array as
                // the current one, indistinguishable from it.
                if ob.historical {
                    accumulate_report_window(state, &mut report, now).await;
                }
                match vtn.upsert_report(report).await {
                    Ok(()) => {
                        state.rearm_obligation(ob.id, next_due).await;
                        info!(
                            obligation_id = %ob.id,
                            payload_type = %ob.payload_type,
                            "obligation report submitted"
                        );
                        record_report_sent(
                            history.clone(),
                            ob.payload_type.clone(),
                            ob.event_id.clone(),
                            now,
                        )
                        .await;
                    }
                    Err(e) => {
                        let is_not_found = e
                            .downcast_ref::<VtnHttpError>()
                            .is_some_and(|v| v.status == StatusCode::NOT_FOUND);
                        if is_not_found {
                            // GB-23: source event/program confirmed gone — drop
                            // this obligation rather than retrying forever.
                            // Only this obligation is removed (design.md D2):
                            // a 404 on one submission does not by itself prove
                            // every obligation sharing its event_id is gone.
                            state.remove_obligation(ob.id).await;
                            info!(
                                obligation_id = %ob.id,
                                event_id = %ob.event_id,
                                program_id = ?ob.program_id,
                                "obligation dropped: VTN confirmed its source is gone (404)"
                            );
                        } else {
                            error!(
                                obligation_id = %ob.id,
                                "obligation report submission failed: {e:#}"
                            );
                            return Err(e);
                        }
                    }
                }
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

/// Replace each resource's freshly-closed intervals with its accumulated
/// window (D-3).
///
/// Keyed per report *and* resource: two resources of one report are two
/// series, and merging them would put one asset's values into the other's.
async fn accumulate_report_window(
    state: &AppState,
    report: &mut crate::controller::vtn_port::OadrReportBody,
    now: DateTime<Utc>,
) {
    let Some(report_name) = report.reportName.clone() else {
        // Without a stable name there is nothing to accumulate against: every
        // submission would create a new report anyway.
        return;
    };
    for resource in report.resources.iter_mut() {
        let key = format!("{report_name}/{}", resource.resourceName);
        let fresh = std::mem::take(&mut resource.intervals);
        resource.intervals = state.accumulate_report_intervals(&key, fresh, now).await;
    }

    // Re-derive the descriptors from the intervals that are actually going
    // out. The reporter computed them from this submission's intervals alone,
    // and accumulation has just changed the set: a payload type present only
    // in a carried-forward interval would otherwise travel undeclared, which
    // is exactly the failure `wire-contracts` exists to prevent (GB-50).
    let all: Vec<_> = report
        .resources
        .iter()
        .flat_map(|r| r.intervals.iter().cloned())
        .collect();
    report.payloadDescriptors = crate::controller::report_payload::descriptors_for(&all);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::mock_vtn::MockVtn;
    use crate::state::AppState;

    #[tokio::test]
    async fn test_check_skips_when_none_due() {
        let state = AppState::new();
        let vtn = MockVtn::new();

        ObligationService::check_and_report(
            &state,
            HashMap::new(),
            &vtn,
            "test-ven",
            Utc::now(),
            None,
        )
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
            None,
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
            interval_width_s: 900,
            submit_every_s: 900,
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

        ObligationService::check_and_report(&state, make_samples(), &vtn, "test-ven", now, None)
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
        ObligationService::check_and_report(&state, make_samples(), &vtn, "test-ven", now, None)
            .await
            .unwrap();
        assert_eq!(
            vtn.submitted().len(),
            1,
            "not due yet — no second report submitted"
        );
    }

    /// D-3: a `PUT` replaces the report on the VTN, so each submission has to
    /// carry the whole window. Sending only what closed since last time leaves
    /// the series permanently two intervals wide, which is the shape a reader
    /// cannot tell from "this VEN only ran for two minutes".
    #[tokio::test]
    async fn measurement_reports_accumulate_their_intervals_across_submissions() {
        let state = AppState::new();
        let vtn = MockVtn::new();
        let first = ts(1800);
        state
            .add_obligations(vec![make_due_obligation(first)])
            .await;

        ObligationService::check_and_report(&state, make_samples(), &vtn, "test-ven", first, None)
            .await
            .unwrap();
        let first_count = vtn.submitted()[0].resources[0].intervals.len();
        assert!(first_count >= 1);

        // The second submission sees only the samples that arrived since --
        // which is what the live sample buffer hands it, and the whole reason
        // the window has to be remembered here.
        let second = first + chrono::Duration::seconds(1800);
        let mut samples = HashMap::new();
        samples.insert(
            "asset-1".to_string(),
            vec![
                AssetReportSample {
                    ts: ts(1800),
                    power_kw: 2.0,
                    soc: None,
                },
                AssetReportSample {
                    ts: ts(2700),
                    power_kw: 3.0,
                    soc: None,
                },
                AssetReportSample {
                    ts: ts(3600),
                    power_kw: 4.0,
                    soc: None,
                },
            ],
        );
        ObligationService::check_and_report(&state, samples, &vtn, "test-ven", second, None)
            .await
            .unwrap();

        let submitted = vtn.submitted();
        assert_eq!(submitted.len(), 2);
        let second_intervals = &submitted[1].resources[0].intervals;
        assert_eq!(
            second_intervals.len(),
            first_count + 1,
            "the second submission must carry the earlier interval as well as              the newly closed one, not replace it"
        );
        let earliest = submitted[0].resources[0].intervals[0]
            .intervalPeriod
            .as_ref()
            .and_then(|p| p.start.clone());
        assert_eq!(
            second_intervals[0]
                .intervalPeriod
                .as_ref()
                .and_then(|p| p.start.clone()),
            earliest,
            "the window still starts where the first submission started"
        );
    }

    /// The descriptors say what the values in the report mean, and after
    /// accumulation the report carries intervals the reporter never saw. A
    /// payload type present only in a carried-forward interval must still be
    /// declared -- an undeclared value on the wire is GB-50 exactly.
    #[tokio::test]
    async fn accumulated_reports_declare_every_payload_type_they_still_carry() {
        let state = AppState::new();
        let vtn = MockVtn::new();
        let first = ts(1800);
        state
            .add_obligations(vec![make_due_obligation(first)])
            .await;

        ObligationService::check_and_report(&state, make_samples(), &vtn, "test-ven", first, None)
            .await
            .unwrap();

        let second = first + chrono::Duration::seconds(1800);
        let mut samples = HashMap::new();
        samples.insert(
            "asset-1".to_string(),
            vec![
                AssetReportSample {
                    ts: ts(1800),
                    power_kw: 2.0,
                    soc: None,
                },
                AssetReportSample {
                    ts: ts(2700),
                    power_kw: 3.0,
                    soc: None,
                },
                AssetReportSample {
                    ts: ts(3600),
                    power_kw: 4.0,
                    soc: None,
                },
            ],
        );
        ObligationService::check_and_report(&state, samples, &vtn, "test-ven", second, None)
            .await
            .unwrap();

        let report = &vtn.submitted()[1];
        for interval in &report.resources[0].intervals {
            for payload in &interval.payloads {
                assert!(
                    report
                        .payloadDescriptors
                        .iter()
                        .any(|d| d.payloadType == payload.r#type),
                    "{} travels undeclared in the accumulated report",
                    payload.r#type
                );
            }
        }
    }

    /// The case the fixture above cannot produce: this submission carries one
    /// payload type, the window still holds an interval with two. Without
    /// re-deriving the descriptors, the second type goes out undeclared.
    #[tokio::test]
    async fn descriptors_cover_a_payload_type_only_a_carried_forward_interval_has() {
        use crate::controller::vtn_port::{
            OadrIntervalPeriod, OadrReportBody, OadrReportInterval, OadrReportPayload,
            OadrReportResource,
        };

        let state = AppState::new();
        let now = ts(3600);

        let mut with_state = OadrReportInterval {
            id: 0,
            intervalPeriod: Some(OadrIntervalPeriod {
                start: Some("2026-09-22T10:00:00+00:00".to_string()),
                duration: Some("PT15M".to_string()),
                randomizeStart: None,
            }),
            payloads: vec![OadrReportPayload::energy_kwh("USAGE", 1.0)],
        };
        with_state
            .payloads
            .push(OadrReportPayload::state("OPERATING_STATE", "ACTIVE"));
        state
            .accumulate_report_intervals("r/ven-meter", vec![with_state], now)
            .await;

        let fresh = OadrReportInterval {
            id: 0,
            intervalPeriod: Some(OadrIntervalPeriod {
                start: Some("2026-09-22T10:15:00+00:00".to_string()),
                duration: Some("PT15M".to_string()),
                randomizeStart: None,
            }),
            payloads: vec![OadrReportPayload::energy_kwh("USAGE", 2.0)],
        };
        let mut report = OadrReportBody {
            eventID: Some("evt-1".to_string()),
            clientName: "test-ven".to_string(),
            reportName: Some("r".to_string()),
            payloadDescriptors: crate::controller::report_payload::descriptors_for(
                std::slice::from_ref(&fresh),
            ),
            resources: vec![OadrReportResource {
                resourceName: "ven-meter".to_string(),
                intervals: vec![fresh],
            }],
        };
        assert!(
            !report
                .payloadDescriptors
                .iter()
                .any(|d| d.payloadType == "OPERATING_STATE"),
            "precondition: this submission declares only USAGE"
        );

        accumulate_report_window(&state, &mut report, now).await;

        assert_eq!(report.resources[0].intervals.len(), 2);
        assert!(
            report
                .payloadDescriptors
                .iter()
                .any(|d| d.payloadType == "OPERATING_STATE"),
            "the carried-forward interval's OPERATING_STATE travels undeclared"
        );
    }

    /// A forecast says what the VEN now expects, and each submission
    /// supersedes the last one whole. Accumulating it would leave intervals
    /// from a forecast the VEN has changed its mind about sitting in the same
    /// array as the current one, indistinguishable from it.
    #[tokio::test]
    async fn forecast_reports_are_not_accumulated() {
        let state = AppState::new();
        let now = ts(1800);
        let mut ob = make_due_obligation(now);
        ob.payload_type = "USAGE_FORECAST".to_string();
        ob.historical = false;
        state.add_obligations(vec![ob]).await;

        ObligationService::check_and_report(
            &state,
            make_samples(),
            &vtn_for_forecast(),
            "test-ven",
            now,
            None,
        )
        .await
        .ok();

        assert!(
            state.report_window_sizes().await.is_empty(),
            "a forecast obligation must not open an accumulation window"
        );
    }

    fn vtn_for_forecast() -> MockVtn {
        MockVtn::new()
    }

    #[tokio::test]
    async fn test_due_obligation_vtn_error_leaves_due_at_unchanged() {
        let state = AppState::new();
        let vtn = MockVtn::new().with_upsert_error("VTN unavailable");
        let now = ts(1800);
        let ob = make_due_obligation(now);
        state.add_obligations(vec![ob]).await;

        let result = ObligationService::check_and_report(
            &state,
            make_samples(),
            &vtn,
            "test-ven",
            now,
            None,
        )
        .await;
        assert!(result.is_err(), "VTN error propagates");

        let obs = state.report_obligations().await;
        assert_eq!(obs.len(), 1);
        assert_eq!(
            obs[0].due_at, now,
            "due_at unchanged on error — retried on the next tick"
        );
    }

    // ── GB-23: drop obligation on confirmed 404 ─────────────────────────────

    #[tokio::test]
    async fn due_obligation_404_is_removed_not_rearmed() {
        let state = AppState::new();
        let vtn = MockVtn::new().with_upsert_error_status(StatusCode::NOT_FOUND, "gone");
        let now = ts(1800);
        let ob = make_due_obligation(now);
        state.add_obligations(vec![ob]).await;

        let result = ObligationService::check_and_report(
            &state,
            make_samples(),
            &vtn,
            "test-ven",
            now,
            None,
        )
        .await;
        assert!(result.is_ok(), "404 is handled, not propagated as an error");

        let obs = state.report_obligations().await;
        assert!(
            obs.is_empty(),
            "obligation must be removed from state after a confirmed 404"
        );
    }

    #[tokio::test]
    async fn a_404_on_one_obligation_does_not_remove_sibling_obligations() {
        // Two obligations sharing an event_id; only the "USAGE" one's report
        // 404s (targeted by reportName). Per design.md D2, removal is keyed
        // by the obligation's own confirmed 404, not inferred from a sibling.
        let now = ts(1800);
        let ob_a = make_due_obligation(now); // payload_type USAGE, event_id "evt-1"
        let mut ob_b = make_due_obligation(now); // same event_id "evt-1" (see helper)
        ob_b.payload_type = "DEMAND".to_string();
        let id_b = ob_b.id;
        assert_eq!(
            ob_a.event_id, ob_b.event_id,
            "test relies on a shared event_id"
        );
        let report_name_a = format!("ob-test-ven-{}-USAGE", ob_a.event_id);

        let vtn = MockVtn::new().with_upsert_error_status_for(
            &report_name_a,
            StatusCode::NOT_FOUND,
            "gone",
        );
        let state = AppState::new();
        state.add_obligations(vec![ob_a, ob_b]).await;

        ObligationService::check_and_report(&state, make_samples(), &vtn, "test-ven", now, None)
            .await
            .unwrap();

        let obs = state.report_obligations().await;
        assert_eq!(
            obs.len(),
            1,
            "only the 404'd obligation is removed, its sibling remains"
        );
        assert_eq!(obs[0].id, id_b);
        assert!(
            obs[0].due_at > now,
            "the surviving sibling was successfully submitted and re-armed"
        );
    }

    #[tokio::test]
    async fn due_obligation_non_404_error_is_not_dropped() {
        // Regression guard for the downcast path: a non-404 status-carrying
        // error must still propagate and leave due_at unchanged, exactly like
        // a plain string error (test_due_obligation_vtn_error_leaves_due_at_unchanged).
        let state = AppState::new();
        let vtn =
            MockVtn::new().with_upsert_error_status(StatusCode::INTERNAL_SERVER_ERROR, "boom");
        let now = ts(1800);
        let ob = make_due_obligation(now);
        state.add_obligations(vec![ob]).await;

        let result = ObligationService::check_and_report(
            &state,
            make_samples(),
            &vtn,
            "test-ven",
            now,
            None,
        )
        .await;
        assert!(result.is_err(), "non-404 VTN error still propagates");

        let obs = state.report_obligations().await;
        assert_eq!(obs.len(), 1, "obligation is not dropped on a non-404 error");
        assert_eq!(obs[0].due_at, now, "due_at unchanged — retried next tick");
    }

    // ── R-43: append_report_sent wiring ──────────────────────────────────────

    #[tokio::test]
    async fn successful_obligation_report_appends_a_report_sent_row() {
        use crate::services::test_support::mock_history_port::MockHistoryPort;

        let state = AppState::new();
        let vtn = MockVtn::new();
        let history = Arc::new(MockHistoryPort::new());
        let now = ts(1800);
        let ob = make_due_obligation(now);

        state.add_obligations(vec![ob]).await;
        ObligationService::check_and_report(
            &state,
            make_samples(),
            &vtn,
            "test-ven",
            now,
            Some(history.clone()),
        )
        .await
        .unwrap();

        let rows = history.appended_reports();
        assert_eq!(rows.len(), 1, "one ReportSent row appended");
        assert_eq!(rows[0].report_type, "USAGE");
        assert_eq!(rows[0].event_id, "evt-1");
    }

    #[tokio::test]
    async fn no_history_port_configured_is_a_no_op_not_an_error() {
        let state = AppState::new();
        let vtn = MockVtn::new();
        let now = ts(1800);
        let ob = make_due_obligation(now);
        state.add_obligations(vec![ob]).await;

        let result = ObligationService::check_and_report(
            &state,
            make_samples(),
            &vtn,
            "test-ven",
            now,
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "absent HistoryPort must not fail submission"
        );
        assert_eq!(
            vtn.submitted().len(),
            1,
            "report still submitted to the VTN"
        );
    }

    #[tokio::test]
    async fn test_mock_vtn_error_propagated_when_upsert_called() {
        // Directly verify MockVtn error propagation via the VtnPort trait.
        use crate::controller::vtn_port::OadrReportBody;
        let vtn = MockVtn::new().with_upsert_error("network error");
        let body = OadrReportBody {
            eventID: None,
            clientName: "ven-1".to_string(),
            reportName: Some("x".to_string()),
            resources: vec![],
            payloadDescriptors: Vec::new(),
        };
        let result = vtn.upsert_report(body).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("network error"));
    }
}
