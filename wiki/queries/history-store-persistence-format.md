---
title: History store persistence format, time encoding, compaction, docker location
type: query
created: 2026-07-10
updated: 2026-07-10
synced_commit: 65afa27
sources: [VEN/src/history_store/schema.rs, VEN/src/history_store/mod.rs, VEN/src/tasks/history_sampler/accumulator.rs, VEN/src/main.rs, VEN/docker-compose.yml, VEN/src/profile/schema.rs, VEN/src/profile/defaults.rs]
tags: [history, sqlite, time, retention, docker]
---

# History store persistence format, time encoding, compaction, docker location

User asked: how is past data stored in SQLite, is there a special time format, is every
asset's time stored, is compaction foreseen, and where do the SQLite files live in docker.
Answered by reading the actual Phase 1 source directly (not just [[history-store]]'s prose)
since this is a load-bearing operational question.

## Schema and time format

Six tables (`VEN/src/history_store/schema.rs`, schema v1): `tick_samples`, `grid_samples`,
`plan_snapshots`, `events_received`, `reports_sent`, `ledger_periods`. All timestamp
columns are `INTEGER` — **Unix epoch seconds**, not milliseconds or ISO-8601 text.
Conversion is centralized in `VEN/src/history_store/mod.rs`:

```rust
fn to_unix(ts: DateTime<Utc>) -> i64 { ts.timestamp() }
fn from_unix(secs: i64) -> Result<DateTime<Utc>, DomainError> { Utc.timestamp_opt(secs, 0)... }
```

Every write/read round-trips through `chrono::DateTime<Utc>`; no sub-second resolution is
stored, which matches the 1-minute sampling cadence below.

## Per-asset storage — one row per asset per minute, not a raw per-second log

`tick_samples` has an `asset_id TEXT` column, so each 1-minute sample is one row per asset
(`ev`, `battery`, `heater`, …) sharing the same `ts`. System-wide grid aggregates
(`import_kw`, `export_kw`, tariffs, CO₂) live separately in `grid_samples`, one row per
minute (not per-asset, since they aren't asset-scoped quantities).

Critically, `power_kw`/`soc_pct`/`temperature_c` are **means over the minute**, not
instantaneous readings: `HistorySampler`
(`VEN/src/tasks/history_sampler/accumulator.rs`) accumulates every 1-second sim tick into
a running per-asset sum, and flushes to the DB only on a wall-clock minute rollover
(`now.timestamp() / 60 != previous_minute`). SOC/temperature means are computed only over
samples where that field was actually present, so an asset lacking `temp_c` doesn't drag
its mean toward zero.

## Compaction — flat retention, no multi-tier downsampling

There is no tiered compaction (e.g. keep raw for a week, hourly after a month). Retention
is a single cutoff, `HistoryConfig.retention_days` (`VEN/src/profile/schema.rs`,
defaulting to 90 in `VEN/src/profile/defaults.rs`), applied by
`SqliteHistoryStore::prune_before(cutoff)`: a `DELETE ... WHERE ts < ?` across all six
tables, followed by `PRAGMA wal_checkpoint(PASSIVE)` to reclaim WAL space without blocking
writers. This runs once per calendar day (the sampler's day-boundary check), not per
write. The whole store is optional: `profile.history.enabled: false` skips constructing
`SqliteHistoryStore` and spawning the sampler entirely.

## Docker location

`main.rs` derives `data_dir` from `PERSIST_PATH`'s parent (fallback `/data`) and opens
`{data_dir}/history.sqlite`. `VEN/docker-compose.yml` sets `PERSIST_PATH:
/data/state.json` per VEN and bind-mounts `./data/ven-N:/data`, so on Pi4 the files are at:

```
/srv/docker/openadr_lab/VEN/data/ven-1/history.sqlite  (and ven-2, ven-3)
```

A bind-mounted host directory (not a named Docker volume) — survives container rebuilds
and sits directly alongside the pre-existing `state.json`, directly inspectable from the
Pi4 host.

## Relation to [[history-store]]

The component page covers the same ground at a system/architecture level (why the port
exists, how it fits [[ven-hexagonal-architecture]]'s port discipline, the VTN-side
recorder counterpart). This query drills into the concrete on-disk format the user asked
about; no discrepancy found between the two — this page just cites the exact code.
