# Data Model: Backend Adoption of TimeSeries Resampling

**Feature**: 009-backend-timeseries-adoption
**Date**: 2026-03-21

## New Entity: TariffTimeSeries

A container holding three independent `TimeSeries` — one per tariff quantity — all with Step interpolation.

**Fields**:
| Field | Type | Description |
|-------|------|-------------|
| `import_eur_kwh` | TimeSeries (Step) | Import tariff series, one sample per interval start |
| `export_eur_kwh` | TimeSeries (Step) | Export tariff series, one sample per interval start |
| `co2_g_kwh` | TimeSeries (Step) | CO2 intensity series, one sample per interval start |

**Constructed from**: `Vec<TariffSnapshot>` at the OpenADR interface boundary.

**Conversion rule**: For each `TariffSnapshot`, if the quantity field is `Some(val)`, emit `(interval_start, val)` into the corresponding `TimeSeries`. Snapshots sorted by `interval_start`; duplicates resolved by last-write-wins.

**Lifecycle**: Created once per event-poll cycle. Passed to planner. Not persisted.

## Modified Entity: run_planner() Parameter

**Before**: `rates: &[TariffSnapshot]`
**After**: `tariffs: &TariffTimeSeries`

The planner resamples each series to its slot grid internally.

## Modified Entity: build_grid() Internal Flow

**Before**: Per-slot calls to `tariff_import_at()`, `tariff_export_at()`, `tariff_co2_at()`, `nearest_value()`.

**After**: Pre-loop resampling phase:
1. `tariffs.import_eur_kwh.resample_uniform(slot_width)` → HashMap<epoch_sec, f64>
2. `tariffs.export_eur_kwh.resample_uniform(slot_width)` → HashMap<epoch_sec, f64>
3. `tariffs.co2_g_kwh.resample_uniform(slot_width)` → HashMap<epoch_sec, f64>
4. For each asset forecast: `ts.resample_uniform(slot_width)` → HashMap<epoch_sec, f64>

Per-slot: lookup by `slot_start.timestamp()` in the HashMap, fallback to default.

## Removed Functions

| Function | File | Replacement |
|----------|------|-------------|
| `tariff_import_at()` | planner.rs:545-550 | HashMap lookup from resampled `import_eur_kwh` |
| `tariff_export_at()` | planner.rs:552-557 | HashMap lookup from resampled `export_eur_kwh` |
| `tariff_co2_at()` | planner.rs:559-564 | HashMap lookup from resampled `co2_g_kwh` |
| `nearest_value()` | planner.rs:572-600 | HashMap lookup from resampled asset forecast |

## Unchanged Entities

- **PlanTimeSlot**: No field changes. `import_tariff_eur_kwh`, `export_tariff_eur_kwh`, `co2_g_kwh` populated from resampled values instead of ad-hoc lookups.
- **TariffSnapshot**: Still used as the parsing output from `parse_rate_snapshots()`. Converted immediately to `TariffTimeSeries`.
- **PlannedTariffs**: Still used in `state.planned_tariffs()` for the tick loop (monitor cost attribution). Not affected by this change.
- **OadrCapacityState**: Unchanged — single snapshot, not a time series.
- **TimeSeries**: No changes to the struct or its methods (RF-05a deliverable, already complete).

## State Transitions

None. This feature is a pure refactor of data flow — no new states, no persistence changes, no new lifecycle events.
