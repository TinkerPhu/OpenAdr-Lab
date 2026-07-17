// HistoryPort trait — the boundary between application/service code and the
// persistent history store. Mirrors SolverPort/SimulatorPort/VtnPort: the
// trait is domain-level; the implementation (a SQLite-backed adapter) lives
// in `history_store` (infra ring), reachable only through this port.
//
// Every method is synchronous/blocking by design: the concrete adapter wraps
// a blocking `rusqlite::Connection`. Callers in async contexts must invoke
// these through `tokio::task::spawn_blocking` — never call directly from an
// async fn body.
//
// Not yet consumed as `dyn HistoryPort` from `main.rs` — that wiring is
// WP1.2's history-sampler task; landing the port + adapter as their own
// reviewable commit first.
#![allow(dead_code)]
use chrono::{DateTime, Utc};

use crate::entities::history::{
    EventReceived, GridSample, LedgerPeriod, PlanSnapshot, ReportSent, TickSample,
};
use crate::entities::notification::UserNotification;
use crate::entities::DomainError;

pub trait HistoryPort: Send + Sync {
    fn append_tick_samples(&self, rows: &[TickSample]) -> Result<(), DomainError>;
    fn append_grid_sample(&self, row: &GridSample) -> Result<(), DomainError>;
    fn append_plan_snapshot(&self, row: &PlanSnapshot) -> Result<(), DomainError>;
    fn append_event_received(&self, row: &EventReceived) -> Result<(), DomainError>;
    fn append_report_sent(&self, row: &ReportSent) -> Result<(), DomainError>;
    fn append_ledger_period(&self, row: &LedgerPeriod) -> Result<(), DomainError>;
    /// WP4.3 (BL-20): persist one user notification. Default no-op so
    /// history-less test doubles keep compiling; the SQLite store overrides.
    fn append_notification(&self, row: &UserNotification) -> Result<(), DomainError> {
        let _ = row;
        Ok(())
    }

    fn query_ticks(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        asset_id: Option<&str>,
    ) -> Result<Vec<TickSample>, DomainError>;
    fn query_grid(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<GridSample>, DomainError>;
    fn query_events(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<EventReceived>, DomainError>;
    fn query_reports(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<ReportSent>, DomainError>;
    fn query_plans(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<PlanSnapshot>, DomainError>;
    fn query_ledger_periods(&self, asset_id: &str) -> Result<Vec<LedgerPeriod>, DomainError>;
    /// WP4.3 (BL-20): the NEWEST `limit` notifications last seen after `since`
    /// (all when `None`), returned oldest first. 030: optionally filtered to
    /// one severity — the filter applies before `limit`, so a filtered page is
    /// still `limit` matching rows. Default empty for test doubles.
    fn query_notifications(
        &self,
        since: Option<DateTime<Utc>>,
        limit: usize,
        severity: Option<crate::entities::design_vocabulary::UserNotificationSeverity>,
    ) -> Result<Vec<UserNotification>, DomainError> {
        let _ = (since, limit, severity);
        Ok(Vec::new())
    }

    /// 030 (notification-dedup): record a dedup hit — bump `count` and
    /// `last_seen_at` on an existing notification. The window decision is
    /// made in the application layer. Default no-op for test doubles.
    fn update_notification_seen(
        &self,
        id: uuid::Uuid,
        count: u32,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let _ = (id, count, last_seen_at);
        Ok(())
    }

    /// Delete all rows across every table with a time column older than `cutoff`.
    /// Returns the total number of rows deleted.
    fn prune_before(&self, cutoff: DateTime<Utc>) -> Result<u64, DomainError>;
}
