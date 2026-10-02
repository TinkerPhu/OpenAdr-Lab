//! WP5.2 (BL-14) — daily background job: learn per-asset heuristics from
//! history and store them in `AppState`. The learning algorithm lives in
//! `services::heuristics` (application ring); this is just the scheduling
//! glue, mirroring `tasks/history_sampler`'s shape.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::warn;

use crate::controller::HistoryPort;
use crate::services::heuristics::{learn_asset_heuristics, HeuristicsConfig, HEURISTIC_ASSET_IDS};
use crate::state::AppState;
use crate::tasks::daily_gate::DailyGate;

/// Run the aggregation once for every heuristic-eligible asset, storing
/// each non-`None` result. Log-and-continue on failure — never blocks or
/// crashes the control loop.
pub(crate) async fn run_heuristics_once(
    history: Arc<dyn HistoryPort>,
    state: &AppState,
    now: DateTime<Utc>,
    cfg: &HeuristicsConfig,
) {
    for asset_id in HEURISTIC_ASSET_IDS {
        let history = history.clone();
        let id = asset_id.to_string();
        // R-60: fetch this asset's previously-learned heuristic (if any)
        // before it gets overwritten below, so `learn_asset_heuristics` can
        // compute how far off its own past predictions were.
        let previous = state.asset_heuristics().await.get(asset_id).cloned();
        let cfg = *cfg;
        let result = tokio::task::spawn_blocking(move || {
            learn_asset_heuristics(history.as_ref(), &id, now, &cfg, previous.as_ref())
        })
        .await;
        match result {
            Ok(Ok(Some(heuristics))) => {
                let mut all = state.asset_heuristics().await;
                all.insert(asset_id.to_string(), heuristics);
                state.set_asset_heuristics(all).await;
            }
            Ok(Ok(None)) => {} // cold-start: not enough history yet
            Ok(Err(e)) => warn!("heuristics job failed for {asset_id}: {e}"),
            Err(e) => warn!("heuristics job task panicked for {asset_id}: {e}"),
        }
    }
}

pub(crate) fn spawn_heuristics_job(
    history: Arc<dyn HistoryPort>,
    state: AppState,
    heuristics_config: HeuristicsConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut daily = DailyGate::default();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            let now = Utc::now();
            if daily.crossed(now) {
                run_heuristics_once(history.clone(), &state, now, &heuristics_config).await;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::base_load::BaseLoad;
    use crate::entities::asset_params::{ApplianceSpikeParams, BaseLoadParams};
    use crate::services::heuristics::generate_synthetic_backfill;
    use crate::services::test_support::mock_history_port::MockHistoryPort;
    use chrono::{Duration, TimeZone};

    // The once-per-day gate is tested once, in `tasks/daily_gate.rs`.

    #[tokio::test]
    async fn run_heuristics_once_stores_non_flat_base_load_profile() {
        let now = Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap();
        let bl = BaseLoad::from_params(&BaseLoadParams {
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
        });
        let rows = generate_synthetic_backfill(
            "base_load",
            now - Duration::days(28),
            now,
            crate::services::heuristics::base_load_power_kw_at(&bl),
        );

        let history: Arc<dyn HistoryPort> = Arc::new(MockHistoryPort::new());
        history.append_tick_samples(&rows).unwrap();

        let state = AppState::new();
        run_heuristics_once(history, &state, now, &HeuristicsConfig::default()).await;

        let all = state.asset_heuristics().await;
        let base_load_heuristics = all
            .get("base_load")
            .expect("base_load heuristics must be stored after a successful run");
        assert!(
            base_load_heuristics.daytime_profile_kw[0][8]
                > base_load_heuristics.daytime_profile_kw[0][3],
            "coffee hour should exceed quiet hour in the stored heuristic"
        );
        // Only heuristic-eligible assets with seeded history get an entry.
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn run_heuristics_once_respects_a_custom_min_samples_threshold() {
        // Seed only 50 ticks — fewer than the default cold-start gate (100),
        // but more than a deliberately lowered custom one (10). Proves `cfg`
        // (not a hardcoded default) actually reaches `learn_asset_heuristics`.
        let now = Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap();
        let bl = BaseLoad::from_params(&BaseLoadParams::default());
        let rows = generate_synthetic_backfill(
            "base_load",
            now - Duration::minutes(50),
            now,
            crate::services::heuristics::base_load_power_kw_at(&bl),
        );
        assert_eq!(rows.len(), 50);

        let history: Arc<dyn HistoryPort> = Arc::new(MockHistoryPort::new());
        history.append_tick_samples(&rows).unwrap();

        let default_state = AppState::new();
        run_heuristics_once(
            history.clone(),
            &default_state,
            now,
            &HeuristicsConfig::default(),
        )
        .await;
        assert!(
            default_state.asset_heuristics().await.is_empty(),
            "default cold-start gate (100) should decline 50 samples"
        );

        let custom_cfg = HeuristicsConfig {
            min_samples_for_confidence: 10,
            ..HeuristicsConfig::default()
        };
        let custom_state = AppState::new();
        run_heuristics_once(history, &custom_state, now, &custom_cfg).await;
        assert!(
            custom_state
                .asset_heuristics()
                .await
                .contains_key("base_load"),
            "custom cfg with a lowered threshold (10) should clear the gate on the same 50 samples"
        );
    }
}
