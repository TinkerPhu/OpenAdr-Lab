# Research: Uniform-Grid Timeline API

## R1: Response Shape — Keep or Change?

**Decision**: Keep the existing `Record<string, {ts, values}[]>` response shape unchanged.

**Rationale**: The core problem is timestamp misalignment across assets caused by per-asset `downsample()` picking different stride indices. The fix is to resample all assets onto the same uniform grid — but the output format doesn't need to change. Every asset array gets the same `ts` values at each position. This avoids a breaking change and means RF-05d (UI cleanup) only needs to remove `findNearest` and use positional indexing.

**Alternatives considered**:
- New envelope with `grid_timestamps` + `assets` map: Factoring out shared timestamps saves ~30% payload but introduces a breaking structural change. The payload savings don't justify forcing a UI-side format migration. Rejected.

## R2: Now-Point — How to Include?

**Decision**: Include a single now-point per asset as an entry in the array, positioned between the history grid portion and the future grid portion. The now-point has `ts` = exact server `now` and `values` = instantaneous asset readings from the most recent history row.

**Rationale**: The uniform grid snaps to round boundaries, so `now` almost never falls on a grid point. Without a now-point the UI would need to interpolate between the two nearest grid points — but it doesn't know the aggregation/interpolation method. The server owns the data and should provide the exact value. Placing the now-point between history and future preserves ascending sort order naturally.

**Alternatives considered**:
- No now-point (UI interpolates): Rejected — UI doesn't know the interpolation method.
- Anchor grid at `now`: Breaks the deterministic grid guarantee (same resolution always produces the same grid timestamps regardless of call time). Rejected.
- `now_point` as separate response field: Changes the response shape. Rejected.
- Append now-point at end of array: Breaks ascending sort order. Rejected.

## R3: Grid Determinism — Round Boundary Snapping

**Decision**: Grid timestamps are snapped to round boundaries of the resolution. E.g., `resolution=10s` gives timestamps at exact multiples of 10 seconds (`:00`, `:10`, `:20`, ...). The grid is computed as `floor(t / resolution) * resolution` for the start, then stepping by `resolution`.

**Rationale**: This guarantees that two calls with the same `resolution` and overlapping windows produce identical grid timestamps for the overlapping portion. The grid is deterministic and cache-friendly.

## R4: History Bucket Aggregation Strategy

**Decision**: Time-weighted mean using last-observation-carried-forward (LOCF) within each bucket.

**Rationale**: History rows arrive at 1-second intervals from the sim tick loop. When multiple rows fall within a single grid bucket (e.g., 10-second resolution), LOCF ensures that if only one row exists early in a bucket, its value is carried forward through the bucket duration before averaging. This correctly weights sparse data.

**Alternatives considered**:
- Simple arithmetic mean: Doesn't account for uneven spacing within buckets. For 1-second source data at 10-30s resolution, practically equivalent but less correct for edge cases.
- Last value (snapshot): Loses information about intra-bucket variation.

## R5: Future (Plan) Bucket Strategy

**Decision**: Step interpolation — each bucket gets the value of the plan slot whose `[start, end)` interval covers the bucket's start timestamp.

**Rationale**: Plan slots are step functions (constant power allocation for a fixed duration, typically 300 seconds). Step interpolation preserves this semantic.

## R6: Resolution Auto-Calculation

**Decision**: `resolution_s = ceil(total_window_s / 300)`, clamped to `[1, total_window_s]`. Max grid points capped at 3600.

**Rationale**: 300 points is a good default for a ~1200px chart width. The 3600 cap prevents abuse.

## R7: max_points Backward Compatibility

**Decision**: Convert `max_points` to `resolution_s = ceil(total_window_s / max_points)`. When both `resolution` and `max_points` are specified, `resolution` wins.

**Rationale**: Existing UI code uses `max_points`. Simple conversion formula keeps both paths working during transition.

## R8: Null Handling for Empty Buckets

**Decision**: Grid entries with no data have `"values": null` in the JSON output. The `ts` field is always present (to maintain array alignment). This gives `{"ts": "2026-...", "values": null}`.

**Rationale**: The UI needs to distinguish "no data recorded yet" from "data exists but is zero". JSON `null` for values is the standard representation, and recharts handles it correctly (gap in the line/area).
