//! Versioned DDL for the history SQLite store, applied stepwise via
//! `PRAGMA user_version` in `history_store::migrate`.

pub(super) const SCHEMA_VERSION: i64 = 10;

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

/// WP4.3 (BL-20): user-facing notification feed persistence.
pub(super) const SCHEMA_V2: &str = "
CREATE TABLE notifications (
    id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    severity TEXT NOT NULL,
    message TEXT NOT NULL,
    asset_id TEXT,
    event_id TEXT
);
CREATE INDEX idx_notifications_ts ON notifications(created_at);
";

/// WP4.2 (BL-19): per-asset user settings (first consumer: comfort-curve overrides).
pub(super) const SCHEMA_V3: &str = "
CREATE TABLE user_settings (
    key TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (key, asset_id)
);
";

/// 030 (notification-dedup): repeats with the same dedup_key inside the
/// rolling window collapse into one row via count/last_seen_at. ADD COLUMN
/// requires a constant default, so last_seen_at is backfilled from
/// created_at in a second statement.
pub(super) const SCHEMA_V4: &str = "
ALTER TABLE notifications ADD COLUMN dedup_key TEXT;
ALTER TABLE notifications ADD COLUMN count INTEGER NOT NULL DEFAULT 1;
ALTER TABLE notifications ADD COLUMN last_seen_at INTEGER NOT NULL DEFAULT 0;
UPDATE notifications SET last_seen_at = created_at WHERE last_seen_at = 0;
CREATE INDEX idx_notifications_last_seen ON notifications(last_seen_at);
";

/// pv-curtailment-history: records the applied PV export limit and its source (plan vs. live
/// capacity) per tick sample. Both nullable — `NULL` means no limit was commanded during the
/// window, not a sentinel value. No `ADD COLUMN ... DEFAULT` needed since both are nullable with
/// no non-null default required (existing rows simply read back as `NULL`, correctly meaning
/// "unknown/not recorded before this migration", not "no limit").
pub(super) const SCHEMA_V5: &str = "
ALTER TABLE tick_samples ADD COLUMN export_limit_kw REAL;
ALTER TABLE tick_samples ADD COLUMN curtailment_source TEXT;
";

/// pv-generation-limit-rename: renames the persisted column to match the PV-asset-level rename
/// (`export_limit_kw` → `generation_limit_kw`) applied application-wide — the column name was a
/// mislabel of a device-level PV output cap as a site-level grid export quantity, which the PV
/// inverter has no visibility into. `RENAME COLUMN` is safe on the bundled rusqlite (SQLite ≥
/// 3.45, well past the 3.25 minimum for this syntax).
pub(super) const SCHEMA_V6: &str = "
ALTER TABLE tick_samples RENAME COLUMN export_limit_kw TO generation_limit_kw;
";

/// R-63: plan_snapshots was dead code — its only writer (`append_plan_snapshot`) was never
/// called from any production path (only unit tests and the mock port), so `GET /history/plans`
/// and the VEN UI's "Plans" panel were permanently, silently empty. No replacement mechanism —
/// see `docs/architecture/VEN_ARCHITECTURE.md` §4.9a (SCHEMA_V8 below) for a later, narrower
/// mechanism that explicitly considered and rejected reviving this table for its own purposes.
pub(super) const SCHEMA_V7: &str = "
DROP TABLE plan_snapshots;
";

/// forecast-accuracy-tracking: near/far forecast samples for PV, base_load, and site-residual,
/// reconciled with the real value once `target_ts` elapses. `actual_kw`/`actual_at` start NULL
/// and are filled in by `history_sampler`'s per-minute flush — see design.md Decision 4.
pub(super) const SCHEMA_V8: &str = "
CREATE TABLE forecast_accuracy_samples (
    asset_id TEXT NOT NULL,
    lead_kind TEXT NOT NULL,
    target_ts INTEGER NOT NULL,
    predicted_kw REAL NOT NULL,
    predicted_at INTEGER NOT NULL,
    actual_kw REAL,
    actual_at INTEGER
);
CREATE INDEX idx_forecast_accuracy_target ON forecast_accuracy_samples(asset_id, target_ts);
";

/// history-envelope-persistence: the Dynamic Operating Envelope's capacity-limit schedule
/// (`IMPORT_CAPACITY_LIMIT`/`EXPORT_CAPACITY_LIMIT`), persisted alongside the tariff fields
/// already on `grid_samples`. Tightest value observed within the minute window, not a mean —
/// see `history_sampler/accumulator.rs`'s `GridAcc`. `NULL` means "no capacity event was
/// applicable during this window", not zero.
pub(super) const SCHEMA_V9: &str = "
ALTER TABLE grid_samples ADD COLUMN import_limit_kw REAL;
ALTER TABLE grid_samples ADD COLUMN export_limit_kw REAL;
";

/// GB-25: per-plan-cycle solve-quality history — solve time, solver outcome, the
/// configured MIP-gap proxy (see `controller::milp_planner::types::MIP_GAP_TARGET`), and a
/// diagnostic cost/warning summary. `warning_kinds` is a comma-joined TEXT column, narrower
/// than a join table (see `entities::history::PlanHistorySample`'s doc comment). Deliberately
/// not a revival of the dropped `plan_snapshots` table (R-63, SCHEMA_V7 above) — narrow typed
/// columns instead of a full plan JSON blob, following the `forecast_accuracy_samples`
/// pattern (SCHEMA_V8) — see `docs/architecture/VEN_ARCHITECTURE.md` §4.9a.
pub(super) const SCHEMA_V10: &str = "
CREATE TABLE plan_history (
    plan_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    trigger TEXT NOT NULL,
    solver_ms INTEGER,
    solve_status TEXT NOT NULL,
    objective_eur REAL NOT NULL,
    friction_eur REAL NOT NULL,
    mip_gap_target REAL,
    warning_count INTEGER NOT NULL,
    warning_kinds TEXT NOT NULL DEFAULT '',
    c_energy_eur REAL,
    c_grid_eur REAL,
    c_wear_eur REAL,
    c_violations_eur REAL,
    c_peak_penalty_eur REAL
);
CREATE INDEX idx_plan_history_created_at ON plan_history(created_at);
";
