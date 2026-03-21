# Quickstart: Planner Slot Costing — Configurable Aggregation

## What changed

`TimeSeries::resample_uniform()` now takes an `Aggregation` parameter (Mean, Min, or Max)
that controls how values within each resampling bucket are combined.

## Files modified

| File | Change |
|------|--------|
| `VEN/src/common/mod.rs` | Added `Aggregation` enum, `bucket_min`/`bucket_max`/`bucket_extreme` methods, updated `resample_uniform` signature |
| `VEN/src/controller/planner.rs` | Updated 4 call sites to pass `Aggregation::Mean`, added `Aggregation` to imports |

## Usage

```rust
use crate::common::{Aggregation, TimeSeries};

// Tariff resampling (time-weighted mean — blended cost across slot)
let tariff_map = tariffs.import_eur_kwh
    .resample_uniform(slot_dur, Aggregation::Mean);

// Capacity limit resampling (min — strictest limit in slot)
let cap_map = capacity_series
    .resample_uniform(slot_dur, Aggregation::Min);
```

## How to verify

```bash
cd VEN
cargo test common::tests      # 41 tests including 5 new min/max tests
cargo test planner::tests     # 7 tests — unchanged behavior with Mean
```

## Next steps

- **RF-05b**: Convert scalar `import_limit_kw` / `export_limit_kw` to `TimeSeries` in the
  planner, then call `resample_uniform(slot_dur, Aggregation::Min)` on them.
- **RF-05e**: Reporter resampling may use `Aggregation::Mean` for power quantities and
  point-sampling for SoC.
