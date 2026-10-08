//! Hourly refresh of the base load's trailing-window summary (the key features on the
//! Controller's "Flexibility & Forecast" panel). Acquisition only: this reads the recorded
//! 1-minute rows through `HistoryPort` and hands them to the asset, which owns what they
//! mean (`assets::load_window`). Reading the whole window each time is also the seeding:
//! a restart finds the same history and shows the same numbers.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use tokio::sync::Mutex;
use tracing::warn;

use crate::assets::load_window::{LoadWindowStats, LOAD_WINDOW_DAYS};
use crate::controller::HistoryPort;
use crate::ids::ASSET_BASE_LOAD;
use crate::simulator::SimState;

/// The window only needs to move once an hour; precision is not the point.
const REFRESH_INTERVAL_S: u64 = 3600;
/// While the history holds no record yet (fresh install), look again sooner so the
/// first sample does not wait an hour to show.
const RETRY_INTERVAL_S: u64 = 60;

/// Re-read the window ending at `now` and inject it into the base-load asset. Returns the
/// stats found, `None` when the history holds no record (or could not be read). A failed
/// read leaves the previous summary in place. Best-effort: log and continue.
pub(crate) async fn refresh_base_load_window(
    history: Arc<dyn HistoryPort>,
    sim: &Mutex<SimState>,
    now: DateTime<Utc>,
) -> Option<LoadWindowStats> {
    let from = now - Duration::days(LOAD_WINDOW_DAYS);
    let rows =
        tokio::task::spawn_blocking(move || history.query_ticks(from, now, Some(ASSET_BASE_LOAD)))
            .await;
    let rows = match rows {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => {
            warn!("base load window refresh failed: {e}");
            return None;
        }
        Err(e) => {
            warn!("base load window refresh task panicked: {e}");
            return None;
        }
    };
    let stats = LoadWindowStats::from_power_kw(rows.iter().map(|r| r.power_kw));
    sim.lock().await.set_base_load_observed_window(stats);
    stats
}

pub(crate) fn spawn_base_load_window(
    history: Arc<dyn HistoryPort>,
    sim: Arc<Mutex<SimState>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let found = refresh_base_load_window(history.clone(), &sim, Utc::now())
                .await
                .is_some();
            let wait_s = if found {
                REFRESH_INTERVAL_S
            } else {
                RETRY_INTERVAL_S
            };
            tokio::time::sleep(StdDuration::from_secs(wait_s)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::asset_params::{AssetParams, BaseLoadParams};
    use crate::entities::history::TickSample;
    use crate::services::test_support::mock_history_port::MockHistoryPort;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 6, 12, 0, 0).unwrap()
    }

    fn row(age: Duration, power_kw: f64) -> TickSample {
        TickSample {
            ts: now() - age,
            asset_id: ASSET_BASE_LOAD.to_string(),
            power_kw,
            soc_pct: None,
            temperature_c: None,
            generation_limit_kw: None,
            curtailment_source: None,
            plugged: None,
        }
    }

    fn sim() -> Mutex<SimState> {
        Mutex::new(SimState::from_params(
            &[AssetParams::BaseLoad(BaseLoadParams::default())],
            now(),
        ))
    }

    async fn base_load_features(sim: &Mutex<SimState>) -> Vec<(String, String)> {
        let sim = sim.lock().await;
        let (entry, asset) = sim.find_asset(ASSET_BASE_LOAD).unwrap();
        asset
            .key_features(&entry.state)
            .into_iter()
            .map(|f| (f.label, f.value))
            .collect()
    }

    #[tokio::test]
    async fn refresh_injects_average_and_max_of_the_records_inside_the_window() {
        let history = MockHistoryPort::new();
        history
            .append_tick_samples(&[
                row(Duration::hours(1), 0.2),
                row(Duration::days(2), 0.4),
                // Older than the window: must not count.
                row(Duration::days(LOAD_WINDOW_DAYS + 6), 9.0),
            ])
            .unwrap();
        let sim = sim();

        let stats = refresh_base_load_window(Arc::new(history), &sim, now()).await;

        assert_eq!(stats, LoadWindowStats::from_power_kw([0.2, 0.4]));
        assert_eq!(
            base_load_features(&sim).await,
            vec![
                ("avg".to_string(), "0.30 kW".to_string()),
                ("max".to_string(), "0.40 kW".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn refresh_without_records_leaves_dashes() {
        let sim = sim();

        let stats = refresh_base_load_window(Arc::new(MockHistoryPort::new()), &sim, now()).await;

        assert_eq!(stats, None);
        assert_eq!(
            base_load_features(&sim).await,
            vec![
                ("avg".to_string(), "-".to_string()),
                ("max".to_string(), "-".to_string()),
            ]
        );
    }
}
