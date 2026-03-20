## Refactoring

These items improve internal consistency and architecture without changing external behaviour.
Each is a prerequisite for future feature work as noted.

### RF-01 — Asset Interface: implement forecast() and past() on each asset
**What:** Each asset type (PV, battery, EV, heater, base load) must implement the
`AssetInterface` trait: `current()`, `forecast(timespan)`, `past(timespan)`.
**Why:** The planner currently contains a standalone `pv_forecast()` function
(`planner.rs:561–573`) that duplicates PV physics. This violates the single-responsibility
principle and will break when a real sensor replaces the simulator — the planner would
still use the wrong formula.
**Changes required:**
- Implement `predict()` properly on `PvInverter`, `Battery`, `EvCharger`, `Heater`, `BaseLoad`
  (currently all stubs returning single-point `(now, current_power)`)
- Remove `pv_forecast()` from `planner.rs`; replace call sites with `asset.forecast(ts)`
- Implement `past(window)` on each asset backed by its `EnergyCounter` / power profile ring buffer
**Prerequisite for:** real sensor integration (RF-03), timeline visualisation accuracy

### RF-02 — Flatten simulator/assets/ into assets/
**What:** Move `VEN/src/simulator/assets/{ev,heater,pv,battery,base_load}` into a top-level
`VEN/src/assets/` directory. Each asset module owns its physics model, forecast logic,
simulation state, and `/sim` parameter types.
**Why:** The current layout implies simulation is a global concern. After RF-01, each asset
*is* its own simulator. The `simulator/` wrapper becomes redundant. The target layout is:
```
VEN/src/assets/
  pv/        — PvAsset (irradiation model, forecast, past, sim params)
  battery/   — BatteryAsset
  ev/        — EvAsset
  heater/    — HeaterAsset
  base_load/ — BaseLoadAsset
  mod.rs     — AssetInterface trait + AssetEntry + Vec<AssetEntry> SimState
```
**Prerequisite for:** RF-01, clean addition of MeasuredAsset variants

### RF-03 — Asset type switches in user_request.rs → move into assets
**What:** `VEN/src/controller/user_request.rs` contains `match asset_type { ... }` blocks
that set default `CompletionPolicy`, `PostDeadlineComfortBid`, etc. per asset type.
**Why:** These defaults are asset-specific knowledge and belong inside each asset module.
The controller should ask the asset for its defaults, not hard-code them.
**Note:** Already tracked as a one-liner in the old BACKLOG below.

### RF-04 — min_soc not in state_values
**What:** `min_soc` is not written to `state_values`, so it always returns the default 0.10.
**Why:** Bug — user-configured `min_soc` is silently ignored at runtime.

### RF-05 — TimeSeries\<T\> abstraction + common/ module
**What:** Create `VEN/src/common/` as a plain Rust module (`mod common;` in `main.rs`) —
no separate crate, no workspace changes. Introduce `TimeSeries<T>` with a declared
`Interpolation` mode (`Step | Linear | None`) and operations: `at(ts)`, `resample(grid)`,
`merge(series)`, `bucket(width, agg)`, plus interval arithmetic helpers (`overlap()`,
`union_of_breakpoints()`, time-weighted average). Replace all ad-hoc lookup functions.
**Why:** The codebase has three independent strategies (exact-interval match in planner,
nearest-neighbour in UI, latest-snapshot in reporter) with no shared semantics. This causes
silent correctness bugs when signals of different types are mixed or when series have
different periods. See `VEN_ARCHITECTURE.md §5` for full audit. The `common/` module boundary
is drawn now so that when a VTN controller is built, extraction into a shared crate is a
rename operation with no API changes.
**Files to create:**
- `VEN/src/common/mod.rs`
- `VEN/src/common/timeseries.rs` — `TimeSeries<T>`, `Interpolation`, `Aggregator`
- `VEN/src/common/interval.rs` — `overlap()`, `union_of_breakpoints()`, time-weighted average
**Changes required:**
- Wrap `TariffSnapshot` series as `TimeSeries<f64>` with `Interpolation::Step`
- Wrap asset power history as `TimeSeries<f64>` with `Interpolation::Linear`
- Replace `tariff_import_at()`, `tariff_export_at()`, `tariff_co2_at()` in `planner.rs`
- Replace `findNearest()` + `buildStackedFromAllTimelines()` in `GridAccumulatedCell.tsx`
  with bucket aggregation: `mean` for power, `last` for states
