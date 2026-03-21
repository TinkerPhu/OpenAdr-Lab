## Refactoring

These items improve internal consistency and architecture without changing external behaviour.
Each is a prerequisite for future feature work as noted.

### RF-01 — Asset Interface: implement forecast() and history() on each asset
**What:** Each asset type (PV, battery, EV, heater, base load) must implement the
`AssetInterface` trait: `current()`, `forecast(timespan)`, `history(timespan)`.
**Why:** The planner currently contains a standalone `pv_forecast()` function
(`planner.rs:561–573`) that duplicates PV physics. This violates the single-responsibility
principle and will break when a real sensor replaces the simulator — the planner would
still use the wrong formula.
**Changes required:**
- Implement `predict()` properly on `PvInverter`, `Battery`, `EvCharger`, `Heater`, `BaseLoad`
  (currently all stubs returning single-point `(now, current_power)`)
- Remove `pv_forecast()` from `planner.rs`; replace call sites with `asset.forecast(ts)`
- Implement `history(window)` on each asset backed by its `EnergyCounter` / power profile ring buffer
**Prerequisite for:** real sensor integration (RF-03), timeline visualisation accuracy

### RF-01a - ~~rename past() to recordings()~~ resolved: renamed to history()
- Was too general a name. Renamed to `history()`, which pairs cleanly with `forecast()` and aligns with the HTTP endpoint `/history/:asset_id`. Implemented in speckit 007.

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

### RF-05a — TimeSeries resampling operations
**What:** Add resampling operations to the existing `TimeSeries` in `VEN/src/common/mod.rs`:
- `resample_uniform(width: Duration) -> TimeSeries` — resample onto a regular grid with
  the given step width. Interpolation uses the series' own `interpolation` field (Step = LOCF,
  Linear = proportional). Aggregation within each bucket is also determined by the
  interpolation mode (Step/Linear → time-weighted mean).
- `resample_to_grid(timestamps: &[DateTime<Utc>]) -> TimeSeries` — resample onto an
  arbitrary timestamp grid. Each output point is the interpolated value at that timestamp.
**Why:** The codebase has three independent lookup strategies (exact-interval match in planner,
nearest-neighbour in UI, latest-snapshot in reporter) with no shared semantics. This causes
silent correctness bugs when signals of different interpolation types are mixed or when
series have different periods. See `VEN_ARCHITECTURE.md §5` for full audit.
Adding operations to the existing `TimeSeries` avoids introducing a parallel container —
the struct already carries `samples`, `interpolation`, `quantity`, and `unit`.
**Grid-alignment rounding rule** (agreed during RF-01 spec):
When `resample_uniform(interval)` is applied to a series anchored at `now`, timestamps MUST be
rounded to the interval grid boundary — not computed as `now ± n×interval`:
- `forecast(timespan).resample_uniform(interval)`: first point = `ceil(now, interval)`
- `history(timespan).resample_uniform(interval)`: last point = `floor(now, interval)`
- Example: `now = 12:22`, `interval = 5 min` → forecast starts `12:25`, history ends `12:20`
This ensures series from different assets automatically share timestamps after resampling.
**Deliverable:** Methods on `TimeSeries` with comprehensive unit tests, no integration changes.
**Prerequisite for:** RF-05b, RF-05c

### RF-05b — Backend adoption of TimeSeries resampling
**What:** Replace all ad-hoc time-series lookup functions in backend Rust code with
`TimeSeries` resampling operations.
**Changes required:**
- Convert `TariffSnapshot` series into `TimeSeries` (Step interpolation) at the
  OpenADR interface boundary — one series per quantity (import, export, CO2)
- Replace `tariff_import_at()`, `tariff_export_at()`, `tariff_co2_at()` in `planner.rs`
  with `resample_uniform(slot_width)` called once before the slot loop
- Replace `nearest_value()` with the same resampled series lookup
- Replace single-snapshot report generation with `resample_uniform(obligation_interval)`
**Why:** After resampling all series to the planner's slot width, the slot loop becomes a
simple index lookup — no per-slot search, no interpolation bugs, and tariffs that span
slot boundaries are correctly time-weighted.
**Depends on:** RF-05a
**Prerequisite for:** RF-06, accurate report generation

### RF-05c — Backend: uniform-grid timeline API with now-point
**What:** Modify `GET /timeline/all` (and `GET /timeline/:asset_id`) to resample all assets
onto a shared uniform time grid with a now-point. The response format stays unchanged
(`Record<string, {ts, values}[]>`). Each asset's array is three segments concatenated in
ascending time order: (1) history grid points, (2) a single now-point, (3) future grid points.
**Why:** Investigation showed that history timestamps are already aligned across assets
(all pushed with the same `now` in the tick loop). Misalignment only occurs because
the per-asset `downsample()` stride in `get_timeline_all` picks different indices per asset.
The fix belongs in the API: return a shared grid so the UI needs no interpolation at all.
**New query parameter:**
- `resolution` (optional, seconds) — bucket width for the uniform grid. Default: auto-calculated
  from `hours_back + hours_forward` to target ~300 points. Replaces `max_points`.
