//! WP4.3 (BL-20) — application-layer notification service.
//!
//! Producers (tasks, services) call [`Notifier::notify`]; it fans one
//! `UserNotification` out to the three consumers: the in-memory ring on
//! `AppState` (live feed + `GET /notifications`), the SSE broadcast
//! (`GET /notifications/events`), and the history store (survives restarts).
//! Inner rings never gain outward deps: producers depend on this service,
//! not on the store or the transport.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use tracing::warn;

use crate::controller::HistoryPort;
use crate::entities::design_vocabulary::UserNotificationSeverity;
use crate::entities::notification::UserNotification;
use crate::state::AppState;

#[derive(Clone)]
pub struct Notifier {
    history: Option<Arc<dyn HistoryPort>>,
    tx: broadcast::Sender<UserNotification>,
}

impl Notifier {
    pub fn new(history: Option<Arc<dyn HistoryPort>>) -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { history, tx }
    }

    /// Subscribe to live notifications (SSE bridge).
    pub fn subscribe(&self) -> broadcast::Receiver<UserNotification> {
        self.tx.subscribe()
    }

    /// Create and fan out one notification. Storage failures are logged,
    /// never propagated — notifying must not break the calling code path.
    /// `now` is injected per the determinism rule.
    pub async fn notify(
        &self,
        state: &AppState,
        now: DateTime<Utc>,
        severity: UserNotificationSeverity,
        message: impl Into<String>,
        asset_id: Option<String>,
        event_id: Option<String>,
    ) -> UserNotification {
        let n = UserNotification::new(now, severity, message, asset_id, event_id);
        state.push_notification(n.clone()).await;
        let _ = self.tx.send(n.clone()); // no subscribers = fine
        if let Some(h) = self.history.clone() {
            let row = n.clone();
            match tokio::task::spawn_blocking(move || h.append_notification(&row)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => warn!(error = %e, "notification persist failed"),
                Err(e) => warn!(error = %e, "notification persist task panicked"),
            }
        }
        n
    }
}

/// WP4.3: warnings present in `cur` but absent from `prev` — the ones worth
/// telling the user about exactly once, mapped to notification severities.
/// Planner `Info` warnings are deliberately not surfaced (feed noise).
pub fn new_plan_warnings(
    prev: Option<&crate::entities::plan::Plan>,
    cur: &crate::entities::plan::Plan,
) -> Vec<(UserNotificationSeverity, String)> {
    use crate::entities::plan::WarningSeverity;
    let prev_msgs: Vec<&str> = prev
        .map(|p| p.warnings.iter().map(|w| w.message.as_str()).collect())
        .unwrap_or_default();
    cur.warnings
        .iter()
        .filter(|w| !prev_msgs.contains(&w.message.as_str()))
        .filter_map(|w| match w.severity {
            WarningSeverity::Critical => Some((UserNotificationSeverity::Alert, w.message.clone())),
            WarningSeverity::Warning => Some((UserNotificationSeverity::Warn, w.message.clone())),
            WarningSeverity::Info => None,
        })
        .collect()
}

/// WP4.3: VTN reachability edge → notification, `None` while steady.
/// `was_ok` = previous poll outcome, `now_ok` = this poll's outcome.
pub fn outage_transition(
    was_ok: bool,
    now_ok: bool,
) -> Option<(UserNotificationSeverity, &'static str)> {
    match (was_ok, now_ok) {
        (true, false) => Some((
            UserNotificationSeverity::Warn,
            "VTN unreachable — planning continues on last known rates",
        )),
        (false, true) => Some((UserNotificationSeverity::Info, "VTN connection restored")),
        _ => None,
    }
}

/// One-call producer for the planning task (kept here for the tasks/ file-size
/// cap): surfaces warnings the newly-adopted plan carries that the previous
/// plan didn't. No-op when the plan wasn't adopted.
pub async fn notify_new_plan_warnings(
    notifier: &Notifier,
    state: &AppState,
    now: DateTime<Utc>,
    adopted: bool,
    prev: Option<&crate::entities::plan::Plan>,
    cur: &crate::entities::plan::Plan,
) {
    if !adopted {
        return;
    }
    for (sev, msg) in new_plan_warnings(prev, cur) {
        notifier.notify(state, now, sev, msg, None, None).await;
    }
}

