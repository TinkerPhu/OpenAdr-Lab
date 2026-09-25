// Deviation-arbiter tick-loop glue, split out of helpers.rs to keep both
// files under the tasks/ file-size cap. See `controller::arbiter`'s module
// doc for the overall design (`openspec/changes/deviation-arbiter/`).

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::controller;
use crate::entities::sim_inject::SimInjectState;

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
    absorbed_kwh_by_asset: &HashMap<String, f64>,
    now: DateTime<Utc>,
) {
    if absorbed_kwh_by_asset.is_empty() {
        return;
    }
    for (asset_id, kwh) in absorbed_kwh_by_asset {
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
    let active_lever = outcome.active_lever.map(str::to_string);
    let limit_lever = outcome.limit.as_ref().and_then(|l| l.active_lever);
    state
        .set_limit_active_lever(limit_lever.map(str::to_string))
        .await;
    let prev = state.arbiter_diagnostics().await;
    let prev_limit = prev.limit.as_ref();
    let limit = outcome.limit.as_ref();
    let events = [
        decision_event(
            "deviation",
            (prev.active_lever.as_deref(), prev.unresolved_kw),
            (outcome.active_lever, outcome.unresolved_kw),
            outcome.net_kw.zip(outcome.dev_kw).map(|(n, d)| n - d),
            outcome.dev_kw,
            now,
        ),
        decision_event(
            "limit",
            prev_limit.map_or((None, 0.0), |l| (l.active_lever, l.unresolved_kw)),
            limit.map_or((None, 0.0), |l| (l.active_lever, l.unresolved_kw)),
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
            active_lever: active_lever.clone(),
            unresolved_kw: outcome.unresolved_kw,
            measured_net_kw,
            limit: outcome.limit.clone(),
            updated_at: Some(now),
        })
        .await;
    state.set_arbiter_active_lever(active_lever).await;
}

/// PHASE 0: user toggle AND no active (non-expired) EvSession. Also updates
/// the derived `paused_by_active_session` flag on `EvSettings` when it goes
/// stale, and clears an EvSession once its `departure_time` has passed —
/// nothing else ever expires a session (only explicit cancel or the VTN
/// signal disappearing), so a finished/missed session would otherwise pause
/// opportunistic charging and hide the EV from the headroom forecast forever.
pub(crate) async fn resolve_overlay_enabled(
    state: &crate::state::AppState,
    now: DateTime<Utc>,
) -> bool {
    let ev_sess_tick = state.ev_session().await;
    if ev_sess_tick
        .as_ref()
        .is_some_and(|s| s.departure_time <= now)
    {
        state.set_ev_session(None).await;
    }
    let ev_sess_tick = state.ev_session().await;
    let ev_settings_tick = state.ev_settings().await;
    let session_active = ev_sess_tick.is_some();
    if ev_settings_tick.paused_by_active_session != session_active {
        state
            .set_ev_settings(crate::state::EvSettings {
                paused_by_active_session: session_active,
                ..ev_settings_tick.clone()
            })
            .await;
    }
    ev_settings_tick.opportunistic_charging_enabled && !session_active
}

/// Manual sim-inject heater overrides win over the arbiter's decision
/// (mirrors the existing "manual override wins" precedent for PV smoothing)
/// — only fall back to the arbiter's mode when neither inject field is
/// explicitly set.
pub(crate) fn resolve_heater_emergency_mode(
    inject: &SimInjectState,
    arbiter_mode: Option<(bool, bool)>,
) -> (Option<bool>, Option<bool>) {
    if inject.heater_emergency_curtail.is_some() || inject.heater_emergency_absorb.is_some() {
        (
            inject.heater_emergency_curtail,
            inject.heater_emergency_absorb,
        )
    } else if let Some((curtail, absorb)) = arbiter_mode {
        (Some(curtail), Some(absorb))
    } else {
        (None, None)
    }
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
            active_lever: lever,
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
                active_lever: lever,
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

    fn make_ev_session(
        departure_time: DateTime<Utc>,
    ) -> crate::entities::device_session::EvSession {
        crate::entities::device_session::EvSession {
            id: uuid::Uuid::new_v4(),
            target_soc: 0.8,
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
        state.set_ev_session(Some(make_ev_session(ts(100)))).await;

        let enabled = resolve_overlay_enabled(&state, ts(0)).await;

        assert!(!enabled, "a live session must still suppress the overlay");
        assert!(state.ev_session().await.is_some());
        assert!(state.ev_settings().await.paused_by_active_session);
    }

    /// A session whose `departure_time` has already passed is never expired
    /// by anything else (only explicit cancel or a vanished VTN signal) — it
    /// must be cleared here so it stops permanently pausing opportunistic
    /// charging and stops hiding the EV from the headroom forecast.
    #[tokio::test]
    async fn resolve_overlay_enabled_clears_an_expired_session() {
        let state = AppState::new();
        state.set_ev_session(Some(make_ev_session(ts(-1)))).await;
        state
            .set_ev_settings(crate::state::EvSettings {
                opportunistic_charging_enabled: true,
                paused_by_active_session: true,
            })
            .await;

        let enabled = resolve_overlay_enabled(&state, ts(0)).await;

        assert!(
            enabled,
            "an expired session must no longer suppress the overlay"
        );
        assert!(
            state.ev_session().await.is_none(),
            "expired session must be cleared from state"
        );
        assert!(!state.ev_settings().await.paused_by_active_session);
    }
}
