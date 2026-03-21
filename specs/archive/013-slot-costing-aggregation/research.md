# Research: Planner Slot Costing — Configurable Aggregation

## R-01: Correct aggregation semantics per quantity type

**Decision**: Time-weighted mean for tariffs/prices, min for capacity limits, max reserved for future use.

**Rationale**: Tariff rates that change mid-slot need a blended cost proportional to
how long each rate was active — this is the time-weighted mean. Capacity limits must use
the strictest (lowest) value anywhere in the slot to prevent the planner from scheduling
power that would violate the limit during any sub-interval. Max aggregation has no
current consumer but is trivially symmetric to min and costs nothing to include.

**Alternatives considered**:
- Point-sampling at slot start: Incorrect — misses tariff changes within the slot.
- Simple arithmetic mean of sample values: Incorrect — ignores how long each value applies.
- Separate functions per aggregation: Rejected — bucket_min and bucket_max share identical
  logic (split-point enumeration) differing only by the reduction function.

## R-02: Min/max correctness for Step vs Linear interpolation

**Decision**: For both modes, evaluate the signal at the bucket start plus all interior
sample timestamps. For Linear only, also evaluate at the bucket end.

**Rationale**: Step (LOCF) signals are constant between change-points, so extremes can
only occur at change-point boundaries. Linear signals are piecewise monotonic between
samples, so extremes occur at segment endpoints. Checking the bucket-end for Linear
captures the final segment's endpoint; for Step it's unnecessary since the value
doesn't change until the next sample.

**Alternatives considered**:
- Sampling at fixed sub-intervals within the bucket: Approximate, not exact.
- Analytical approach (solve for derivative zero): Unnecessary — linear segments are
  monotonic so no interior extrema exist.

## R-03: API design — parameter vs method overload

**Decision**: Add an `Aggregation` enum parameter to `resample_uniform()`.

**Rationale**: A single entry point with an explicit mode is clearer than three
separate methods (`resample_uniform_mean`, `resample_uniform_min`, etc.) and
scales to future aggregation modes without API proliferation.

**Alternatives considered**:
- Separate methods per aggregation: More code, harder to extend.
- Default parameter (mean if omitted): Rust doesn't have default parameters; a builder
  pattern would be over-engineering for a single optional config.
- Closure parameter: Too flexible — callers would need to understand the bucket structure.
  The enum constrains choices to semantically valid options.
