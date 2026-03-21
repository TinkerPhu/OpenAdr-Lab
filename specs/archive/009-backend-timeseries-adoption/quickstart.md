# Quickstart: Backend Adoption of TimeSeries Resampling

**Feature**: 009-backend-timeseries-adoption

## What This Feature Does

Replaces all ad-hoc per-slot tariff and forecast lookups in the VEN planner with pre-resampled `TimeSeries` arrays. After this change:

- Tariffs are converted from `TariffSnapshot` lists to three `TimeSeries` (import, export, CO2) at the OpenADR interface boundary
- The planner resamples all series to its slot grid **once** before the loop
- Each slot reads values by HashMap lookup instead of scanning the full tariff list
- Tariffs spanning slot boundaries are correctly time-weighted
- The reporter resamples asset history to obligation intervals

## Files Changed

| File | Change |
|------|--------|
| `VEN/src/controller/openadr_interface.rs` | Add `tariffs_to_timeseries()` conversion function |
| `VEN/src/controller/planner.rs` | Change signature to accept `TariffTimeSeries`; replace ad-hoc lookups with HashMap reads; remove 4 helper functions |
| `VEN/src/controller/reporter.rs` | Resample asset history to obligation interval |
| `VEN/src/entities/tariff_snapshot.rs` | Add `TariffTimeSeries` struct |
| `VEN/src/main.rs` | Convert tariffs before passing to planner |

## How to Test

```bash
# Unit tests (planner + conversion)
cd VEN && cargo test

# Full BDD suite on Pi4
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"
```

## Key Design Decisions

1. **Three separate TimeSeries per tariff** — not a single multi-valued series — because each quantity can have independent `None` gaps.
2. **HashMap<epoch_sec, f64> for slot lookup** — not positional indexing — because resampled grid may be offset from slot grid by one bucket.
3. **Conversion at interface boundary** — `TariffSnapshot` → `TariffTimeSeries` happens in the caller, not inside the planner.
4. **Capacity limits NOT converted** — `OadrCapacityState` stays as-is (single snapshot, no time variation).

## Prerequisites

- RF-05a (TimeSeries resampling operations) — already complete (commit ccac7c9)
