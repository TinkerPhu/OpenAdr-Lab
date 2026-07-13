//! WP1.2/1.3/1.6 — history sampler task glue: 1-minute downsample write path
//! (accumulator in `accumulator.rs`), daily retention pruning, and monthly
//! `AssetLedger` billing-period rollover. All boundary checks are pure and
//! clock-injected (`now` passed in per call) so they're testable without
//! sleeps; the async loop is a thin wrapper spawning its own 1s tick,
//! snapshotting the simulator (matching the `sim.lock().await` pattern used
//! by other tasks, e.g. `tasks::obligation`), and writing through
//! `spawn_blocking` (history writes are best-effort — log-and-continue,
//! never block the control loop).
mod accumulator;

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Datelike, TimeZone, Utc};
use tokio::sync::Mutex;
use tracing::warn;

use accumulator::HistorySampler;

use crate::controller::{HistoryPort, SimulatorPort};
use crate::entities::history::{GridSample, LedgerPeriod, TickSample};
use crate::entities::DomainError;
use crate::simulator::SimState;
use crate::state::{AppState, AssetLedgerEntry};

/// Append a flushed window through the (blocking) `HistoryPort`, logging and
/// continuing on failure — history writes must never block or crash the
/// control loop.
async fn write_window(history: Arc<dyn HistoryPort>, ticks: Vec<TickSample>, grid: GridSample) {
    let res = tokio::task::spawn_blocking(move || {
        history.append_tick_samples(&ticks)?;
        history.append_grid_sample(&grid)
    })
    .await;
    match res {
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!("history sampler write failed: {e}"),
        Err(e) => warn!("history sampler write task panicked: {e}"),
    }
}

/// WP1.3 — returns `true` (and records `now`'s UTC calendar day) exactly the
/// first time this is called for a given day, i.e. once per day boundary.
/// Fires on the very first call too — pruning is idempotent, so an immediate
/// startup prune is desirable, unlike ledger rollover below.
fn day_boundary_crossed(last_pruned_day: &mut Option<i64>, now: DateTime<Utc>) -> bool {
    let day = now.timestamp().div_euclid(86_400);
    if *last_pruned_day == Some(day) {
        false
    } else {
        *last_pruned_day = Some(day);
        true
    }
}

/// Run `HistoryPort::prune_before` (WAL checkpoint happens inside the adapter)
/// off the async loop, logging and continuing on failure.
async fn prune_retention(history: Arc<dyn HistoryPort>, cutoff: DateTime<Utc>) {
    match tokio::task::spawn_blocking(move || history.prune_before(cutoff)).await {
        Ok(Ok(n)) => {
            if n > 0 {
                tracing::info!("history retention prune: removed {n} rows older than {cutoff}");
            }
        }
        Ok(Err(e)) => warn!("history retention prune failed: {e}"),
        Err(e) => warn!("history retention prune task panicked: {e}"),
    }
}

/// WP1.6 — returns the `(year, month)` being closed exactly when `now` moves
/// into a new calendar month; `None` on the first call (nothing accumulated
/// yet to close — the live ledger may have survived a restart mid-month via
/// `state.json` persistence, so unlike `day_boundary_crossed` this must NOT
/// fire on startup) and `None` while still in the same month.
fn month_boundary_crossed(last: &mut Option<(i32, u32)>, now: DateTime<Utc>) -> Option<(i32, u32)> {
    let ym = (now.year(), now.month());
    let prev = *last;
    *last = Some(ym);
    prev.filter(|&p| p != ym)
}

fn month_start(year: i32, month: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).unwrap()
}

/// WP1.6 — snapshot the live per-asset ledger into closed `LedgerPeriod` rows.
/// Pure w.r.t. `period_start`/`period_end` (both passed in) so it's testable
/// without touching `AppState`.
fn close_ledger_period(
    ledger: &HashMap<String, AssetLedgerEntry>,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
) -> Vec<LedgerPeriod> {
    ledger
        .values()
        .map(|entry| LedgerPeriod {
            asset_id: entry.asset_id.clone(),
            period_start,
            period_end,
            energy_kwh: entry.energy_kwh,
            cost_eur: entry.cost_eur,
            co2_kg: entry.co2_g / 1000.0,
        })
        .collect()
}

/// Archive the current ledger and reset it for the new period. Skips
/// entirely if nothing was accumulated. Log-and-continue on failure — the
/// live ledger is left untouched (and thus retried next month) if the write
/// fails, rather than losing the data.
async fn rollover_ledger(
    history: Arc<dyn HistoryPort>,
    state: &AppState,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
) {
    let ledger = state.asset_ledger().await;
    if ledger.is_empty() {
        return;
    }
    let rows = close_ledger_period(&ledger, period_start, period_end);
    let res = tokio::task::spawn_blocking(move || -> Result<(), DomainError> {
        for row in &rows {
            history.append_ledger_period(row)?;
        }
        Ok(())
    })
    .await;
    match res {
        Ok(Ok(())) => state.set_asset_ledger(HashMap::new()).await,
        Ok(Err(e)) => warn!("ledger rollover write failed: {e}"),
        Err(e) => warn!("ledger rollover task panicked: {e}"),
    }
}