- Replace single-snapshot report generation with interval-bucketed aggregation
**Grid-alignment rounding rule** (agreed during RF-01 spec, deferred here):
When `resample(interval)` is applied to a series anchored at `now`, timestamps MUST be
rounded to the interval grid boundary — not computed as `now ± n×interval`:
- `forecast(timespan).resample(interval)`: first point = `ceil(now, interval)` (next boundary)
- `past(timespan).resample(interval)`: last point = `floor(now, interval)` (last complete interval)
- Example: `now = 12:22`, `interval = 5 min` → forecast starts `12:25`, past ends `12:20`
This ensures series from different assets automatically share timestamps after resampling.
**Prerequisite for:** RF-06, accurate report generation, accurate UI stacked charts

### RF-06 — Planner slot costing: time-weighted tariff across slot boundaries
**What:** Replace `tariff_at(slot.start)` in `planner.rs:build_grid()` with a
time-weighted average over the full slot duration:
```
effective_tariff(slot) =
  Σ( tariff_i × overlap(slot, interval_i) ) / slot.duration
```
For capacity limits: `effective_limit(slot) = min(capacity_i for all overlapping intervals)`.
**Why:** A planning slot that spans a tariff boundary (e.g. a 5-min slot crossing 11:00 when
the hourly price changes) is silently billed at the wrong rate for part of the slot. At a
5-min planning resolution with 1-hour tariff periods the error is small but non-zero, and it
grows if planning resolution is coarser than the tariff granularity. Capacity limit errors
(using a prior-slot limit for the whole slot) can cause constraint violations.
**Prerequisite for:** accurate cost estimates in plan warnings and user notifications
**Depends on:** RF-05 (`TimeSeries<T>.resample()` makes this trivial to compute)

---

## General Backlog

clean up docker orphans

ven-1 differs in naming scheme from othe VENs. this causes confusion and sometimes errors. can we unify them?

make the ven-1 id a uuid and change it in all test and seed references.

DB-level optimization for active event filter: add `ends_at timestamptz` computed column + index so the `?active=true` filter can run in SQL instead of post-filtering in Rust. Not needed until event tables grow large.


Add a filter in VTN UI event table to omit the past events.

Add a DB-Reset script so it can be re-seeded easily.


add a setup script that docker composes all required containers.


add code coverage tools to tests and formater and linter tools to be applied for each code change.


check and remove warnings in all builds.

check for code quality and refactoring possibilities.

write down all your findings to the test errors around VEN UI simulation tests into ven_ui_simulation_test_issues.md. 

The fix is there. Docker's layer cache is stale — it doesn't see the change to Simulation.tsx. Need to force a rebuild without cache


add time provider for simulation: 
pub trait TimeContext: Clone + Send + Sync + 'static {
    type Instant: Copy + Ord + Send + 'static;

    fn now(&self) -> Self::Instant;
    fn sleep_until(&self, deadline: Self::Instant) -> Pin<Box<dyn Future<Output = ()> + Send>>;
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>>;

    fn pause(&self);
    fn resume(&self);
    fn set_rate(&self, rate: f64);
    fn advance(&self, delta: Duration);
}


how can I test the ven controller in ui?


also add ui tests for UserRequests and Controller in VEN\ui\src\__tests__   


the ven poll interval should be configurable in the config file so during test we can easily shorten it. or is there a better option? 

reactor still there?
