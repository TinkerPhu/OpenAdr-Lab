// Deviation-arbiter tick-loop glue, split out of helpers.rs to keep both
// files under the tasks/ file-size cap. See `controller::arbiter`'s module
// doc for the overall design (`openspec/changes/deviation-arbiter/`).

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::controller;
use crate::entities::device_session::EvSession;

/// PHASE 3.5 (post-lock): accumulate this tick's arbiter-absorbed kWh into
/// the per-asset residual tracker and, on a capacity-fraction breach past
/// cooldown, send `PlanTrigger::ResidualThreshold` (§5.5). Deliberately
/// accumulator/cooldown gated, never a raw-per-tick-deviation trigger — see
/// `arbiter.rs`'s module doc for why (§1's feature-017 postmortem).
pub(crate) async fn apply_residual_escalation(
    state: &crate::state::AppState,
    trigger_tx: &std::sync::Arc<
        tokio::sync::watch::Sender<crate::entities::asset::PlanTriggerSignal>,
    >,
    residual_kwh_by_asset: &HashMap<String, f64>,
    now: DateTime<Utc>,
) {
    if residual_kwh_by_asset.is_empty() {
        return;
    }
    for (asset_id, kwh) in residual_kwh_by_asset {
        state.accumulate_residual(asset_id, *kwh).await;
    }
    let residuals = state.residual_state().await;
    let breach = residuals
        .values()
        .any(|r| r.breach_fraction() >= controller::arbiter::RESIDUAL_THRESHOLD_FRACTION);
    if !breach {
        return;
    }
    let cooldown_elapsed = state
        .last_residual_trigger_at()
        .await
        .is_none_or(|last| (now - last).num_seconds() >= controller::arbiter::RESIDUAL_COOLDOWN_S);
    if cooldown_elapsed {
        state.set_last_residual_trigger_at(now).await;
        let _ = trigger_tx.send(crate::entities::asset::PlanTriggerSignal::bare(
            crate::entities::asset::PlanTrigger::ResidualThreshold,
        ));
    }
}

/// PHASE 3.5 (post-lock): update both passes' preemption-margin hysteresis
/// state (§4a.1; the limit pass's also drives its release hysteresis), record
/// this tick's arbiter reasoning for `GET /arbiter-diagnostics`
/// (ui-transparency), and emit a BL-37 edge-triggered notification when the
/// deviation pass's active-lever state transitions — all in one call.
///
/// Each pass's decision changes also go to the controller event log
/// (`ControllerEvent::ArbiterDecision`, `GET /trace/events`).
pub(crate) async fn record_arbiter_outcome(
    state: &crate::state::AppState,
    notifier: &crate::services::notify::Notifier,
    outcome: &controller::arbiter::ArbiterOutcome,
    measured_net_kw: Option<f64>,
    now: DateTime<Utc>,
) {
    use controller::arbiter::decision_event;
    let active_lever = outcome.active_lever.clone();
    let limit_lever = outcome.limit.as_ref().and_then(|l| l.active_lever.clone());
    state.set_limit_active_lever(limit_lever).await;
    let prev = state.arbiter_diagnostics().await;
    let prev_limit = prev.limit.as_ref();
    let limit = outcome.limit.as_ref();
    let events = [
        decision_event(
            "deviation",
            (prev.active_lever.as_deref(), prev.unresolved_kw),
            (outcome.active_lever.as_deref(), outcome.unresolved_kw),
            outcome.net_kw.zip(outcome.dev_kw).map(|(n, d)| n - d),
            outcome.dev_kw,
            now,
        ),
        decision_event(
            "limit",
            prev_limit.map_or((None, 0.0), |l| {
                (l.active_lever.as_deref(), l.unresolved_kw)
            }),
            limit.map_or((None, 0.0), |l| {
                (l.active_lever.as_deref(), l.unresolved_kw)
            }),
            limit.map(|l| l.target_kw),
            limit.map(|l| l.excess_kw),
            now,
        ),
    ];
    for event in events.into_iter().flatten() {
        state.push_controller_event(event).await;
    }
    // Read the previous tick's value before it's overwritten below — the
    // prev/current pair needed for edge detection (design.md D2).
    let prev_active_lever = state.arbiter_active_lever().await;
    crate::services::notify::notify_correction_edge(
        notifier,
        state,
        now,
        prev_active_lever.as_deref(),
        active_lever.as_deref(),
    )
    .await;
    state
        .set_arbiter_diagnostics(crate::state::ArbiterDiagnostics {
            net_kw: outcome.net_kw,
            dev_kw: outcome.dev_kw,
            dev_without_correction_kw: outcome.dev_without_correction_kw,
            active_lever: active_lever.clone(),
            unresolved_kw: outcome.unresolved_kw,
            measured_net_kw,
            limit: outcome.limit.clone(),
            bridge_kw_by_asset: both_passes_bridge_kw(outcome),
            updated_at: Some(now),
        })
        .await;
    state.set_arbiter_active_lever(active_lever).await;
}

