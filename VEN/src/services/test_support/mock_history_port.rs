/// `MockHistoryPort` — in-memory `HistoryPort` fake for use-case/route tests.
///
/// Unlike `MockSolverPort`'s single canned response, this behaves like a tiny
/// real store: appended rows are recorded and `query_*` methods apply the same
/// time-range (and, for ticks, asset_id) filtering a real adapter would, so
/// callers can assert on data that flowed all the way through a sampler task
/// or a route handler without needing a real SQLite file.
use std::sync::Mutex;

use chrono::{DateTime, Utc};

use crate::controller::HistoryPort;
use crate::entities::history::{
    EventReceived, ForecastAccuracySample, ForecastLeadKind, GridSample, LedgerPeriod, ReportSent,
    TickSample,
};
use crate::entities::notification::UserNotification;
use crate::entities::DomainError;

#[derive(Default)]
pub struct MockHistoryPort {
    ticks: Mutex<Vec<TickSample>>,
    grid: Mutex<Vec<GridSample>>,
    events: Mutex<Vec<EventReceived>>,
    reports: Mutex<Vec<ReportSent>>,
    ledger_periods: Mutex<Vec<LedgerPeriod>>,
    notifications: Mutex<Vec<UserNotification>>,
    forecast_samples: Mutex<Vec<ForecastAccuracySample>>,
    /// 030: when set, append methods fail with `StorageError` — for testing
    /// the storage-failure notification producer.
    fail_storage: std::sync::atomic::AtomicBool,
}

impl MockHistoryPort {
    pub fn new() -> Self {
        Self::default()
    }

    /// 030: make subsequent append calls fail with `DomainError::StorageError`.
    pub fn set_fail_storage(&self, fail: bool) {
        self.fail_storage
            .store(fail, std::sync::atomic::Ordering::SeqCst);
    }

    fn storage_result(&self, what: &str) -> Result<(), DomainError> {
        if self.fail_storage.load(std::sync::atomic::Ordering::SeqCst) {
            Err(DomainError::StorageError(format!(
                "{what}: disk full (mock)"
            )))
        } else {
            Ok(())
        }
    }

    /// All tick samples appended so far, in insertion order.
    pub fn appended_ticks(&self) -> Vec<TickSample> {
        self.ticks.lock().unwrap().clone()
    }

    /// All notifications appended so far, in insertion order (WP4.3).
    pub fn appended_notifications(&self) -> Vec<UserNotification> {
        self.notifications.lock().unwrap().clone()
    }

    /// All grid samples appended so far, in insertion order.
    #[allow(dead_code)] // used by WP1.2 sampler tests, not yet by any WP1.1 test
    pub fn appended_grid(&self) -> Vec<GridSample> {
        self.grid.lock().unwrap().clone()
    }

    /// All event-received rows appended so far, in insertion order.
    #[allow(dead_code)] // used by WP1.2 sampler tests, not yet by any WP1.1 test
    pub fn appended_events(&self) -> Vec<EventReceived> {
        self.events.lock().unwrap().clone()
    }

    /// All report-sent rows appended so far, in insertion order.
    #[allow(dead_code)] // used by WP1.2 sampler tests, not yet by any WP1.1 test
    pub fn appended_reports(&self) -> Vec<ReportSent> {
        self.reports.lock().unwrap().clone()
    }

    /// All forecast-accuracy samples appended so far, in insertion order (forecast-accuracy-tracking).
    #[allow(dead_code)] // available for future sampler-level tests, mirroring appended_grid/appended_events
    pub fn appended_forecast_samples(&self) -> Vec<ForecastAccuracySample> {
        self.forecast_samples.lock().unwrap().clone()
    }
}

impl HistoryPort for MockHistoryPort {
    fn append_tick_samples(&self, rows: &[TickSample]) -> Result<(), DomainError> {
        self.storage_result("insert tick samples")?;
        self.ticks.lock().unwrap().extend_from_slice(rows);
        Ok(())
    }

    fn append_grid_sample(&self, row: &GridSample) -> Result<(), DomainError> {
        self.storage_result("insert grid sample")?;
        self.grid.lock().unwrap().push(row.clone());
        Ok(())
    }

