# API Contract: Timeline Endpoints (Uniform Grid)

## GET /timeline/all

Returns all asset timelines resampled onto a shared uniform time grid with a now-point.

**Response format is unchanged** — `Record<string, {ts, values}[]>`.

### Query Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| hours_back | f64 | 1.0 | Hours of history to include |
| hours_forward | f64 | 1.0 | Hours of future plan to include |
| resolution | u64 | auto | Grid bucket width in seconds. Auto = `ceil(total_window_s / 300)` |
| max_points | usize | - | **Deprecated**. Converted to `resolution = ceil(total_window_s / max_points)`. Ignored if `resolution` is also set. |

### Response (200 OK)

Each asset's array has three segments in ascending time order:

```
[ ...history_grid_points, now_point, ...future_grid_points ]
```

Example (resolution=10s, now=11:00:17Z):

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
  ],
  "grid": [
    {"ts": "2026-03-21T11:00:00Z", "values": {"power_kw": 1.6, "cost_rate_eur_h": 0.32}},
    {"ts": "2026-03-21T11:00:10Z", "values": {"power_kw": 1.5, "cost_rate_eur_h": 0.30}},
    {"ts": "2026-03-21T11:00:17Z", "values": {"power_kw": 1.5, "cost_rate_eur_h": 0.30}},
    {"ts": "2026-03-21T11:00:20Z", "values": {"power_kw": 2.0, "cost_rate_eur_h": 0.40}},
    {"ts": "2026-03-21T11:00:30Z", "values": null}
  ]
}
```

### Key Invariants

- `A.length === B.length` for every pair of assets A and B.
- `A[i].ts === B[i].ts` for all `i` — positional alignment.
- Array is sorted ascending by `ts`.
- Grid portion timestamps are snapped to round boundaries of the resolution — deterministic across calls.
- Now-point `ts` is the exact server `now` — not grid-aligned, sits between history and future grid portions.

### Segment Details

**History grid points** (ts < now, grid-aligned):
- LOCF time-weighted mean of raw rows in `[ts, ts + resolution)`.
- `values: null` if no raw data falls in the bucket.

**Now-point** (ts = now, not grid-aligned):
- Instantaneous values from the most recent history row (last sim tick).
- Always present, always has values (not null).

**Future grid points** (ts > now, grid-aligned):
- Step interpolation from plan slot whose `[start, end)` covers the bucket timestamp.
- `values: null` if no plan slot covers the bucket.

### NaN Handling

Individual value keys that are NaN are omitted from the `values` map (unchanged from current behavior). A bucket with all-NaN values is treated as empty (`values: null`).

---

## GET /timeline/:asset_id

Returns the grid-aligned timeline with now-point for a single asset.

### Query Parameters

Same as `GET /timeline/all`.

### Response (200 OK)

```json
[
  {"ts": "2026-03-21T11:00:00Z", "values": {"power_kw": 2.1, "soc": 0.40}},
  {"ts": "2026-03-21T11:00:10Z", "values": {"power_kw": 2.3, "soc": 0.41}},
  {"ts": "2026-03-21T11:00:17Z", "values": {"power_kw": 2.4, "soc": 0.42}},
  {"ts": "2026-03-21T11:00:20Z", "values": {"power_kw": 3.0}},
  {"ts": "2026-03-21T11:00:30Z", "values": null}
]
```

### Response (404 Not Found)

```json
{"error": "unknown asset: xyz"}
```

---

## Backward Compatibility

- **Response shape**: Unchanged (`Record<string, {ts, values}[]>` for `/all`, `[{ts, values}]` for `/:asset_id`).
- **`max_points` parameter**: Still accepted, converted to `resolution` internally.
- **`ts` values**: Now uniformly spaced (grid portions) with a single now-point. Previously independently stride-downsampled — values will differ.
- **`values: null`**: New for empty grid buckets. Previously, empty buckets were absent (shorter arrays). UI code consuming the arrays must handle `null` values.
- **Array length**: Now consistent across all assets (previously could differ due to independent downsampling).
