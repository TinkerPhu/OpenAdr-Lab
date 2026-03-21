# Data Model: Reporter Multi-Interval Resampling (RF-05e)

**Date**: 2026-03-21

## Existing Entities (no changes)

### OadrReportObligation

Already defined in `VEN/src/entities/capacity.rs`. Contains all fields needed by the reporter:

| Field | Type | Description |
|-------|------|-------------|
| id | UUID | Unique obligation identifier |
| event_id | String | OpenADR event this obligation belongs to |
| program_id | Option<String> | Program the event belongs to |
| payload_type | String | e.g. "USAGE", "STORAGE_CHARGE_STATE" |
| reading_type | String | e.g. "DIRECT_READ", "FORECAST" |
| resource_name | Option<String> | Target resource |
| due_at | DateTime<Utc> | When the report is due |
| interval_duration_s | u64 | **Key field**: resampling bucket width |
| fulfilled | bool | Whether report has been submitted |
| created_at | DateTime<Utc> | When obligation was created |

### TimeSeries

Already defined in `VEN/src/common/mod.rs`. Used for resampling:

| Field | Type | Description |
|-------|------|-------------|
| samples | Vec<(DateTime<Utc>, f64)> | Timestamped scalar values |
| interpolation | Interpolation (Step/Linear) | How to interpolate between samples |

Key methods used:
- `resample_uniform(width: Duration) -> TimeSeries` — time-weighted mean per bucket
- `resample_to_grid(timestamps: &[DateTime<Utc>]) -> TimeSeries` — point-in-time sampling

### AssetHistoryBuffer

Already defined in `VEN/src/controller/trace.rs`. Source of raw data:

| Field | Type | Description |
|-------|------|-------------|
| timestamps | VecDeque<DateTime<Utc>> | Row timestamps (1s tick rate) |
| columns | HashMap<String, VecDeque<f64>> | Named columns (power_kw, soc, etc.) |
| capacity | usize | Max rows (3600 = 1 hour at 1s ticks) |

## New Functions (no new entities)

### history_to_timeseries

Converts a single column from `AssetHistoryBuffer` into a scalar `TimeSeries`.

- Input: buffer reference, column name, interpolation mode, optional time window
- Output: `TimeSeries` with NaN rows excluded
- Location: `reporter.rs` (private helper)

### build_measurement_report_for_obligation

Builds a multi-interval report from an obligation and asset history.

- Input: `OadrReportObligation`, asset history map, VEN name
- Output: `Option<Value>` (JSON report with N interval entries)
- Location: `reporter.rs` (public function)

## Report Payload Structure

### Single-interval (current, preserved for fallback)

```
resources[0].intervals = [
  { id: 0, payloads: [{type, values}] }
]
```

### Multi-interval (new, for obligation-based reports)

```
resources[0].intervals = [
  { id: 0, intervalPeriod: {start, duration}, payloads: [{type, values}] },
  { id: 1, intervalPeriod: {start, duration}, payloads: [{type, values}] },
  ...
  { id: N, intervalPeriod: {start, duration}, payloads: [{type, values}] }
]
```

## Column-to-Payload Mapping

| Payload Type | History Column | Interpolation | Aggregation |
|-------------|---------------|---------------|-------------|
| USAGE (import) | power_kw (positive only) | Step | time-weighted mean via resample_uniform |
| USAGE (export) | power_kw (negative, abs) | Step | time-weighted mean via resample_uniform |
| STORAGE_CHARGE_LEVEL | soc | Step | point-in-time via resample_to_grid (interval end) |
| OPERATING_STATE | N/A | N/A | Always "ACTIVE" (constant per interval) |
