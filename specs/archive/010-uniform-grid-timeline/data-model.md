# Data Model: Uniform-Grid Timeline API

## Entities

### UniformGrid (internal, not serialized)

Computed once per request, shared across all assets.

| Field | Type | Description |
|-------|------|-------------|
| resolution_s | u64 | Bucket width in seconds |
| history_timestamps | Vec<DateTime> | Grid points from window start up to (but not including) `now`, snapped to round boundaries |
| future_timestamps | Vec<DateTime> | Grid points from just after `now` to window end, snapped to round boundaries |

Grid timestamps are snapped to multiples of `resolution_s` (e.g., resolution=10s gives `:00`, `:10`, `:20`).

### AssetTimelinePoint (unchanged — existing type)

Each entry in an asset's array:

| Field | Type | Description |
|-------|------|-------------|
| ts | DateTime (ISO 8601) | Timestamp for this point |
| values | Option<Map<String, f64>> | Metric values, or `null` if no data covers this point |

### Asset Array Structure

Each asset's output array is three segments concatenated in ascending time order:

```
[ history_grid_point, ..., now_point, future_grid_point, ... ]
  ────────────────────      ───────   ───────────────────────
  grid-aligned (LOCF)       exact     grid-aligned (step interp)
```

- **History grid points**: One per `history_timestamps` entry. Values are LOCF time-weighted mean of raw rows in `[ts, ts + resolution)`.
- **Now-point**: Single entry at exact server `now`. Values are the asset's instantaneous readings from the most recent history row. Not grid-aligned.
- **Future grid points**: One per `future_timestamps` entry. Values are step-interpolated from the plan slot covering the bucket's start timestamp. `null` if no plan slot covers the bucket.

### Grid Bucket (conceptual, not a separate struct)

A time interval `[grid_ts, grid_ts + resolution)` used during resampling:

- **History**: All raw rows with timestamps in the interval are LOCF time-weighted averaged.
- **Plan**: The plan slot whose `[start, end)` covers the bucket's start timestamp provides the value.
- **No data**: `values` is `null`.

## Response Shape (unchanged)

```
GET /timeline/all → Record<string, {ts: string, values: object | null}[]>
```

```json
{
  "ev": [
    {"ts": "2026-03-21T11:00:00Z", "values": {"power_kw": 2.1, "soc": 0.40}},
    {"ts": "2026-03-21T11:00:10Z", "values": {"power_kw": 2.3, "soc": 0.41}},
    {"ts": "2026-03-21T11:00:17Z", "values": {"power_kw": 2.4, "soc": 0.42}},
    {"ts": "2026-03-21T11:00:20Z", "values": {"power_kw": 3.0}},
    {"ts": "2026-03-21T11:00:30Z", "values": null}
  ],
  "battery": [
    {"ts": "2026-03-21T11:00:00Z", "values": {"power_kw": -0.5, "soc": 0.85}},
    {"ts": "2026-03-21T11:00:10Z", "values": {"power_kw": -0.8, "soc": 0.84}},
    {"ts": "2026-03-21T11:00:17Z", "values": {"power_kw": -0.9, "soc": 0.83}},
    {"ts": "2026-03-21T11:00:20Z", "values": {"power_kw": -1.0}},
    {"ts": "2026-03-21T11:00:30Z", "values": null}
  ]
}
```

In the example above (resolution=10s, now=11:00:17Z):
- Index 0-1: history grid points (`:00`, `:10`)
- Index 2: now-point (`:17` — not grid-aligned)
- Index 3-4: future grid points (`:20`, `:30`)

Key invariant: `ev[i].ts === battery[i].ts` for all `i`.

## Value Keys (unchanged)

| Key | Unit | Source (history) | Source (plan) |
|-----|------|-----------------|---------------|
| power_kw | kW | Sim tick snapshot | Slot allocation |
| cost_rate_eur_h | EUR/h | Sim tick snapshot | power_kw * import_tariff |
| co2_rate_g_h | g/h | Sim tick snapshot | power_kw * co2_g_kwh |
| import_limit_kw | kW | N/A | Slot cap (grid asset only) |
| export_limit_kw | kW | N/A | Slot cap (grid asset only) |
| soc | fraction | Sim tick snapshot (battery/EV) | N/A |

## State Transitions

None — this feature is a read-only transformation of existing data. No new persistent state.