    fn append_event_received(&self, row: &EventReceived) -> Result<(), DomainError> {
        self.events.lock().unwrap().push(row.clone());
        Ok(())
    }

    fn append_report_sent(&self, row: &ReportSent) -> Result<(), DomainError> {
        self.reports.lock().unwrap().push(row.clone());
        Ok(())
    }

    fn append_ledger_period(&self, row: &LedgerPeriod) -> Result<(), DomainError> {
        self.ledger_periods.lock().unwrap().push(row.clone());
        Ok(())
    }

    fn append_notification(&self, row: &UserNotification) -> Result<(), DomainError> {
        self.storage_result("insert notification")?;
        self.notifications.lock().unwrap().push(row.clone());
        Ok(())
    }

    fn update_notification_seen(
        &self,
        id: uuid::Uuid,
        count: u32,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let mut rows = self.notifications.lock().unwrap();
        match rows.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                n.count = count;
                n.last_seen_at = last_seen_at;
                Ok(())
            }
            None => Err(DomainError::NotFound { id }),
        }
    }

    fn query_notifications(
        &self,
        since: Option<DateTime<Utc>>,
        limit: usize,
        severity: Option<crate::entities::design_vocabulary::UserNotificationSeverity>,
    ) -> Result<Vec<UserNotification>, DomainError> {
        let matching: Vec<_> = self
            .notifications
            .lock()
            .unwrap()
            .iter()
            .filter(|n| since.is_none_or(|s| n.last_seen_at > s))
            .filter(|n| severity.as_ref().is_none_or(|s| n.severity == *s))
            .cloned()
            .collect();
        // Mirror the real store: the newest `limit` rows, oldest first.
        let skip = matching.len().saturating_sub(limit);
        Ok(matching.into_iter().skip(skip).collect())
    }

    fn query_ticks(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        asset_id: Option<&str>,
    ) -> Result<Vec<TickSample>, DomainError> {
        Ok(self
            .ticks
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.ts >= from && r.ts < to)
            .filter(|r| asset_id.is_none_or(|id| r.asset_id == id))
            .cloned()
            .collect())
    }

    fn query_grid(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<GridSample>, DomainError> {
        Ok(self
            .grid
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.ts >= from && r.ts < to)
            .cloned()
            .collect())
    }

    fn query_events(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<EventReceived>, DomainError> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.received_at >= from && r.received_at < to)
            .cloned()
            .collect())
    }

    fn query_reports(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<ReportSent>, DomainError> {
        Ok(self
            .reports
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.sent_at >= from && r.sent_at < to)
            .cloned()
            .collect())
    }

    fn query_ledger_periods(&self, asset_id: &str) -> Result<Vec<LedgerPeriod>, DomainError> {
        Ok(self
            .ledger_periods
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.asset_id == asset_id)
            .cloned()
            .collect())
    }

    fn append_forecast_samples(&self, rows: &[ForecastAccuracySample]) -> Result<(), DomainError> {
        self.forecast_samples
            .lock()
            .unwrap()
            .extend_from_slice(rows);
        Ok(())
    }

    fn reconcile_forecast_actuals(
        &self,
        ticks: &[TickSample],
        window_s: i64,
    ) -> Result<(), DomainError> {
        let mut samples = self.forecast_samples.lock().unwrap();
        for tick in ticks {
            let window_start = tick.ts;
            let window_end = tick.ts + chrono::Duration::seconds(window_s);
            for sample in samples.iter_mut() {
                if sample.asset_id == tick.asset_id
                    && sample.actual_kw.is_none()
                    && sample.target_ts >= window_start
                    && sample.target_ts < window_end
                {
                    sample.actual_kw = Some(tick.power_kw);
                    sample.actual_at = Some(window_start);
                }
            }
        }
        Ok(())
    }

    fn query_forecast_accuracy(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        asset_id: Option<&str>,
        lead_kind: Option<ForecastLeadKind>,
    ) -> Result<Vec<ForecastAccuracySample>, DomainError> {
        Ok(self
            .forecast_samples
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.target_ts >= from && r.target_ts < to)
            .filter(|r| asset_id.is_none_or(|id| r.asset_id == id))
            .filter(|r| lead_kind.is_none_or(|k| r.lead_kind == k))
            .cloned()
            .collect())
    }

    fn prune_before(&self, cutoff: DateTime<Utc>) -> Result<u64, DomainError> {
        let mut total: u64 = 0;
        let mut ticks = self.ticks.lock().unwrap();
        let before = ticks.len();
        ticks.retain(|r| r.ts >= cutoff);
        total += (before - ticks.len()) as u64;
        drop(ticks);

        let mut grid = self.grid.lock().unwrap();
        let before = grid.len();
        grid.retain(|r| r.ts >= cutoff);
        total += (before - grid.len()) as u64;
        drop(grid);

        let mut events = self.events.lock().unwrap();
        let before = events.len();
        events.retain(|r| r.received_at >= cutoff);
        total += (before - events.len()) as u64;
        drop(events);

        let mut reports = self.reports.lock().unwrap();
        let before = reports.len();
        reports.retain(|r| r.sent_at >= cutoff);
        total += (before - reports.len()) as u64;
        drop(reports);

        let mut ledger_periods = self.ledger_periods.lock().unwrap();
        let before = ledger_periods.len();
        ledger_periods.retain(|r| r.period_end >= cutoff);
        total += (before - ledger_periods.len()) as u64;
        drop(ledger_periods);

        let mut forecast_samples = self.forecast_samples.lock().unwrap();
        let before = forecast_samples.len();
        forecast_samples.retain(|r| r.target_ts >= cutoff);
        total += (before - forecast_samples.len()) as u64;

        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn test_append_and_query_ticks_roundtrip() {
        let port = MockHistoryPort::new();
        let row = TickSample {
            ts: ts(100),
            asset_id: "ev".into(),
            power_kw: 1.0,
            soc_pct: None,
            temperature_c: None,
            generation_limit_kw: None,
            curtailment_source: None,
        };
        port.append_tick_samples(std::slice::from_ref(&row))
            .unwrap();
        assert_eq!(port.query_ticks(ts(0), ts(200), None).unwrap(), vec![row]);
    }

    #[test]
    fn test_query_ticks_filters_by_time_range() {
        let port = MockHistoryPort::new();
        port.append_tick_samples(&[
            TickSample {
                ts: ts(100),
                asset_id: "ev".into(),
                power_kw: 1.0,
                soc_pct: None,
                temperature_c: None,
                generation_limit_kw: None,
                curtailment_source: None,
            },
            TickSample {
                ts: ts(9999),
                asset_id: "ev".into(),
                power_kw: 2.0,
                soc_pct: None,
                temperature_c: None,
                generation_limit_kw: None,
                curtailment_source: None,
            },
        ])
        .unwrap();
        let rows = port.query_ticks(ts(0), ts(200), None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].power_kw, 1.0);
    }

    #[test]
    fn test_prune_before_removes_across_all_tables() {
        let port = MockHistoryPort::new();
        port.append_tick_samples(&[TickSample {
            ts: ts(1),
            asset_id: "ev".into(),
            power_kw: 1.0,
            soc_pct: None,
            temperature_c: None,
            generation_limit_kw: None,
            curtailment_source: None,
        }])
        .unwrap();
        port.append_grid_sample(&GridSample {
            ts: ts(1),
            import_kw: 1.0,
            export_kw: 0.0,
            import_tariff_eur_kwh: None,
            export_tariff_eur_kwh: None,
            co2_g_kwh: None,
            import_limit_kw: None,
            export_limit_kw: None,
        })
        .unwrap();

        let deleted = port.prune_before(ts(1000)).unwrap();
        assert_eq!(deleted, 2);
        assert!(port.query_ticks(ts(0), ts(2000), None).unwrap().is_empty());
    }

    #[test]
    fn test_appended_ticks_returns_insertion_order() {
        let port = MockHistoryPort::new();
        let a = TickSample {
            ts: ts(1),
            asset_id: "a".into(),
            power_kw: 1.0,
            soc_pct: None,
            temperature_c: None,
            generation_limit_kw: None,
            curtailment_source: None,
        };
        let b = TickSample {
            ts: ts(2),
            asset_id: "b".into(),
            power_kw: 2.0,
            soc_pct: None,
            temperature_c: None,
            generation_limit_kw: None,
            curtailment_source: None,
        };
        port.append_tick_samples(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(port.appended_ticks(), vec![a, b]);
    }

    fn forecast_sample(asset_id: &str, target_ts: DateTime<Utc>) -> ForecastAccuracySample {
        ForecastAccuracySample {
            asset_id: asset_id.into(),
            lead_kind: ForecastLeadKind::Near,
            target_ts,
            predicted_kw: 1.5,
            predicted_at: ts(0),
            actual_kw: None,
            actual_at: None,
        }
    }

    #[test]
    fn test_append_and_query_forecast_samples_roundtrip() {
        let port = MockHistoryPort::new();
        let row = forecast_sample("pv", ts(100));
        port.append_forecast_samples(std::slice::from_ref(&row))
            .unwrap();
        assert_eq!(
            port.query_forecast_accuracy(ts(0), ts(200), None, None)
                .unwrap(),
            vec![row]
        );
    }

    #[test]
    fn test_reconcile_forecast_actuals_fills_matching_open_row() {
        let port = MockHistoryPort::new();
        port.append_forecast_samples(&[forecast_sample("pv", ts(100))])
            .unwrap();
        port.reconcile_forecast_actuals(
            &[TickSample {
                ts: ts(100),
                asset_id: "pv".into(),
                power_kw: 2.5,
                soc_pct: None,
                temperature_c: None,
                generation_limit_kw: None,
                curtailment_source: None,
            }],
            60,
        )
        .unwrap();
        let rows = port
            .query_forecast_accuracy(ts(0), ts(200), None, None)
            .unwrap();
        assert_eq!(rows[0].actual_kw, Some(2.5));
        assert_eq!(rows[0].actual_at, Some(ts(100)));
    }

    #[test]
    fn test_reconcile_forecast_actuals_leaves_non_matching_row_untouched() {
        let port = MockHistoryPort::new();
        port.append_forecast_samples(&[forecast_sample("pv", ts(1_000_000))])
            .unwrap();
        port.reconcile_forecast_actuals(
            &[TickSample {
                ts: ts(100),
                asset_id: "pv".into(),
                power_kw: 2.5,
                soc_pct: None,
                temperature_c: None,
                generation_limit_kw: None,
                curtailment_source: None,
            }],
            60,
        )
        .unwrap();
        let rows = port
            .query_forecast_accuracy(ts(0), ts(2_000_000), None, None)
            .unwrap();
        assert_eq!(rows[0].actual_kw, None);
    }

    #[test]
    fn test_reconcile_forecast_actuals_does_not_overwrite_an_already_reconciled_row() {
        let port = MockHistoryPort::new();
        port.append_forecast_samples(&[forecast_sample("pv", ts(100))])
            .unwrap();
        let tick = TickSample {
            ts: ts(100),
            asset_id: "pv".into(),
            power_kw: 2.5,
            soc_pct: None,
            temperature_c: None,
            generation_limit_kw: None,
            curtailment_source: None,
        };
        port.reconcile_forecast_actuals(std::slice::from_ref(&tick), 60)
            .unwrap();
        let second_tick = TickSample {
            power_kw: 9.9,
            ..tick
        };
        port.reconcile_forecast_actuals(&[second_tick], 60).unwrap();
        let rows = port
            .query_forecast_accuracy(ts(0), ts(200), None, None)
            .unwrap();
        assert_eq!(
            rows[0].actual_kw,
            Some(2.5),
            "first reconciliation must stick"
        );
    }

    #[test]
    fn test_prune_before_removes_forecast_samples_by_target_ts() {
        let port = MockHistoryPort::new();
        port.append_forecast_samples(&[forecast_sample("pv", ts(1))])
            .unwrap();
        let deleted = port.prune_before(ts(1000)).unwrap();
        assert_eq!(deleted, 1);
        assert!(port
            .query_forecast_accuracy(ts(0), ts(2000), None, None)
            .unwrap()
            .is_empty());
    }
}
