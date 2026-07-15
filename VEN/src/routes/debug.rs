//! WP5.2 (BL-14) — on-demand debug/dev-tool routes. Currently just the
//! heuristics preload; a natural home for future test/dev-only actions
//! rather than growing `routes/sim.rs` (physics injection) or `system.rs`
//! (health/metrics) with unrelated concerns.
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::assets::{AssetConfig, BaseLoad};
use crate::controller::residual::SITE_RESIDUAL_ASSET_ID;
use crate::controller::HistoryPort;
use crate::services::forecast::build_heuristic_forecasts;
use crate::services::heuristics::{
    base_load_power_kw_at, generate_synthetic_backfill, learn_asset_heuristics, HeuristicsConfig,
};
use crate::state::AppState;
use crate::AppCtx;

const PRELOAD_ASSET_IDS: [&str; 2] = ["base_load", SITE_RESIDUAL_ASSET_ID];
const PRELOAD_WINDOW_DAYS: i64 = 28;

#[derive(Debug, Serialize, PartialEq)]
pub struct PreloadedAssetSummary {
    pub asset_id: String,
    pub samples_seeded: usize,
    /// `[0]` = weekday (Mon-Fri) profile, `[1]` = weekend (Sat/Sun) profile.
    pub daytime_profile_kw: [Vec<f64>; 2],
    pub seasonal_factor: f64,
}

/// Seed `PRELOAD_WINDOW_DAYS` of backdated synthetic history for every
/// preload-eligible asset (using `base_load`'s configured appliance-noise
/// model) and run the real WP5.2 aggregation over it, storing results in
/// `state` and merging them into `state`'s forecasts immediately. Returns
/// one summary per asset that cleared the cold-start gate.
pub(crate) async fn preload_heuristics(
    history: Arc<dyn HistoryPort>,
    base_load: BaseLoad,
    state: &AppState,
    now: DateTime<Utc>,
) -> Result<Vec<PreloadedAssetSummary>, crate::entities::DomainError> {
    let from = now - Duration::days(PRELOAD_WINDOW_DAYS);
    let mut summaries = Vec::new();

    for asset_id in PRELOAD_ASSET_IDS {
        // site-residual has no independent meter-noise source in the
        // simulator (R-20) — its synthetic backfill is honestly flat 0,
        // not a reuse of base_load's appliance-noise formula.
        let rows = if asset_id == SITE_RESIDUAL_ASSET_ID {
            generate_synthetic_backfill(asset_id, from, now, |_| 0.0)
        } else {
            generate_synthetic_backfill(asset_id, from, now, base_load_power_kw_at(&base_load))
        };
        let n_rows = rows.len();
        let h = history.clone();
        tokio::task::spawn_blocking(move || h.append_tick_samples(&rows))
            .await
            .expect("preload write task must not panic")?;

        let h = history.clone();
        let id = asset_id.to_string();
        let learned = tokio::task::spawn_blocking(move || {
            learn_asset_heuristics(h.as_ref(), &id, now, &HeuristicsConfig::default())
        })
        .await
        .expect("preload learn task must not panic")?;

        if let Some(heuristics) = learned {
            let mut all = state.asset_heuristics().await;
            all.insert(asset_id.to_string(), heuristics.clone());
            state.set_asset_heuristics(all).await;

            summaries.push(PreloadedAssetSummary {
                asset_id: asset_id.to_string(),
                samples_seeded: n_rows,
                daytime_profile_kw: heuristics.daytime_profile_kw,
                seasonal_factor: heuristics.seasonal_factor,
            });
        }
    }

    // Merge immediately so GET /forecast reflects the preload without
    // waiting for the next plan cycle — better feedback for a deliberately
    // triggered demo action.
    let heuristics_map = state.asset_heuristics().await;
    if !heuristics_map.is_empty() {
        let mut forecasts = state.asset_forecasts().await;
        let existing_ids: std::collections::HashSet<String> =
            forecasts.iter().map(|f| f.asset_id.clone()).collect();
        // No plan cycle is guaranteed to have run — sample a flat 24h/30-min
        // horizon purely for this immediate-feedback merge; the next real
        // plan cycle recomputes it properly against the adopted plan's slots.
        let slot_starts: Vec<DateTime<Utc>> =
            (0..48).map(|i| now + Duration::minutes(i * 30)).collect();
        for hf in build_heuristic_forecasts(&heuristics_map, &slot_starts, now) {
            if !existing_ids.contains(&hf.asset_id) {
                forecasts.push(hf);
            }
        }
        state.set_asset_forecasts(forecasts).await;
    }

    Ok(summaries)
}

