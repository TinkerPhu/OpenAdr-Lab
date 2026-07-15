//! WP5.2 (BL-14) — daily background job: learn per-asset heuristics from
//! history and store them in `AppState`. The learning algorithm lives in
//! `services::heuristics` (application ring); this is just the scheduling
//! glue, mirroring `tasks/history_sampler`'s shape.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::warn;

use crate::controller::residual::SITE_RESIDUAL_ASSET_ID;
use crate::controller::HistoryPort;
use crate::services::heuristics::{learn_asset_heuristics, HeuristicsConfig};
use crate::state::AppState;

/// Currently heuristic-eligible asset_ids (roadmap: "site-residual, base
/// load, PV-without-weather"; PV forecasting is WP5.3's job).
const HEURISTIC_ASSET_IDS: [&str; 2] = ["base_load", SITE_RESIDUAL_ASSET_ID];

/// Returns `true` (and records `now`'s UTC calendar day) exactly the first
/// time this is called for a given day — mirrors `history_sampler`'s
/// `day_boundary_crossed`. Fires on the first call too, so a freshly
/// preloaded history gets a heuristic computed on the next check rather
/// than waiting a full day.
fn day_boundary_crossed(last_run_day: &mut Option<i64>, now: DateTime<Utc>) -> bool {
    let day = now.timestamp().div_euclid(86_400);
    if *last_run_day == Some(day) {
        false
    } else {
        *last_run_day = Some(day);
        true
    }
}

/// Run the aggregation once for every heuristic-eligible asset, storing
/// each non-`None` result. Log-and-continue on failure — never blocks or
/// crashes the control loop.
pub(crate) async fn run_heuristics_once(
    history: Arc<dyn HistoryPort>,
    state: &AppState,
    now: DateTime<Utc>,
) {
    for asset_id in HEURISTIC_ASSET_IDS {
        let history = history.clone();
        let id = asset_id.to_string();
        let result = tokio::task::spawn_blocking(move || {
            learn_asset_heuristics(history.as_ref(), &id, now, &HeuristicsConfig::default())
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
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_run_day: Option<i64> = None;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            let now = Utc::now();
            if day_boundary_crossed(&mut last_run_day, now) {
                run_heuristics_once(history.clone(), &state, now).await;
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

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn test_day_boundary_crossed_first_call_is_true() {
        let mut last = None;
        assert!(day_boundary_crossed(&mut last, ts(0)));
        assert_eq!(last, Some(0));
    }

    #[test]
    fn test_day_boundary_crossed_same_day_is_false() {
        let mut last = None;
        day_boundary_crossed(&mut last, ts(0));
        assert!(!day_boundary_crossed(&mut last, ts(86_399)));
    }

    #[test]
    fn test_day_boundary_crossed_next_day_is_true() {
        let mut last = None;
        day_boundary_crossed(&mut last, ts(0));
        assert!(day_boundary_crossed(&mut last, ts(86_400)));
    }

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
        run_heuristics_once(history, &state, now).await;

        let all = state.asset_heuristics().await;
        let base_load_heuristics = all
            .get("base_load")
            .expect("base_load heuristics must be stored after a successful run");
        assert!(
            base_load_heuristics.daytime_profile_kw[0][8]
                > base_load_heuristics.daytime_profile_kw[0][3],
            "coffee hour should exceed quiet hour in the stored heuristic"
        );
        // site-residual has no seeded history in this test — cold-start, correctly absent.
        assert!(!all.contains_key(SITE_RESIDUAL_ASSET_ID));
    }
}