/// Each EV's overlay switch this tick, by asset id. An EV with neither a settings record
/// nor a session reads the default settings with no session running.
#[derive(Debug, Clone, Default)]
pub(crate) struct OverlayEnabled(HashMap<String, bool>);

impl OverlayEnabled {
    #[cfg(test)]
    pub(crate) fn from_map(overlay_by_asset: HashMap<String, bool>) -> Self {
        Self(overlay_by_asset)
    }

    pub(crate) fn of(&self, asset_id: &str) -> bool {
        self.0
            .get(asset_id)
            .copied()
            .unwrap_or_else(|| crate::state::EvSettings::default().overlay_enabled(false))
    }
}

/// PHASE 0, per EV: user toggle AND no EvSession of that EV whose window is open now
/// (`EvSettings::overlay_enabled`), plus the session governing each EV now. Also updates
/// each EV's derived `paused_by_active_session` flag when it goes stale. A session past its
/// `departure_time` is not current, so it cannot pause opportunistic charging; removing it
/// is `AppState::settle_requests`'s job.
pub(crate) async fn resolve_overlay_enabled(
    state: &crate::state::AppState,
    now: DateTime<Utc>,
) -> (OverlayEnabled, HashMap<String, EvSession>) {
    // "Active" means a session's window is open *now* - deliberately not "the queue
    // is non-empty". Under the rolling simulated schedule the queue is almost never
    // empty, so the old emptiness test would pause opportunistic charging forever,
    // including in the gaps between sessions when the car is sitting at home and
    // free to soak up surplus.
    let settings = state.ev_settings_all().await;
    let sessions_now: HashMap<String, EvSession> = state
        .ev_sessions()
        .await
        .into_iter()
        .filter_map(|(id, queue)| queue.current(now).cloned().map(|s| (id, s)))
        .collect();
    let mut ids: Vec<&String> = settings.keys().chain(sessions_now.keys()).collect();
    ids.sort();
    ids.dedup();
    let mut enabled = HashMap::new();
    for ev_id in ids {
        let ev_settings = settings.get(ev_id).cloned().unwrap_or_default();
        let session_active = sessions_now.contains_key(ev_id);
        if ev_settings.paused_by_active_session != session_active {
            let paused = crate::state::EvSettings {
                paused_by_active_session: session_active,
                ..ev_settings.clone()
            };
            state.set_ev_settings(ev_id, paused).await;
        }
        enabled.insert(ev_id.clone(), ev_settings.overlay_enabled(session_active));
    }
    (OverlayEnabled(enabled), sessions_now)
}

