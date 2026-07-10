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
    EventReceived, GridSample, LedgerPeriod, PlanSnapshot, ReportSent, TickSample,
};
use crate::entities::DomainError;

#[derive(Default)]
pub struct MockHistoryPort {
    ticks: Mutex<Vec<TickSample>>,
    grid: Mutex<Vec<GridSample>>,
    plans: Mutex<Vec<PlanSnapshot>>,
    events: Mutex<Vec<EventReceived>>,
    reports: Mutex<Vec<ReportSent>>,
    ledger_periods: Mutex<Vec<LedgerPeriod>>,
}

impl MockHistoryPort {
    pub fn new() -> Self {
        Self::default()
    }

    /// All tick samples appended so far, in insertion order.
    pub fn appended_ticks(&self) -> Vec<TickSample> {
        self.ticks.lock().unwrap().clone()
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
}

impl HistoryPort for MockHistoryPort {
    fn append_tick_samples(&self, rows: &[TickSample]) -> Result<(), DomainError> {
        self.ticks.lock().unwrap().extend_from_slice(rows);
        Ok(())
    }

    fn append_grid_sample(&self, row: &GridSample) -> Result<(), DomainError> {
        self.grid.lock().unwrap().push(row.clone());
        Ok(())
    }

    fn append_plan_snapshot(&self, row: &PlanSnapshot) -> Result<(), DomainError> {
        self.plans.lock().unwrap().push(row.clone());
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

    fn query_plans(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<PlanSnapshot>, DomainError> {
        Ok(self
            .plans
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.created_at >= from && r.created_at < to)
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

        let mut plans = self.plans.lock().unwrap();
        let before = plans.len();
        plans.retain(|r| r.created_at >= cutoff);
        total += (before - plans.len()) as u64;
        drop(plans);

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
            },
            TickSample {
                ts: ts(9999),
                asset_id: "ev".into(),
                power_kw: 2.0,
                soc_pct: None,
                temperature_c: None,
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
        }])
        .unwrap();
        port.append_grid_sample(&GridSample {
            ts: ts(1),
            import_kw: 1.0,
            export_kw: 0.0,
            import_tariff_eur_kwh: None,
            export_tariff_eur_kwh: None,
            co2_g_kwh: None,
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
        };
        let b = TickSample {
            ts: ts(2),
            asset_id: "b".into(),
            power_kw: 2.0,
            soc_pct: None,
            temperature_c: None,
        };
        port.append_tick_samples(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(port.appended_ticks(), vec![a, b]);
    }
}
