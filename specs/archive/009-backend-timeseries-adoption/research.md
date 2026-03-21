# Research: Backend Adoption of TimeSeries Resampling

**Feature**: 009-backend-timeseries-adoption
**Date**: 2026-03-21

## R-01: Tariff-to-TimeSeries conversion strategy

**Decision**: Convert `Vec<TariffSnapshot>` into three separate `TimeSeries` (import, export, CO2) with Step interpolation. Conversion happens in a new function in `openadr_interface.rs` that takes `&[TariffSnapshot]` and returns a struct of three `TimeSeries`.

**Rationale**: Each tariff quantity can have independent `None` gaps (e.g., CO2 missing while import/export present). Separate series per quantity lets `resample_uniform()` handle gaps independently — a missing CO2 sample doesn't invalidate the import series. Step interpolation matches the OpenADR convention that a price holds until the next change.

**Alternatives considered**:
- Single TimeSeries with tuple values: rejected — `resample_uniform` operates on scalar `f64`, not tuples.
- Keep `TariffSnapshot` and add `resample` to it: rejected — would duplicate the resampling logic already in `TimeSeries`.

## R-02: Planner signature change

**Decision**: Change `run_planner()` parameter from `rates: &[TariffSnapshot]` to `tariffs: &TariffTimeSeries` where `TariffTimeSeries` is a new struct holding three `TimeSeries` (import, export, CO2). The conversion from `TariffSnapshot` to `TariffTimeSeries` happens in the caller (`main.rs` planning loop).

**Rationale**: The planner should receive data in the format it consumes (pre-resampled series), not the raw parsing output. The caller already owns the conversion point. Passing the pre-resampled struct makes the planner's slot loop a simple indexed read.

**Alternatives considered**:
- Pass three separate `TimeSeries` as individual params: rejected — clutters the signature (already 8 params).
- Pass raw `&[TariffSnapshot]` and convert inside `run_planner()`: rejected — keeps conversion coupled to planner instead of at the interface boundary.

## R-03: Slot-to-index mapping for resampled series

**Decision**: After `resample_uniform(slot_width)`, the resampled series has timestamps aligned to the slot grid. Build a `HashMap<DateTime<Utc>, f64>` or simply iterate the resampled `samples` vec and match by slot start timestamp. Since both the slot loop and the resampled output use the same grid, a direct positional index (slot `i` → resampled sample `i`) works when the series starts at or before the first slot.

**Rationale**: The slot loop creates slots starting at `now` with step `step_s`. `resample_uniform(step_s)` creates buckets at `ceil(first_sample, step_s)`. If the tariff series starts before `now`, the resampled output's first bucket may be before slot 0. The safest approach is to build a `HashMap<i64, f64>` keyed by epoch seconds (bucket start) and look up `slot_start.timestamp()`.

**Alternatives considered**:
- Direct positional indexing (`samples[i]`): rejected — fragile if resampled series and slot grid are offset by even one bucket.
- Binary search on sorted samples: works but HashMap is simpler and O(1).

## R-04: Asset forecast resampling point

**Decision**: Resample all asset forecasts to the slot grid in `build_grid()` before the slot loop, not in the caller. The planner already receives `&HashMap<String, TimeSeries>` — it calls `ts.resample_uniform(slot_width)` on each and builds per-asset lookup maps.

**Rationale**: The caller doesn't know the slot width (it's a planner-internal config). Resampling inside the planner keeps the knowledge of slot geometry encapsulated.

**Alternatives considered**:
- Resample in caller: rejected — caller would need to know `plan_step_s`, leaking planner internals.
- Resample lazily per slot: rejected — defeats the purpose of pre-resampling.

## R-05: Reporter resampling scope

**Decision**: For `build_measurement_report()`, resample each asset's history to the obligation interval using `history_from_buffer()` → `TimeSeries` → `resample_uniform(obligation_interval)`. Each resampled point becomes one report interval payload.

**Rationale**: Current reporter only emits the latest single value. OpenADR reports can carry multiple intervals. Resampling the history buffer to the obligation interval width produces the correct number of aligned data points.

**Alternatives considered**:
- Keep single-snapshot approach: viable short-term but inaccurate for longer obligation intervals.
- Aggregate manually: rejected — duplicates logic already in `TimeSeries::time_weighted_mean()`.

## R-06: rate_estimated flag preservation

**Decision**: The `rate_estimated` flag currently checks `rates.is_empty()`. After RF-05b, check whether all three tariff TimeSeries have empty samples instead. If all three are empty, `rate_estimated = true`.

**Rationale**: Preserves the existing semantics — "are we using default values because no VTN rates arrived?"

## R-07: Default value handling for gaps

**Decision**: When a resampled series has no value for a slot (slot timestamp not in the HashMap), use the existing default constants: `DEFAULT_IMPORT_PRICE`, `DEFAULT_EXPORT_PRICE`, `DEFAULT_CO2_G_KWH`. For asset forecasts, default to `0.0`.

**Rationale**: Maintains backward compatibility with current fallback behaviour.

## R-08: Capacity limits — not converting to TimeSeries

**Decision**: `OadrCapacityState` (import/export limits) remains a simple struct, NOT converted to TimeSeries. The capacity state is a single snapshot (not a time series) — it represents the current active limit, not a schedule.

**Rationale**: Capacity limits come from a single active event and apply uniformly to all slots. No time-weighted averaging needed. If future OpenADR events carry time-varying capacity schedules, this can be revisited as a separate feature.

**Alternatives considered**:
- Convert to TimeSeries for consistency: rejected — adds complexity for a field that has no time variation in current data model.
