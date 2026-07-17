//! Phase 1 (A-1) — SQLite-backed `HistoryPort` adapter (infra ring).
//!
//! One file per VEN (`/data/history.sqlite`, docker volume so it survives
//! rebuilds). Schema versioned via `PRAGMA user_version` + an idempotent
//! migration run at `open()`. All access goes through a blocking
//! `std::sync::Mutex<Connection>` — rusqlite is blocking; callers in async
//! contexts must use `tokio::task::spawn_blocking`.
//!
//! Not yet constructed from `main.rs` — wiring it into the app (behind a
//! `history.enabled` profile flag) is WP1.2's history-sampler task. Landing
//! the schema/adapter as its own reviewable commit first.
#![allow(dead_code)]

mod notifications;
mod schema;
mod settings;

use std::sync::Mutex;

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{params, Connection};

use crate::controller::HistoryPort;
use crate::entities::history::{
    EventReceived, GridSample, LedgerPeriod, PlanSnapshot, ReportSent, TickSample,
};
use crate::entities::DomainError;
use schema::{SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_VERSION};

type TickSampleRow = (i64, String, f64, Option<f64>, Option<f64>);
type GridSampleRow = (i64, f64, f64, Option<f64>, Option<f64>, Option<f64>);

pub struct SqliteHistoryStore {
    conn: Mutex<Connection>,
}

fn to_unix(ts: DateTime<Utc>) -> i64 {
    ts.timestamp()
}

fn from_unix(secs: i64) -> Result<DateTime<Utc>, DomainError> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .ok_or_else(|| DomainError::StorageError(format!("invalid stored timestamp: {secs}")))
}

impl SqliteHistoryStore {
    /// Open (creating if absent) the history database at `path` and run any
    /// pending migration. Idempotent — safe to call against an already
    /// up-to-date database.
    pub fn open(path: &str) -> Result<Self, DomainError> {
        let conn = Connection::open(path)
            .map_err(|e| DomainError::StorageError(format!("open {path}: {e}")))?;
        Self::from_connection(conn)
    }

    /// In-memory database for tests — no filesystem I/O.
    pub fn in_memory() -> Result<Self, DomainError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| DomainError::StorageError(format!("open in-memory db: {e}")))?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, DomainError> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| DomainError::StorageError(format!("set WAL mode: {e}")))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn migrate(conn: &Connection) -> Result<(), DomainError> {
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| DomainError::StorageError(format!("read user_version: {e}")))?;
        if version >= SCHEMA_VERSION {
            return Ok(());
        }
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)
                .map_err(|e| DomainError::StorageError(format!("apply schema v1: {e}")))?;
        }
        if version < 2 {
            conn.execute_batch(SCHEMA_V2)
                .map_err(|e| DomainError::StorageError(format!("apply schema v2: {e}")))?;
        }
        if version < 3 {
            conn.execute_batch(SCHEMA_V3)
                .map_err(|e| DomainError::StorageError(format!("apply schema v3: {e}")))?;
        }
        if version < 4 {
            conn.execute_batch(SCHEMA_V4)
                .map_err(|e| DomainError::StorageError(format!("apply schema v4: {e}")))?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|e| DomainError::StorageError(format!("set user_version: {e}")))?;
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, DomainError> {
        self.conn
            .lock()
            .map_err(|_| DomainError::StorageError("history db connection poisoned".into()))
    }
}