/// Both passes' storage bridges this tick, summed per asset — next tick's
/// `ArbiterTick::prev_bridge_kw_by_asset`.
pub(super) fn both_passes_bridge_kw(
    outcome: &controller::arbiter::ArbiterOutcome,
) -> HashMap<String, f64> {
    let limit_bridges = outcome.limit.iter().flat_map(|l| &l.bridge_kw_by_asset);
    let mut bridge_kw_by_asset = HashMap::new();
    for (asset_id, kw) in outcome.bridge_kw_by_asset.iter().chain(limit_bridges) {
        *bridge_kw_by_asset.entry(asset_id.clone()).or_insert(0.0) += kw;
    }
    bridge_kw_by_asset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    fn ts(secs: i64) -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    /// BL-37 (task 3.4): `None -> Some -> Some(different lever) -> None`
    /// across four ticks must produce exactly two notifications (one active,
    /// one cleared) — a lever handoff mid-correction is not an edge.
    #[tokio::test]
    async fn record_arbiter_outcome_lever_handoff_sequence_emits_exactly_two_notifications() {
        let state = AppState::new();
        let notifier = crate::services::notify::Notifier::new(None);

        let outcome = |lever: Option<&'static str>| controller::arbiter::ArbiterOutcome {
            active_lever: lever.map(str::to_string),
            net_kw: lever.map(|_| 1.0),
            dev_kw: lever.map(|_| 0.5),
            ..Default::default()
        };
        record_arbiter_outcome(&state, &notifier, &outcome(None), None, ts(0)).await;
        record_arbiter_outcome(&state, &notifier, &outcome(Some("battery")), None, ts(1)).await;
        record_arbiter_outcome(
            &state,
            &notifier,
            &outcome(Some("heater_pause")),
            None,
            ts(2),
        )
        .await;
        record_arbiter_outcome(&state, &notifier, &outcome(None), None, ts(3)).await;

        let ring = state.notifications_since(None).await;
        assert_eq!(ring.len(), 2, "exactly one active + one cleared");
        assert_eq!(
            ring[0].dedup_key.as_deref(),
            Some("arbiter-correction-active")
        );
        assert_eq!(
            ring[1].dedup_key.as_deref(),
            Some("arbiter-correction-cleared")
        );
    }

    /// GB-47: the event log records the limit pass's decisions, not ticks —
    /// engage, hold for two ticks, release = two `ArbiterDecision` events.
    #[tokio::test]
    async fn record_arbiter_outcome_logs_limit_decisions_on_change_only() {
        use controller::arbiter::limit::LimitPassOutcome;
        let state = AppState::new();
        let notifier = crate::services::notify::Notifier::new(None);
        let limit = |lever: Option<&'static str>| controller::arbiter::ArbiterOutcome {
            limit: Some(LimitPassOutcome {
                target_kw: 0.9,
                excess_kw: 1.4,
                active_lever: lever.map(str::to_string),
                ..Default::default()
            }),
            ..Default::default()
        };
        let levers = [
            None,
            Some("battery"),
            Some("battery"),
            Some("battery"),
            None,
        ];
        for (i, lever) in levers.into_iter().enumerate() {
            record_arbiter_outcome(&state, &notifier, &limit(lever), Some(1.0), ts(i as i64)).await;
        }
        let decisions = state
            .controller_trace()
            .await
            .events()
            .into_iter()
            .filter(|e| {
                matches!(
                    e,
                    controller::trace::ControllerEvent::ArbiterDecision { pass, .. } if pass == "limit"
                )
            })
            .count();
        assert_eq!(decisions, 2);
        assert_eq!(state.limit_active_lever().await, None);
    }

    /// A session open from well before `ts(0)` until `departure_time`, so the tests
    /// below vary only the thing they are about: whether that departure has passed.
    fn make_ev_session(
        departure_time: DateTime<Utc>,
    ) -> crate::entities::device_session::EvSession {
        crate::entities::device_session::EvSession {
            asset_id: "ev".into(),
            id: uuid::Uuid::new_v4(),
            target_soc_frac: 0.8,
            window_start: ts(-1000),
            expected_trip_distance_km: None,
            expected_return_time: None,
            departure_time,
            soft_deadline: false,
            mode: Default::default(),
            origin: crate::entities::device_session::EvSessionOrigin::UserRequest,
            budget_eur: None,
            comfort_rates: vec![],
            created_at: ts(0),
            updated_at: ts(0),
        }
    }

    /// A session with `departure_time` still ahead of `now` must keep pausing
    /// opportunistic charging and must not be cleared from state.
    #[tokio::test]
    async fn resolve_overlay_enabled_keeps_a_not_yet_expired_session() {
        let state = AppState::new();
        state
            .insert_ev_session(make_ev_session(ts(100)))
            .await
            .unwrap();

        let enabled = resolve_overlay_enabled(&state, ts(0)).await.0.of("ev");

        assert!(!enabled, "a live session must still suppress the overlay");
        assert!(state.current_ev_session("ev", ts(0)).await.is_some());
        assert!(state.ev_settings("ev").await.paused_by_active_session);
    }

    /// The trap the queue would otherwise spring: with a rolling schedule the queue
    /// is almost never empty, so "a session exists" would pause opportunistic
    /// charging forever. What matters is whether a window is open *now*.
    #[tokio::test]
    async fn resolve_overlay_enabled_does_not_pause_in_the_gap_between_sessions() {
        let state = AppState::new();
        let mut later = make_ev_session(ts(1000));
        later.window_start = ts(500);
        state.insert_ev_session(later).await.unwrap();

        let enabled = resolve_overlay_enabled(&state, ts(0)).await.0.of("ev");

        assert!(
            enabled,
            "a session queued for later must not suppress the overlay now"
        );
        assert!(!state.ev_settings("ev").await.paused_by_active_session);
        assert_eq!(
            state.ev_sessions().await.len(),
            1,
            "and it must still be queued - not current is not the same as expired"
        );
    }

    /// A session whose `departure_time` has already passed is never expired
    /// by anything else (only an explicit cancel) — it
    /// must be cleared here so it stops permanently pausing opportunistic
    /// charging and stops hiding the EV from the headroom forecast.
    #[tokio::test]
    async fn resolve_overlay_enabled_is_not_paused_by_an_expired_session() {
        let state = AppState::new();
        state
            .insert_ev_session(make_ev_session(ts(-1)))
            .await
            .unwrap();
        state
            .set_ev_settings(
                "ev",
                crate::state::EvSettings {
                    opportunistic_charging_enabled: true,
                    paused_by_active_session: true,
                },
            )
            .await;

        let enabled = resolve_overlay_enabled(&state, ts(0)).await.0.of("ev");

        assert!(
            enabled,
            "an expired session must no longer suppress the overlay"
        );
        assert!(!state.ev_settings("ev").await.paused_by_active_session);
    }

    /// W5a: each EV's overlay follows its own settings and its own session; an EV with
    /// neither reads the default (on).
    #[tokio::test]
    async fn resolve_overlay_enabled_is_per_ev() {
        let state = AppState::new();
        let off = crate::state::EvSettings {
            opportunistic_charging_enabled: false,
            paused_by_active_session: false,
        };
        state.set_ev_settings("ev-a", off).await;
        let mut session = make_ev_session(ts(100));
        session.asset_id = "ev-b".into();
        state.insert_ev_session(session).await.unwrap();

        let (enabled, sessions_now) = resolve_overlay_enabled(&state, ts(0)).await;

        assert!(!enabled.of("ev-a"), "switched off by its user");
        assert!(!enabled.of("ev-b"), "paused by its own session");
        assert!(enabled.of("ev-c"), "an EV with no record reads the default");
        assert_eq!(sessions_now.keys().collect::<Vec<_>>(), vec!["ev-b"]);
        assert!(state.ev_settings("ev-b").await.paused_by_active_session);
        assert!(!state.ev_settings("ev-a").await.paused_by_active_session);
    }
}