pub(crate) fn spawn_history_sampler(
    sim: Arc<Mutex<SimState>>,
    history: Arc<dyn HistoryPort>,
    state: AppState,
    retention_days: u32,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut sampler = HistorySampler::new();
        let mut last_pruned_day: Option<i64> = None;
        let mut last_ledger_month: Option<(i32, u32)> = None;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let now = Utc::now();
            let mut snap = {
                let sim_guard = sim.lock().await;
                sim_guard
                    .snapshot()
                    .expect("SimState::snapshot is infallible")
            };
            // SITE_RESIDUAL (BL-08, Phase 5 WP5.1): this loop takes its own
            // raw simulator snapshot independent of `tick_once`/`publish.rs`
            // (a separate 1s cadence), so site-residual must be inserted
            // here too for its history to accumulate via `tick_samples`.
            let residual_kw = crate::controller::residual::compute_site_residual_kw(&snap);
            snap.assets.insert(
                crate::controller::residual::SITE_RESIDUAL_ASSET_ID.to_string(),
                crate::controller::residual::site_residual_snapshot(residual_kw),
            );
            let tariffs_snap = state.planned_tariffs().await;
            if let Some((ticks, grid)) = sampler.record(now, &snap, &tariffs_snap) {
                write_window(history.clone(), ticks, grid).await;
            }
            if day_boundary_crossed(&mut last_pruned_day, now) {
                let cutoff = now - chrono::Duration::days(retention_days as i64);
                prune_retention(history.clone(), cutoff).await;
            }
            if let Some((py, pm)) = month_boundary_crossed(&mut last_ledger_month, now) {
                let period_start = month_start(py, pm);
                let period_end = month_start(now.year(), now.month());
                rollover_ledger(history.clone(), &state, period_start, period_end).await;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn ymd(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
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
        assert!(day_boundary_crossed(&mut last, ts(0)));
        assert!(
            !day_boundary_crossed(&mut last, ts(86_399)),
            "still day 0 — must not cross again"
        );
    }

    #[test]
    fn test_day_boundary_crossed_next_day_is_true_exactly_once() {
        let mut last = None;
        assert!(day_boundary_crossed(&mut last, ts(0)));
        assert!(day_boundary_crossed(&mut last, ts(86_400)), "day 1 begins");
        assert!(
            !day_boundary_crossed(&mut last, ts(86_400 + 100)),
            "still day 1 — must not cross again"
        );
    }

    #[test]
    fn test_month_boundary_crossed_first_call_is_none() {
        let mut last = None;
        assert_eq!(month_boundary_crossed(&mut last, ymd(2026, 1, 15)), None);
        assert_eq!(last, Some((2026, 1)));
    }

    #[test]
    fn test_month_boundary_crossed_same_month_is_none() {
        let mut last = None;
        month_boundary_crossed(&mut last, ymd(2026, 1, 1));
        assert_eq!(month_boundary_crossed(&mut last, ymd(2026, 1, 28)), None);
    }

    #[test]
    fn test_month_boundary_crossed_returns_old_period_exactly_once() {
        let mut last = None;
        month_boundary_crossed(&mut last, ymd(2026, 1, 15));
        assert_eq!(
            month_boundary_crossed(&mut last, ymd(2026, 2, 1)),
            Some((2026, 1))
        );
        assert_eq!(
            month_boundary_crossed(&mut last, ymd(2026, 2, 15)),
            None,
            "still February — must not cross again"
        );
    }

    #[test]
    fn test_month_boundary_crossed_handles_year_rollover() {
        let mut last = None;
        month_boundary_crossed(&mut last, ymd(2026, 12, 20));
        assert_eq!(
            month_boundary_crossed(&mut last, ymd(2027, 1, 1)),
            Some((2026, 12))
        );
    }

    #[test]
    fn test_close_ledger_period_maps_entries_and_converts_co2_to_kg() {
        let mut ledger = HashMap::new();
        ledger.insert(
            "ev".to_string(),
            AssetLedgerEntry {
                asset_id: "ev".to_string(),
                energy_kwh: 42.0,
                cost_eur: 10.5,
                co2_g: 2500.0,
                updated_at: None,
                started_at: None,
            },
        );
        let rows = close_ledger_period(&ledger, ymd(2026, 1, 1), ymd(2026, 2, 1));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].asset_id, "ev");
        assert_eq!(rows[0].period_start, ymd(2026, 1, 1));
        assert_eq!(rows[0].period_end, ymd(2026, 2, 1));
        assert_eq!(rows[0].energy_kwh, 42.0);
        assert_eq!(rows[0].cost_eur, 10.5);
        assert!((rows[0].co2_kg - 2.5).abs() < 1e-9, "2500 g == 2.5 kg");
    }

    #[test]
    fn test_close_ledger_period_empty_ledger_returns_empty() {
        let rows = close_ledger_period(&HashMap::new(), ymd(2026, 1, 1), ymd(2026, 2, 1));
        assert!(rows.is_empty());
    }
}