**Response format (unchanged shape, new alignment guarantees):**
```json
{
  "ev": [
    {"ts": "2026-03-21T11:00:00Z", "values": {"power_kw": 2.1}},
    {"ts": "2026-03-21T11:00:10Z", "values": {"power_kw": 2.3}},
    {"ts": "2026-03-21T11:00:17Z", "values": {"power_kw": 2.4}},
    {"ts": "2026-03-21T11:00:20Z", "values": {"power_kw": 3.0}},
    {"ts": "2026-03-21T11:00:30Z", "values": null}
  ],
  "battery": [
    {"ts": "2026-03-21T11:00:00Z", "values": {"power_kw": -0.5}},
    {"ts": "2026-03-21T11:00:10Z", "values": {"power_kw": -0.8}},
    {"ts": "2026-03-21T11:00:17Z", "values": {"power_kw": -0.9}},
    {"ts": "2026-03-21T11:00:20Z", "values": {"power_kw": -1.0}},
    {"ts": "2026-03-21T11:00:30Z", "values": null}
  ]
}
```
In the example above (resolution=10s, now=11:00:17Z): index 0-1 are history grid points,
index 2 is the now-point (not grid-aligned), index 3-4 are future grid points.
**Key design decisions:**
- All assets share the same `ts` values at each index — the UI can index by position.
- Grid timestamps are snapped to round boundaries of the resolution (deterministic: same
  resolution + window = same grid, regardless of when the call is made).
- The now-point sits between history and future grid portions at the exact server `now`,
  preserving ascending sort order. It provides instantaneous values so the UI does not
  need to interpolate (the UI doesn't know the interpolation method).
- History buckets: aggregate via time-weighted mean (LOCF within bucket, then average).
- Future (plan) buckets: step interpolation — each bucket gets the plan slot value that covers
  its start timestamp.
- Empty buckets: `{"ts": "...", "values": null}` — no data available.
- `resolution` replaces `max_points`. `max_points` kept as deprecated alias.
- `/tariffs` is NOT resampled — tariffs are sparse step functions (1-10 points per 24h),
  render correctly as-is, and cost/CO₂ rates are already baked into each timeline point.
**Depends on:** RF-05a (TimeSeries resampling concepts)
**Prerequisite for:** RF-05d (UI cleanup)

### RF-05d — Frontend: remove findNearest, use grid-aligned API response
**What:** Update `GridAccumulatedCell.tsx` to consume the grid-aligned response from RF-05c.
Remove `findNearest()`, `TOLERANCE_MS`, and the tolerance-based matching logic.
**Changes required:**
- Replace `buildStackedFromAllTimelines` with a simple positional zip across assets
  (all arrays share the same indices and `ts` values — no lookup needed).
- Handle `values: null` entries from empty grid buckets (render as gaps in the chart).
- Remove `findNearest()` function entirely.
- Update or replace `GridAccumulatedCell.test.tsx` to test the new direct-index approach.
**Note:** No response shape change — the format is still `Record<string, {ts, values}[]>`.
The client/hook code does not need updating. The now-point is already inline in the array.
**Depends on:** RF-05c
**Prerequisite for:** accurate UI stacked charts

### RF-05e — Reporter adoption: multi-interval resampling for measurement reports
**What:** Refactor `build_measurement_report()` in `VEN/src/controller/reporter.rs` to
resample asset history to obligation intervals using `resample_uniform()`, producing one
report row per interval instead of a single latest-snapshot.
**Why:** Currently the reporter emits a single data point per report. With resampled history,
reports can cover multiple obligation intervals with correctly aggregated values.
**Complications identified (from RF-05b analysis):**
1. Obligation interval duration is not currently passed into the reporter — needs plumbing
   from the event's report descriptor
2. `AssetHistoryBuffer` returns multi-keyed snapshots (power, SoC, temperature), not scalar
   `TimeSeries` — needs per-asset-type conversion logic
3. Report JSON payload is hardcoded to a single interval — needs structural change to emit
   an array of interval payloads
4. EV SoC requires point-in-time sampling (not time-weighted mean) — different aggregation
   semantics than power quantities
5. Import/export split per interval requires sign-based partitioning of the resampled power
   series
**Depends on:** RF-05a, RF-05b (resampling infrastructure + planner adoption)
**Prerequisite for:** accurate multi-interval OpenADR measurement reports

### RF-06 — Planner slot costing: time-weighted tariff across slot boundaries
**What:** With RF-05b in place, tariff series are already resampled to the slot grid using
`resample_uniform(slot_width)`. Each resampled point is the time-weighted average across
that bucket — so slot costing is automatically correct. RF-06 becomes a verification task:
confirm that `resample_uniform` with Step interpolation produces the correct time-weighted
average when a slot spans a tariff boundary.
```
Example: slot [10:55, 11:00) with tariff [10:00=€0.20, 11:00=€0.15]
resample_uniform(5min) at 10:55 → €0.20 (entire bucket within €0.20 interval)
slot [10:57, 11:02) → (3min × €0.20 + 2min × €0.15) / 5min = €0.185
```
For capacity limits: `resample_uniform` with Step + min aggregation gives the strictest
limit that applies anywhere within each slot.
**Prerequisite for:** accurate cost estimates in plan warnings and user notifications
**Depends on:** RF-05b (resampling already in place)

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
