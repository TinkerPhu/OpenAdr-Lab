# Quickstart: Reporter Multi-Interval Resampling (RF-05e)

## What this feature does

Changes the VEN's measurement reporter from emitting a single data point per report to emitting one data row per obligation interval. For example, if an event's reportDescriptor says "report every 15 minutes", the reporter now produces 4 rows per hour instead of 1 snapshot.

## Files to modify

1. **`VEN/src/controller/reporter.rs`** — Add `history_to_timeseries()` helper and `build_measurement_report_for_obligation()`. Keep existing `build_measurement_report()` for backward compatibility.

2. **`VEN/src/main.rs`** — Wire the obligation fulfillment loop (line ~496) to call the new reporter function and submit reports via VTN client.

3. **`tests/features/reporter_resampling.feature`** — BDD scenarios for multi-interval reports.

4. **`tests/steps/reporter_steps.py`** — Step definitions for report verification.

## Key design decisions

- **Two report paths**: Timer-driven (existing, single snapshot) for events without reportDescriptors. Obligation-driven (new, multi-interval) for events with reportDescriptors.
- **Resampling**: Power uses `resample_uniform()` (time-weighted mean). SoC uses `resample_to_grid()` (point-in-time at interval end).
- **Import/export split**: Sum all assets into net site power, resample, then clamp per bucket (positive=import, abs(negative)=export).

## How to test

```bash
# Unit tests (cargo)
cd VEN && cargo test reporter

# BDD integration tests
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/reporter_resampling.feature
```

## Dependencies

- RF-05a (`TimeSeries::resample_uniform()`) must be merged — already on main.
- `OadrReportObligation` with `interval_duration_s` — already in codebase.