/// `POST /debug/heuristics/preload` — on-demand only, never auto-run at
/// startup, so repeated restarts can't silently re-seed/duplicate synthetic
/// history over real accumulated data.
pub async fn post_heuristics_preload(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let Some(history) = ctx.history.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "history store is not enabled for this VEN" })),
        )
            .into_response();
    };

    let base_load = {
        let sim = ctx.sim.lock().await;
        sim.find_asset(crate::ids::ASSET_BASE_LOAD)
            .and_then(|(_, cfg)| match cfg {
                AssetConfig::BaseLoad(bl) => Some(bl.clone()),
                _ => None,
            })
    };
    let Some(base_load) = base_load else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "no base_load asset configured" })),
        )
            .into_response();
    };

    match preload_heuristics(history, base_load, &ctx.state, Utc::now()).await {
        Ok(summaries) => (
            StatusCode::OK,
            Json(serde_json::json!({ "preloaded": summaries })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{ApplianceSpikeParams, BaseLoadParams};
    use crate::services::test_support::mock_history_port::MockHistoryPort;

    fn base_load_with_coffee() -> BaseLoad {
        BaseLoad::from_params(&BaseLoadParams {
            baseline_kw: 0.3,
            spikes: vec![ApplianceSpikeParams {
                center_hour: 8.0,
                jitter_h: 0.05,
                amplitude_kw: 1.2,
                duration_h: 0.25,
                ramp_h: 0.03,
                probability: 1.0,
                weekdays: vec![],
            }],
            ..BaseLoadParams::default()
        })
    }

    #[tokio::test]
    async fn preload_heuristics_seeds_history_and_stores_non_flat_heuristic() {
        let history: Arc<dyn HistoryPort> = Arc::new(MockHistoryPort::new());
        let state = AppState::new();
        let now = Utc::now();

        let summaries = preload_heuristics(history, base_load_with_coffee(), &state, now)
            .await
            .expect("preload must succeed");

        let base_load_summary = summaries
            .iter()
            .find(|s| s.asset_id == "base_load")
            .expect("base_load must clear the cold-start gate with 28 days of 1-min samples");
        assert!(base_load_summary.samples_seeded > 0);
        assert!(
            base_load_summary.daytime_profile_kw[0][8] > base_load_summary.daytime_profile_kw[0][3],
            "learned profile should peak near the configured coffee hour"
        );

        let stored = state.asset_heuristics().await;
        assert!(stored.contains_key("base_load"));

        // GET /forecast reflects the preload immediately, without a plan cycle.
        let forecasts = state.asset_forecasts().await;
        let heuristic_forecast = forecasts
            .iter()
            .find(|f| f.asset_id == "base_load")
            .expect("base_load forecast must be present after preload");
        assert_eq!(
            heuristic_forecast.source,
            crate::entities::design_vocabulary::ForecastSource::Heuristic
        );
    }

    #[tokio::test]
    async fn preload_heuristics_site_residual_stays_flat_by_design() {
        // R-20: the simulator has no independent meter-noise source, so
        // site-residual's synthetic backfill is flat 0 — its learned
        // heuristic should correctly reflect that, not error out.
        let history: Arc<dyn HistoryPort> = Arc::new(MockHistoryPort::new());
        let state = AppState::new();
        let now = Utc::now();

        let summaries = preload_heuristics(history, base_load_with_coffee(), &state, now)
            .await
            .expect("preload must succeed");

        let residual_summary = summaries
            .iter()
            .find(|s| s.asset_id == SITE_RESIDUAL_ASSET_ID)
            .expect("site-residual must clear the cold-start gate too");
        assert!(
            residual_summary
                .daytime_profile_kw
                .iter()
                .all(|bucket| bucket.iter().all(|&kw| kw.abs() < 1e-9)),
            "site-residual's synthetic backfill is always 0 (R-20) — the learned \
             profile must correctly reflect that, not show a fake pattern"
        );
    }
}
