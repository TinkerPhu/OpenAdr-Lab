//! Schema v1 DDL for the history SQLite store, applied once via
//! `PRAGMA user_version` in `history_store::migrate`.

pub(super) const SCHEMA_VERSION: i64 = 1;

pub(super) const SCHEMA_V1: &str = "
CREATE TABLE tick_samples (
    ts INTEGER NOT NULL,
    asset_id TEXT NOT NULL,
    power_kw REAL NOT NULL,
    soc_pct REAL,
    temperature_c REAL
);
CREATE INDEX idx_tick_samples_ts ON tick_samples(ts);
CREATE INDEX idx_tick_samples_asset ON tick_samples(asset_id, ts);

CREATE TABLE grid_samples (
    ts INTEGER NOT NULL,
    import_kw REAL NOT NULL,
    export_kw REAL NOT NULL,
    import_tariff_eur_kwh REAL,
    export_tariff_eur_kwh REAL,
    co2_g_kwh REAL
);
CREATE INDEX idx_grid_samples_ts ON grid_samples(ts);

CREATE TABLE plan_snapshots (
    created_at INTEGER NOT NULL,
    horizon_start INTEGER NOT NULL,
    horizon_end INTEGER NOT NULL,
    plan_json TEXT NOT NULL
);
CREATE INDEX idx_plan_snapshots_created_at ON plan_snapshots(created_at);

CREATE TABLE events_received (
    received_at INTEGER NOT NULL,
    event_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL
);
CREATE INDEX idx_events_received_ts ON events_received(received_at);

CREATE TABLE reports_sent (
    sent_at INTEGER NOT NULL,
    report_type TEXT NOT NULL,
    event_id TEXT NOT NULL,
    payload_json TEXT NOT NULL
);
CREATE INDEX idx_reports_sent_ts ON reports_sent(sent_at);

CREATE TABLE ledger_periods (
    asset_id TEXT NOT NULL,
    period_start INTEGER NOT NULL,
    period_end INTEGER NOT NULL,
    energy_kwh REAL NOT NULL,
    cost_eur REAL NOT NULL,
    co2_kg REAL NOT NULL
);
CREATE INDEX idx_ledger_periods_asset ON ledger_periods(asset_id, period_start);
";
