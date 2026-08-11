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
    trigger_tx: &std::sync::Arc<tokio::sync::watch::Sender<crate::entities::asset::PlanTrigger>>,
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
        let _ = trigger_tx.send(crate::entities::asset::PlanTrigger::ResidualThreshold);
    }
}

/// PHASE 3.5 (post-lock): update the preemption-margin hysteresis state
/// (§4a.1), record this tick's arbiter reasoning for `GET /arbiter-diagnostics`
/// (ui-transparency), and emit a BL-37 edge-triggered notification when the
/// active-lever state transitions — all in one call.
pub(crate) async fn record_arbiter_outcome(
    state: &crate::state::AppState,
    notifier: &crate::services::notify::Notifier,
    (active_lever, net_kw, dev_kw): (Option<String>, Option<f64>, Option<f64>),
    now: DateTime<Utc>,
) {
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
        .set_arbiter_diagnostics(net_kw, dev_kw, active_lever.clone(), now)
        .await;
    state.set_arbiter_active_lever(active_lever).await;
}

/// PHASE 0: user toggle AND no active EvSession. Also updates the derived
/// `paused_by_active_session` flag on `EvSettings` when it goes stale.
pub(crate) async fn resolve_overlay_enabled(state: &crate::state::AppState) -> bool {
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

/// Weather-sourced PV power for this exact instant — same translation
/// (staleness gating, transposition physics, calibration) the planner's own
/// PV input uses (R-50); reused here via the one shared entry point rather
/// than re-derived.
pub(crate) async fn resolve_weather_pv_kw_now(
    weather: &dyn crate::controller::WeatherForecastPort,
    weather_pv_params: Option<&crate::entities::asset_params::PvForecastParams>,
    now: DateTime<Utc>,
) -> Option<f64> {
    weather_pv_params?;
    let forecast = weather.latest().await;
    crate::entities::solar::resolve_weather_pv_kw(
        weather_pv_params,
        forecast.as_ref(),
        now,
        crate::services::planning::WEATHER_STALENESS_THRESHOLD,
        &[now],
    )
    .and_then(|v| v.first().copied())
}

/// Real-measurement MQTT feed value for this exact instant (real-measurement-mqtt).
/// `enabled` is the profile-level gate (`measurements.pv_enabled` /
/// `.base_load_enabled`) — the second gate alongside the port itself only
/// existing when the corresponding env var was set at startup.
async fn resolve_measured_kw_now(
    port: &dyn crate::controller::MeasurementPort,
    enabled: bool,
    now: DateTime<Utc>,
) -> Option<f64> {
    if !enabled {
        return None;
    }
    let latest = port.latest_kw().await;
    crate::entities::measurement::resolve_measured_kw(
        latest,
        now,
        crate::entities::measurement::MEASUREMENT_STALENESS_THRESHOLD,
    )
}

/// Both signals' measured readings for this instant, `(pv, base_load)` —
/// bundles the two `resolve_measured_kw_now` calls into one await site to
/// keep `tick_once` under the tasks/ file-size cap.
pub(crate) async fn resolve_measurements_now(
    pv_port: &dyn crate::controller::MeasurementPort,
    pv_enabled: bool,
    base_load_port: &dyn crate::controller::MeasurementPort,
    base_load_enabled: bool,
    now: DateTime<Utc>,
) -> (Option<f64>, Option<f64>) {
    let pv = resolve_measured_kw_now(pv_port, pv_enabled, now).await;
    let base_load = resolve_measured_kw_now(base_load_port, base_load_enabled, now).await;
    (pv, base_load)
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

        record_arbiter_outcome(&state, &notifier, (None, None, None), ts(0)).await;
        record_arbiter_outcome(
            &state,
            &notifier,
            (Some("battery".to_string()), Some(1.0), Some(0.5)),
            ts(1),
        )
        .await;
        record_arbiter_outcome(
            &state,
            &notifier,
            (Some("heater_pause".to_string()), Some(1.0), Some(0.5)),
            ts(2),
        )
        .await;
        record_arbiter_outcome(&state, &notifier, (None, None, None), ts(3)).await;

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
}
