# Data Model: Planner Slot Costing — Configurable Aggregation

## Entities

### Aggregation (new)

An enumeration controlling how values within a resampling bucket are combined.

| Variant | Semantics | Use case |
|---------|-----------|----------|
| Mean    | Time-weighted average across the bucket | Tariffs, prices, CO2 intensity |
| Min     | Lowest value at any point in the bucket | Capacity limits (strictest applies) |
| Max     | Highest value at any point in the bucket | Peak demand tracking (future) |

### TimeSeries (modified)

Existing entity — ordered sequence of `(DateTime<Utc>, f64)` samples with an
`Interpolation` mode (Step or Linear).

**Modified method**:
- `resample_uniform(width, agg)` — now accepts an `Aggregation` parameter

**New private methods**:
- `bucket_min(start, end)` — minimum signal value over `[start, end)`
- `bucket_max(start, end)` — maximum signal value over `[start, end)`
- `bucket_extreme(start, end, pick)` — shared logic, parameterized by reduction function

## Relationships

```
Aggregation ──used by──▶ TimeSeries::resample_uniform()
                              │
                    ┌─────────┼─────────┐
                    ▼         ▼         ▼
               Mean path  Min path  Max path
               (TWM)      (bucket   (bucket
                           _min)     _max)
```

## State Transitions

None — `Aggregation` is a stateless enum. No lifecycle or persistence.

## Validation Rules

- `resample_uniform` requires `width > 0` and non-empty samples (unchanged).
- `Aggregation` is an exhaustive enum — no invalid states possible.