impl HistoryPort for SqliteHistoryStore {
    fn append_tick_samples(&self, rows: &[TickSample]) -> Result<(), DomainError> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| DomainError::StorageError(format!("begin tx: {e}")))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO tick_samples (ts, asset_id, power_kw, soc_pct, temperature_c)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(|e| DomainError::StorageError(format!("prepare insert: {e}")))?;
            for row in rows {
                stmt.execute(params![
                    to_unix(row.ts),
                    row.asset_id,
                    row.power_kw,
                    row.soc_pct,
                    row.temperature_c,
                ])
                .map_err(|e| DomainError::StorageError(format!("insert tick sample: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| DomainError::StorageError(format!("commit tx: {e}")))?;
        Ok(())
    }

    fn append_grid_sample(&self, row: &GridSample) -> Result<(), DomainError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO grid_samples
                (ts, import_kw, export_kw, import_tariff_eur_kwh, export_tariff_eur_kwh, co2_g_kwh)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                to_unix(row.ts),
                row.import_kw,
                row.export_kw,
                row.import_tariff_eur_kwh,
                row.export_tariff_eur_kwh,
                row.co2_g_kwh,
            ],
        )
        .map_err(|e| DomainError::StorageError(format!("insert grid sample: {e}")))?;
        Ok(())
    }

    fn append_plan_snapshot(&self, row: &PlanSnapshot) -> Result<(), DomainError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO plan_snapshots (created_at, horizon_start, horizon_end, plan_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                to_unix(row.created_at),
                to_unix(row.horizon_start),
                to_unix(row.horizon_end),
                row.plan_json,
            ],
        )
        .map_err(|e| DomainError::StorageError(format!("insert plan snapshot: {e}")))?;
        Ok(())
    }

    fn append_event_received(&self, row: &EventReceived) -> Result<(), DomainError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO events_received (received_at, event_id, event_type, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                to_unix(row.received_at),
                row.event_id,
                row.event_type,
                row.payload_json,
            ],
        )
        .map_err(|e| DomainError::StorageError(format!("insert event received: {e}")))?;
        Ok(())
    }

    fn append_report_sent(&self, row: &ReportSent) -> Result<(), DomainError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO reports_sent (sent_at, report_type, event_id, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                to_unix(row.sent_at),
                row.report_type,
                row.event_id,
                row.payload_json,
            ],
        )
        .map_err(|e| DomainError::StorageError(format!("insert report sent: {e}")))?;
        Ok(())
    }

    fn append_ledger_period(&self, row: &LedgerPeriod) -> Result<(), DomainError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO ledger_periods
                (asset_id, period_start, period_end, energy_kwh, cost_eur, co2_kg)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.asset_id,
                to_unix(row.period_start),
                to_unix(row.period_end),
                row.energy_kwh,
                row.cost_eur,
                row.co2_kg,
            ],
        )
        .map_err(|e| DomainError::StorageError(format!("insert ledger period: {e}")))?;
        Ok(())
    }

    fn query_ticks(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        asset_id: Option<&str>,
    ) -> Result<Vec<TickSample>, DomainError> {
        let conn = self.lock()?;
        let (sql, asset_filter): (&str, Option<&str>) = match asset_id {
            Some(id) => (
                "SELECT ts, asset_id, power_kw, soc_pct, temperature_c FROM tick_samples
                 WHERE ts >= ?1 AND ts < ?2 AND asset_id = ?3 ORDER BY ts ASC",
                Some(id),
            ),
            None => (
                "SELECT ts, asset_id, power_kw, soc_pct, temperature_c FROM tick_samples
                 WHERE ts >= ?1 AND ts < ?2 ORDER BY ts ASC",
                None,
            ),
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| DomainError::StorageError(format!("prepare query_ticks: {e}")))?;
        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<TickSampleRow> {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        };
        let raw: Vec<_> = if let Some(id) = asset_filter {
            stmt.query_map(params![to_unix(from), to_unix(to), id], map_row)
        } else {
            stmt.query_map(params![to_unix(from), to_unix(to)], map_row)
        }
        .map_err(|e| DomainError::StorageError(format!("query_ticks: {e}")))?
        .collect::<Result<_, _>>()
        .map_err(|e| DomainError::StorageError(format!("read query_ticks rows: {e}")))?;

        raw.into_iter()
            .map(
                |(ts, asset_id, power_kw, soc_pct, temperature_c)| -> Result<TickSample, DomainError> {
                    Ok(TickSample {
                        ts: from_unix(ts)?,
                        asset_id,
                        power_kw,
                        soc_pct,
                        temperature_c,
                    })
                },
            )
            .collect()
    }

    fn query_grid(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<GridSample>, DomainError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT ts, import_kw, export_kw, import_tariff_eur_kwh, export_tariff_eur_kwh, co2_g_kwh
                 FROM grid_samples WHERE ts >= ?1 AND ts < ?2 ORDER BY ts ASC",
            )
            .map_err(|e| DomainError::StorageError(format!("prepare query_grid: {e}")))?;
        let raw: Vec<GridSampleRow> = stmt
            .query_map(params![to_unix(from), to_unix(to)], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .map_err(|e| DomainError::StorageError(format!("query_grid: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| DomainError::StorageError(format!("read query_grid rows: {e}")))?;

        raw.into_iter()
            .map(
                |(
                    ts,
                    import_kw,
                    export_kw,
                    import_tariff_eur_kwh,
                    export_tariff_eur_kwh,
                    co2_g_kwh,
                )|
                 -> Result<GridSample, DomainError> {
                    Ok(GridSample {
                        ts: from_unix(ts)?,
                        import_kw,
                        export_kw,
                        import_tariff_eur_kwh,
                        export_tariff_eur_kwh,
                        co2_g_kwh,
                    })
                },
            )
            .collect()
    }

    fn query_events(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<EventReceived>, DomainError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT received_at, event_id, event_type, payload_json FROM events_received
                 WHERE received_at >= ?1 AND received_at < ?2 ORDER BY received_at ASC",
            )
            .map_err(|e| DomainError::StorageError(format!("prepare query_events: {e}")))?;
        let raw: Vec<(i64, String, String, String)> = stmt
            .query_map(params![to_unix(from), to_unix(to)], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| DomainError::StorageError(format!("query_events: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| DomainError::StorageError(format!("read query_events rows: {e}")))?;

        raw.into_iter()
            .map(
                |(received_at, event_id, event_type, payload_json)| -> Result<EventReceived, DomainError> {
                    Ok(EventReceived {
                        received_at: from_unix(received_at)?,
                        event_id,
                        event_type,
                        payload_json,
                    })
                },
            )
            .collect()
    }

    fn query_reports(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<ReportSent>, DomainError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT sent_at, report_type, event_id, payload_json FROM reports_sent
                 WHERE sent_at >= ?1 AND sent_at < ?2 ORDER BY sent_at ASC",
            )
            .map_err(|e| DomainError::StorageError(format!("prepare query_reports: {e}")))?;
        let raw: Vec<(i64, String, String, String)> = stmt
            .query_map(params![to_unix(from), to_unix(to)], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| DomainError::StorageError(format!("query_reports: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| DomainError::StorageError(format!("read query_reports rows: {e}")))?;

        raw.into_iter()
            .map(
                |(sent_at, report_type, event_id, payload_json)| -> Result<ReportSent, DomainError> {
                    Ok(ReportSent {
                        sent_at: from_unix(sent_at)?,
                        report_type,
                        event_id,
                        payload_json,
                    })
                },
            )
            .collect()
    }

    fn query_plans(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<PlanSnapshot>, DomainError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT created_at, horizon_start, horizon_end, plan_json FROM plan_snapshots
                 WHERE created_at >= ?1 AND created_at < ?2 ORDER BY created_at ASC",
            )
            .map_err(|e| DomainError::StorageError(format!("prepare query_plans: {e}")))?;
        let raw: Vec<(i64, i64, i64, String)> = stmt
            .query_map(params![to_unix(from), to_unix(to)], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| DomainError::StorageError(format!("query_plans: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| DomainError::StorageError(format!("read query_plans rows: {e}")))?;

        raw.into_iter()
            .map(
                |(created_at, horizon_start, horizon_end, plan_json)| -> Result<PlanSnapshot, DomainError> {
                    Ok(PlanSnapshot {
                        created_at: from_unix(created_at)?,
                        horizon_start: from_unix(horizon_start)?,
                        horizon_end: from_unix(horizon_end)?,
                        plan_json,
                    })
                },
            )
            .collect()
    }

    fn query_ledger_periods(&self, asset_id: &str) -> Result<Vec<LedgerPeriod>, DomainError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT asset_id, period_start, period_end, energy_kwh, cost_eur, co2_kg
                 FROM ledger_periods WHERE asset_id = ?1 ORDER BY period_start ASC",
            )
            .map_err(|e| DomainError::StorageError(format!("prepare query_ledger_periods: {e}")))?;
        let raw: Vec<(String, i64, i64, f64, f64, f64)> = stmt
            .query_map(params![asset_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .map_err(|e| DomainError::StorageError(format!("query_ledger_periods: {e}")))?
            .collect::<Result<_, _>>()
            .map_err(|e| {
                DomainError::StorageError(format!("read query_ledger_periods rows: {e}"))
            })?;

        raw.into_iter()
            .map(
                |(asset_id, period_start, period_end, energy_kwh, cost_eur, co2_kg)| -> Result<LedgerPeriod, DomainError> {
                    Ok(LedgerPeriod {
                        asset_id,
                        period_start: from_unix(period_start)?,
                        period_end: from_unix(period_end)?,
                        energy_kwh,
                        cost_eur,
                        co2_kg,
                    })
                },
            )
            .collect()
    }

    fn append_notification(
        &self,
        row: &crate::entities::notification::UserNotification,
    ) -> Result<(), DomainError> {
        notifications::append(&*self.lock()?, row)
    }

    fn query_notifications(
        &self,
        since: Option<DateTime<Utc>>,
        limit: usize,
        severity: Option<crate::entities::design_vocabulary::UserNotificationSeverity>,
    ) -> Result<Vec<crate::entities::notification::UserNotification>, DomainError> {
        notifications::query(&*self.lock()?, since, limit, severity)
    }

    fn update_notification_seen(
        &self,
        id: uuid::Uuid,
        count: u32,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        notifications::update_seen(&*self.lock()?, id, count, last_seen_at)
    }

    fn prune_before(&self, cutoff: DateTime<Utc>) -> Result<u64, DomainError> {
        let conn = self.lock()?;
        let cutoff_unix = to_unix(cutoff);
        let mut total: u64 = 0;
        for (table, col) in [
            ("tick_samples", "ts"),
            ("grid_samples", "ts"),
            ("plan_snapshots", "created_at"),
            ("events_received", "received_at"),
            ("reports_sent", "sent_at"),
            ("ledger_periods", "period_end"),
            ("notifications", "created_at"),
        ] {
            let sql = format!("DELETE FROM {table} WHERE {col} < ?1");
            let n = conn
                .execute(&sql, params![cutoff_unix])
                .map_err(|e| DomainError::StorageError(format!("prune {table}: {e}")))?;
            total += n as u64;
        }
        // WP1.3: reclaim WAL space after a bulk delete. PASSIVE mode never
        // blocks writers, so this is safe to run inline on every prune.
        conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
            .map_err(|e| DomainError::StorageError(format!("wal_checkpoint: {e}")))?;
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs_from_epoch: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs_from_epoch, 0).unwrap()
    }

    #[test]
    fn test_migrate_creates_schema_and_sets_user_version() {
        let store = SqliteHistoryStore::in_memory().expect("open should succeed");
        let conn = store.conn.lock().unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            table_count, 8,
            "expected 6 tables from schema v1 + notifications (v2) + user_settings (v3)"
        );
    }

    #[test]
    fn test_migration_idempotent_on_existing_db() {
        let store = SqliteHistoryStore::in_memory().expect("first open");
        // Re-run migration against the same already-migrated connection —
        // must not error or duplicate schema objects.
        let conn = store.conn.lock().unwrap();
        SqliteHistoryStore::migrate(&conn).expect("second migrate call must be a no-op");
    }

    #[test]
    fn test_append_tick_samples_roundtrip() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        let rows = vec![
            TickSample {
                ts: ts(1000),
                asset_id: "ev".into(),
                power_kw: 3.5,
                soc_pct: Some(42.0),
                temperature_c: None,
            },
            TickSample {
                ts: ts(1060),
                asset_id: "heater".into(),
                power_kw: 1.2,
                soc_pct: None,
                temperature_c: Some(55.0),
            },
        ];
        store.append_tick_samples(&rows).unwrap();

        let queried = store.query_ticks(ts(0), ts(2000), None).unwrap();
        assert_eq!(queried, rows);
    }

    #[test]
    fn test_query_ticks_filters_by_asset() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        store
            .append_tick_samples(&[
                TickSample {
                    ts: ts(1000),
                    asset_id: "ev".into(),
                    power_kw: 3.5,
                    soc_pct: None,
                    temperature_c: None,
                },
                TickSample {
                    ts: ts(1000),
                    asset_id: "heater".into(),
                    power_kw: 1.2,
                    soc_pct: None,
                    temperature_c: None,
                },
            ])
            .unwrap();

        let ev_only = store.query_ticks(ts(0), ts(2000), Some("ev")).unwrap();
        assert_eq!(ev_only.len(), 1);
        assert_eq!(ev_only[0].asset_id, "ev");
    }

    #[test]
    fn test_append_and_query_grid_sample() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        let row = GridSample {
            ts: ts(500),
            import_kw: 2.0,
            export_kw: 0.0,
            import_tariff_eur_kwh: Some(0.25),
            export_tariff_eur_kwh: Some(0.05),
            co2_g_kwh: Some(300.0),
        };
        store.append_grid_sample(&row).unwrap();
        let rows = store.query_grid(ts(0), ts(1000)).unwrap();
        assert_eq!(rows, vec![row]);
    }

    #[test]
    fn test_append_and_query_plan_snapshot() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        let row = PlanSnapshot {
            created_at: ts(100),
            horizon_start: ts(100),
            horizon_end: ts(100 + 48 * 3600),
            plan_json: "{\"slots\":[]}".into(),
        };
        store.append_plan_snapshot(&row).unwrap();
        let rows = store.query_plans(ts(0), ts(1000)).unwrap();
        assert_eq!(rows, vec![row]);
    }

    #[test]
    fn test_append_and_query_event_received() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        let row = EventReceived {
            received_at: ts(200),
            event_id: "evt-1".into(),
            event_type: "PRICE".into(),
            payload_json: "{}".into(),
        };
        store.append_event_received(&row).unwrap();
        let rows = store.query_events(ts(0), ts(1000)).unwrap();
        assert_eq!(rows, vec![row]);
    }

    #[test]
    fn test_append_and_query_report_sent() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        let row = ReportSent {
            sent_at: ts(300),
            report_type: "USAGE".into(),
            event_id: "evt-1".into(),
            payload_json: "{}".into(),
        };
        store.append_report_sent(&row).unwrap();
        let rows = store.query_reports(ts(0), ts(1000)).unwrap();
        assert_eq!(rows, vec![row]);
    }

    #[test]
    fn test_append_and_query_ledger_period() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        let row = LedgerPeriod {
            asset_id: "battery".into(),
            period_start: ts(0),
            period_end: ts(30 * 24 * 3600),
            energy_kwh: 120.5,
            cost_eur: 24.1,
            co2_kg: 15.0,
        };
        store.append_ledger_period(&row).unwrap();
        let rows = store.query_ledger_periods("battery").unwrap();
        assert_eq!(rows, vec![row]);
    }

    #[test]
    fn test_prune_before_deletes_only_older() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        store
            .append_tick_samples(&[
                TickSample {
                    ts: ts(100),
                    asset_id: "ev".into(),
                    power_kw: 1.0,
                    soc_pct: None,
                    temperature_c: None,
                },
                TickSample {
                    ts: ts(10_000),
                    asset_id: "ev".into(),
                    power_kw: 2.0,
                    soc_pct: None,
                    temperature_c: None,
                },
            ])
            .unwrap();
        store
            .append_grid_sample(&GridSample {
                ts: ts(100),
                import_kw: 1.0,
                export_kw: 0.0,
                import_tariff_eur_kwh: None,
                export_tariff_eur_kwh: None,
                co2_g_kwh: None,
            })
            .unwrap();

        let deleted = store.prune_before(ts(5000)).unwrap();
        assert_eq!(
            deleted, 2,
            "one old tick_sample row + one old grid_sample row"
        );

        let remaining = store.query_ticks(ts(0), ts(20_000), None).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].ts, ts(10_000));
    }

    #[test]
    fn test_prune_before_no_matches_returns_zero() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        store
            .append_tick_samples(&[TickSample {
                ts: ts(10_000),
                asset_id: "ev".into(),
                power_kw: 1.0,
                soc_pct: None,
                temperature_c: None,
            }])
            .unwrap();
        let deleted = store.prune_before(ts(100)).unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_query_ticks_time_range_exclusive_upper_bound() {
        let store = SqliteHistoryStore::in_memory().unwrap();
        store
            .append_tick_samples(&[TickSample {
                ts: ts(1000),
                asset_id: "ev".into(),
                power_kw: 1.0,
                soc_pct: None,
                temperature_c: None,
            }])
            .unwrap();
        let rows = store.query_ticks(ts(0), ts(1000), None).unwrap();
        assert!(rows.is_empty(), "upper bound `to` must be exclusive");
        let rows = store.query_ticks(ts(0), ts(1001), None).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn test_open_creates_file_and_reopen_preserves_data() {
        let dir = std::env::temp_dir().join(format!("ven_history_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.sqlite");
        let path_str = path.to_str().unwrap();

        {
            let store = SqliteHistoryStore::open(path_str).unwrap();
            store
                .append_grid_sample(&GridSample {
                    ts: ts(42),
                    import_kw: 1.0,
                    export_kw: 0.0,
                    import_tariff_eur_kwh: None,
                    export_tariff_eur_kwh: None,
                    co2_g_kwh: None,
                })
                .unwrap();
        }
        {
            let store = SqliteHistoryStore::open(path_str).unwrap();
            let rows = store.query_grid(ts(0), ts(1000)).unwrap();
            assert_eq!(rows.len(), 1, "data must survive reopening the same file");
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_append_notification_roundtrip_and_since_filter() {
        use crate::entities::design_vocabulary::UserNotificationSeverity;
        use crate::entities::notification::UserNotification;
        let store = SqliteHistoryStore::in_memory().unwrap();
        let old = UserNotification::new(
            ts(100),
            UserNotificationSeverity::Warn,
            "VTN unreachable",
            None,
            None,
        );
        let new = UserNotification::new(
            ts(200),
            UserNotificationSeverity::Alert,
            "grid emergency",
            Some("ev".into()),
            Some("evt-1".into()),
        );
        store.append_notification(&old).unwrap();
        store.append_notification(&new).unwrap();

        let all = store.query_notifications(None, 100, None).unwrap();
        assert_eq!(
            all,
            vec![old.clone(), new.clone()],
            "roundtrip, oldest first"
        );

        let since = store.query_notifications(Some(ts(150)), 100, None).unwrap();
        assert_eq!(since, vec![new], "since filter keeps only newer rows");

        let pruned = store.prune_before(ts(150)).unwrap();
        assert_eq!(pruned, 1, "prune covers the notifications table");
    }

    #[test]
    fn test_update_notification_seen_bumps_count_and_last_seen() {
        use crate::entities::design_vocabulary::UserNotificationSeverity;
        use crate::entities::notification::UserNotification;
        let store = SqliteHistoryStore::in_memory().unwrap();
        let n = UserNotification::new(
            ts(100),
            UserNotificationSeverity::Alert,
            "storage error",
            None,
            None,
        )
        .with_dedup_key("storage-error");
        store.append_notification(&n).unwrap();

        store.update_notification_seen(n.id, 2, ts(400)).unwrap();

        let rows = store.query_notifications(None, 10, None).unwrap();
        assert_eq!(rows.len(), 1, "dedup hit must not add a row");
        assert_eq!(rows[0].count, 2);
        assert_eq!(rows[0].last_seen_at, ts(400));
        assert_eq!(rows[0].created_at, ts(100), "first occurrence preserved");
        assert_eq!(rows[0].message, "storage error");

        let missing = store.update_notification_seen(uuid::Uuid::new_v4(), 2, ts(500));
        assert!(
            matches!(missing, Err(DomainError::NotFound { .. })),
            "unknown id must surface NotFound, got {missing:?}"
        );
    }

    #[test]
    fn test_migrate_v4_backfills_count_and_last_seen_from_created_at() {
        // Build a v3 database by hand (pre-030 state) with one notification,
        // then let from_connection run the v4 migration against it.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(schema::SCHEMA_V1).unwrap();
        conn.execute_batch(schema::SCHEMA_V2).unwrap();
        conn.execute_batch(schema::SCHEMA_V3).unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn.execute(
            "INSERT INTO notifications (id, created_at, severity, message, asset_id, event_id)
             VALUES (?1, ?2, 'WARN', 'VTN unreachable', NULL, NULL)",
            params![uuid::Uuid::new_v4().to_string(), 12345_i64],
        )
        .unwrap();

        let store = SqliteHistoryStore::from_connection(conn).expect("v3→v4 migration");
        let rows = store.query_notifications(None, 10, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].count, 1, "existing rows backfilled with count 1");
        assert_eq!(
            rows[0].last_seen_at, rows[0].created_at,
            "last_seen_at backfilled from created_at"
        );
        assert_eq!(rows[0].dedup_key, None);
    }

    #[test]
    fn test_settings_put_get_delete_roundtrip() {
        use crate::controller::SettingsPort;
        let store = SqliteHistoryStore::in_memory().unwrap();
        assert_eq!(store.get_setting("comfort_curve", "ev").unwrap(), None);

        store
            .put_setting("comfort_curve", "ev", "[{\"fill\":0.8}]", ts(100))
            .unwrap();
        assert_eq!(
            store.get_setting("comfort_curve", "ev").unwrap().as_deref(),
            Some("[{\"fill\":0.8}]")
        );

        // Upsert replaces the value for the same (key, asset_id).
        store
            .put_setting("comfort_curve", "ev", "[{\"fill\":0.9}]", ts(200))
            .unwrap();
        assert_eq!(
            store.get_setting("comfort_curve", "ev").unwrap().as_deref(),
            Some("[{\"fill\":0.9}]")
        );

        store
            .put_setting("comfort_curve", "heater", "[]", ts(300))
            .unwrap();
        let mut all = store.settings_for_key("comfort_curve").unwrap();
        all.sort();
        assert_eq!(all.len(), 2, "one row per asset");

        assert!(store.delete_setting("comfort_curve", "ev").unwrap());
        assert!(!store.delete_setting("comfort_curve", "ev").unwrap());
        assert_eq!(store.get_setting("comfort_curve", "ev").unwrap(), None);
    }

    #[test]
    fn test_query_notifications_returns_newest_n_oldest_first() {
        use crate::entities::design_vocabulary::UserNotificationSeverity;
        use crate::entities::notification::UserNotification;
        let store = SqliteHistoryStore::in_memory().unwrap();
        for (i, msg) in ["a", "b", "c"].iter().enumerate() {
            store
                .append_notification(&UserNotification::new(
                    ts(100 * (i as i64 + 1)),
                    UserNotificationSeverity::Info,
                    *msg,
                    None,
                    None,
                ))
                .unwrap();
        }
        // limit 2 must keep the NEWEST two (b, c), oldest first — not (a, b).
        let got = store.query_notifications(None, 2, None).unwrap();
        let msgs: Vec<_> = got.iter().map(|n| n.message.as_str()).collect();
        assert_eq!(msgs, vec!["b", "c"]);
    }
}