/// Edge-triggered VTN-reachability producer for the poll loop (kept here for
/// the tasks/ file-size cap). Returns `now_ok` for the caller to store back.
pub async fn notify_outage_edge(
    notifier: &Notifier,
    state: &AppState,
    now: DateTime<Utc>,
    was_ok: bool,
    now_ok: bool,
) -> bool {
    if let Some((sev, msg)) = outage_transition(was_ok, now_ok) {
        notifier.notify(state, now, sev, msg, None, None).await;
    }
    now_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::mock_history_port::MockHistoryPort;
    use crate::state::NOTIFICATION_RING_CAP;

    fn ts(secs: i64) -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    #[tokio::test]
    async fn test_notify_pushes_ring_broadcast_and_persists() {
        let state = AppState::new();
        let mock = Arc::new(MockHistoryPort::new());
        let notifier = Notifier::new(Some(mock.clone()));
        let mut rx = notifier.subscribe();

        let n = notifier
            .notify(
                &state,
                ts(0),
                UserNotificationSeverity::Alert,
                "grid emergency",
                None,
                Some("evt-1".into()),
            )
            .await;

        let ring = state.notifications_since(None).await;
        assert_eq!(ring, vec![n.clone()], "ring holds the notification");
        assert_eq!(rx.try_recv().unwrap(), n, "broadcast delivered");
        assert_eq!(
            mock.appended_notifications(),
            vec![n],
            "persisted to the history store"
        );
    }

    #[tokio::test]
    async fn test_notify_without_history_still_feeds_ring() {
        let state = AppState::new();
        let notifier = Notifier::new(None);
        notifier
            .notify(
                &state,
                ts(0),
                UserNotificationSeverity::Warn,
                "VTN unreachable",
                None,
                None,
            )
            .await;
        assert_eq!(state.notifications_since(None).await.len(), 1);
    }

    #[tokio::test]
    async fn test_notification_ring_bounded_evicts_oldest() {
        let state = AppState::new();
        let notifier = Notifier::new(None);
        for i in 0..(NOTIFICATION_RING_CAP + 5) {
            notifier
                .notify(
                    &state,
                    ts(i as i64),
                    UserNotificationSeverity::Info,
                    format!("n{i}"),
                    None,
                    None,
                )
                .await;
        }
        let ring = state.notifications_since(None).await;
        assert_eq!(ring.len(), NOTIFICATION_RING_CAP);
        assert_eq!(ring.first().unwrap().message, "n5", "oldest evicted");
    }

    fn plan_with_warnings(
        warnings: Vec<(crate::entities::plan::WarningSeverity, &str)>,
    ) -> crate::entities::plan::Plan {
        let mut p: crate::entities::plan::Plan = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "created_at": "2026-01-01T00:00:00Z",
            "trigger": "PERIODIC",
            "horizon": {
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-02T00:00:00Z",
                "step_size_s": 900,
                "num_steps": 96,
                "far_horizon": "2026-01-02T00:00:00Z"
            },
            "slots": [],
            "summary": {
                "total_cost_eur": 0.0,
                "total_co2_g": 0.0,
                "total_import_kwh": 0.0,
                "total_export_kwh": 0.0
            },
            "envelopes": [],
            "warnings": [],
            "objective_eur": 0.0,
            "friction_eur": 0.0
        }))
        .expect("test Plan must deserialize");
        p.warnings = warnings
            .into_iter()
            .map(|(severity, message)| crate::entities::plan::PlanWarning {
                severity,
                message: message.to_string(),
                suggested_action: None,
            })
            .collect();
        p
    }

    #[test]
    fn test_new_plan_warnings_reports_only_new_non_info() {
        use crate::entities::plan::WarningSeverity as W;
        let prev = plan_with_warnings(vec![(W::Warning, "EV behind schedule")]);
        let cur = plan_with_warnings(vec![
            (W::Warning, "EV behind schedule"), // carried over → not re-notified
            (W::Critical, "MILP solver failed"),
            (W::Info, "plan updated"), // Info → never surfaced
        ]);
        let out = new_plan_warnings(Some(&prev), &cur);
        assert_eq!(
            out,
            vec![(
                UserNotificationSeverity::Alert,
                "MILP solver failed".to_string()
            )]
        );
    }

    #[test]
    fn test_new_plan_warnings_without_prev_reports_all_non_info() {
        use crate::entities::plan::WarningSeverity as W;
        let cur = plan_with_warnings(vec![(W::Warning, "deadline at risk")]);
        let out = new_plan_warnings(None, &cur);
        assert_eq!(
            out,
            vec![(
                UserNotificationSeverity::Warn,
                "deadline at risk".to_string()
            )]
        );
    }

    #[test]
    fn test_outage_transition_fires_on_edges_only() {
        assert!(matches!(
            outage_transition(true, false),
            Some((UserNotificationSeverity::Warn, _))
        ));
        assert!(matches!(
            outage_transition(false, true),
            Some((UserNotificationSeverity::Info, _))
        ));
        assert_eq!(outage_transition(true, true), None);
        assert_eq!(outage_transition(false, false), None);
    }

    #[tokio::test]
    async fn test_notifications_since_filters_by_created_at() {
        let state = AppState::new();
        let notifier = Notifier::new(None);
        notifier
            .notify(
                &state,
                ts(0),
                UserNotificationSeverity::Info,
                "old",
                None,
                None,
            )
            .await;
        notifier
            .notify(
                &state,
                ts(60),
                UserNotificationSeverity::Info,
                "new",
                None,
                None,
            )
            .await;
        let filtered = state.notifications_since(Some(ts(30))).await;
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].message, "new");
    }
}
