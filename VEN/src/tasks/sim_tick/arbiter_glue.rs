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
