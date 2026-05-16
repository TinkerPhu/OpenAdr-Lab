# Research: 027-clean-timeline-infra

## Finding 1: `state_values()` per asset type

**Question**: What does `AssetConfig::state_values(&state)` return for each variant?

**Answer** (from `VEN/src/assets/`):

| Asset | Keys returned |
|-------|--------------|
| Battery | `soc`, `capacity_kwh` |
| EV | `soc`, `plugged` |
| Heater | `temp_c`, `max_kw` |
| PV | `irradiance`, `rated_kw` |
| BaseLoad | `baseline_kw` |

**Implication**: `TimelinePoint` must carry these pre-computed state values so that
`build_asset_timeline` can include them in the output `AssetTimelinePoint.values` HashMap
without touching `AssetState` or `AssetConfig`. The conversion must happen in
`to_timeline_snapshot()` at the infra boundary.

---

## Finding 2: `plan_trajectory` is heater-only stateful physics

**Question**: How does `plan_trajectory` work and can it be decoupled from infra types?

**Answer** (from `VEN/src/assets/heater.rs`):

- Only `AssetConfig::Heater` returns a trajectory; all other variants return `None`.
- `HeaterPlanTrajectory` is a struct with 5 plain `f64` fields:
  `e_kwh`, `temp_min_c`, `thermal_mass`, `q_dem_kw`, `e_max_kwh`.
- `next_slot(&mut self, p_heat_kw: f64, dt_h: f64)` advances state and returns
  `{"temp_c": f64}` — pure arithmetic, no infra dependencies.
- `HeaterPlanTrajectory::new(cfg: &Heater, live_temp_c: f64)` computes the 5 fields
  from heater config + current temperature.

**Decision**: Move `HeaterPlanTrajectory` (struct + `next_slot`) to the domain ring
(inside `controller/timeline.rs`). The construction logic (`new()`) moves inline into
`simulator/mod.rs::to_timeline_snapshot()`, which remains in infra and is allowed to
read both `AssetConfig::Heater` and `AssetState::Heater`. `TimelineAssetData` stores
`plan_trajectory: Option<HeaterPlanTrajectory>` pre-computed at the infra boundary.

**Alternatives considered**:
- Fall back to `planned_state_by_asset` (spec-approved): rejected as first choice because it
  loses the live-state correction that makes the displayed heater curve start from the actual
  current temperature. This would be a visible regression in the heater timeline display.
- Keep `HeaterPlanTrajectory` in infra, store as `Box<dyn Trait>`: rejected as over-engineering —
  the struct has no infra imports and moves cleanly to domain.

---

## Finding 3: `recent_avg_power` — 60-second rolling LOCF average

**Question**: What does `AssetHistoryBuffer::recent_avg_power(window, now)` do and
how can `build_now_point` work without the ring buffer?

**Answer** (from `VEN/src/assets/mod.rs:202`):

- Computes LOCF time-weighted mean of `power_kw` values within a 60-second window.
- Returns `None` if no points exist (empty history).
- Falls back to `latest().power_kw` when all points are outside the window.

**Decision**: Pre-compute this value in `to_timeline_snapshot()` using the existing
`AssetHistoryBuffer::recent_avg_power(Duration::seconds(60), now)` call (still allowed in
infra). Store the result as `current_power_kw: f64` in `TimelineAssetData`.
`build_now_point` then reads `data.current_power_kw` directly without any ring buffer access.

For the grid asset: `grid_asset.history.latest().map(|p| p.power_kw).unwrap_or(0.0)` gives
the current grid power — store as a separate field in `TimelineSnapshot`.

---

## Finding 4: `AssetHistoryBuffer::latest()` for now-point state values

**Question**: `build_now_point` calls `data.history.latest()` to get the most recent state
for `state_values()`. How is this preserved?

**Answer**: `latest()` returns `Option<&HistoryPoint>`. `state_values()` is then called
on that point's state. These are the same state values as those in the most recent
`TimelinePoint` in the history Vec (since history is appended chronologically and sliced
from the end).

**Decision**: Store `current_state_values: HashMap<String, f64>` in `TimelineAssetData`,
pre-computed in `to_timeline_snapshot()` from the latest history point's state via
`config.state_values(&entry.state)` (using the live `entry.state` which is always the
most current — equivalent to `history.latest().state`).

---

## Finding 5: Pre-existing file size violation

**Question**: Does `controller/timeline.rs` satisfy the 500-line file limit?

**Answer**: No. It is currently **1316 lines** — 2.6× over the 500-line limit.
`simulator/mod.rs` is 441 lines (within limit; adding ~30 lines for conversion logic
brings it to ~470, still safe).

**Decision**: The 500-line violation in `timeline.rs` pre-dates this feature. This
feature will not make it worse (removing infra type machinery will shorten the file
slightly), but will not fully resolve it. File splitting is a follow-up task.
Document as a pre-existing violation in the plan's Complexity Tracking table.

---

## Summary: Design decisions

| Question | Decision | Rationale |
|----------|----------|-----------|
| `state_values` for history | Pre-compute in `to_timeline_snapshot()`, store in `TimelinePoint.state_values` | Only infra can call `AssetConfig::state_values()` |
| `plan_trajectory` (heater) | Move `HeaterPlanTrajectory` to domain (`controller/timeline.rs`) | Struct is pure math, no infra deps |
| `recent_avg_power` for now-point | Pre-compute in `to_timeline_snapshot()`, store as `current_power_kw` | Avoids ring buffer in domain |
| `latest()` state for now-point | Pre-compute `current_state_values` in infra | Equivalent to latest history point's state |
| Grid history | Map `grid_asset.history` to `Vec<TimelinePoint>` in `to_timeline_snapshot()` | Grid is a special-cased infra field |
| 500-line violation | Document as pre-existing; not fixed in this feature | Scope discipline |
