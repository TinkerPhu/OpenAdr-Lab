# Data Model: 027-clean-timeline-infra

## Domain Types (new / modified in `controller/timeline.rs`)

### `TimelinePoint` (new)

Replaces the combined use of `HistoryPoint` + `AssetConfig::state_values()` at the
domain layer. All fields are pre-computed at the infra boundary before the sim lock is
released.

```
TimelinePoint {
    ts:           DateTime<Utc>          // timestamp of the reading
    power_kw:     f64                    // power at this instant
    state_values: HashMap<String, f64>   // asset-specific overlay values:
                                         //   Battery/EV: {"soc", "capacity_kwh"/"plugged"}
                                         //   Heater:     {"temp_c", "max_kw"}
                                         //   PV:         {"irradiance", "rated_kw"}
                                         //   BaseLoad:   {"baseline_kw"}
                                         //   Grid:       {} (empty — grid has no AssetConfig)
}
```

### `HeaterPlanTrajectory` (moved from `assets/heater.rs` to `controller/timeline.rs`)

Pure-maths stateful trajectory computer. No infra dependencies. Moved to domain ring
so `TimelineAssetData` can hold it without touching `assets/`.

```
HeaterPlanTrajectory {
    e_kwh:       f64   // current stored thermal energy above temp_min_c [kWh]
    temp_min_c:  f64   // minimum tank temperature [°C]
    thermal_mass: f64  // thermal mass [kWh/°C]
    q_dem_kw:    f64   // forecast demand power [kW]
    e_max_kwh:   f64   // maximum stored energy [kWh]
}

Methods:
  next_slot(p_heat_kw: f64, dt_h: f64) -> HashMap<String, f64>
    // advances e_kwh, returns {"temp_c": <value>}
```

Construction of `HeaterPlanTrajectory` from infra types moves to `to_timeline_snapshot()`
in `simulator/mod.rs` (infra layer — allowed to read `AssetConfig::Heater` and
`AssetState::Heater`).

### `TimelineAssetData` (updated)

Replaces current `{ history: AssetHistoryBuffer, config: AssetConfig, current_state: AssetState }`
with a domain-only bundle.

```
TimelineAssetData {
    asset_id:             String                        // e.g. "ev-1"
    asset_type:           AssetType                    // from entities/asset.rs (domain)
    history:              Vec<TimelinePoint>            // pre-computed domain points
    current_power_kw:     f64                          // 60s LOCF avg (for now-point)
    current_state_values: HashMap<String, f64>         // for now-point state overlay
    plan_trajectory:      Option<HeaterPlanTrajectory> // None unless asset is a Heater
}
```

**Removed fields**: `history: AssetHistoryBuffer`, `config: AssetConfig`,
`current_state: AssetState`.

### `TimelineSnapshot` (updated)

```
TimelineSnapshot {
    assets:           HashMap<String, TimelineAssetData>  // per-asset domain bundles
    grid_history:     Vec<TimelinePoint>                  // grid history (power_kw only)
    grid_current_kw:  f64                                 // current grid power for now-point
}
```

**Changed field**: `grid_history: AssetHistoryBuffer` → `Vec<TimelinePoint>`.
**New field**: `grid_current_kw: f64` for `build_now_point("grid", ...)`.

---

## Infra Changes (`simulator/mod.rs` — `to_timeline_snapshot()`)

The function remains in `simulator/mod.rs` (infra). Its body changes to perform all
type conversions before returning:

```
For each AssetEntry:
  1. Map entry.history.slice(FULL_WINDOW, now) → Vec<TimelinePoint>
       ts        = point.ts
       power_kw  = point.power_kw
       state_values = config.state_values(&point.state)   ← infra call, allowed here

  2. current_power_kw = entry.history.recent_avg_power(60s, now)
                          .unwrap_or(entry.history.latest().map(|p| p.power_kw).unwrap_or(0.0))

  3. current_state_values = config.state_values(&entry.state)  ← current live state

  4. plan_trajectory = match (&entry.config, &entry.state) {
       (AssetConfig::Heater(cfg), AssetState::Heater(s)) => Some(HeaterPlanTrajectory {
           e_kwh:       ((s.temperature_c - cfg.temp_min_c) * cfg.thermal_mass_kwh_per_c)
                            .clamp(0.0, e_max_kwh),
           temp_min_c:  cfg.temp_min_c,
           thermal_mass: cfg.thermal_mass_kwh_per_c,
           q_dem_kw:    cfg.forecast_demand_kw(cfg.ambient_temp_c),
           e_max_kwh:   (cfg.temp_max_c - cfg.temp_min_c) * cfg.thermal_mass_kwh_per_c,
       }),
       _ => None,
     }

  5. Build TimelineAssetData { asset_id, asset_type (from config variant), history,
                               current_power_kw, current_state_values, plan_trajectory }

For grid_asset:
  6. grid_history = grid_asset.history.slice(FULL_WINDOW, now).map(|p| TimelinePoint {
       ts: p.ts, power_kw: p.power_kw, state_values: HashMap::new()
     })
  7. grid_current_kw = grid_asset.history.latest().map(|p| p.power_kw).unwrap_or(0.0)
```

**Note on history window**: `to_timeline_snapshot()` currently clones the full
`AssetHistoryBuffer` (up to 3600 entries). After the change it maps the full buffer
to `Vec<TimelinePoint>`. The `slice(back_window, now)` filtering still happens inside
`build_asset_timeline` using `filter(|p| p.ts >= window_start)`.

---

## Call-site Changes in `controller/timeline.rs`

### `build_now_point`

Before:
```rust
let last = data.history.latest();
let values = data.config.state_values(&last.state);
let power_kw = data.history.recent_avg_power(60s, now).unwrap_or(last.power_kw);
```

After:
```rust
let power_kw = data.current_power_kw;
let mut values = data.current_state_values.clone();
values.insert("power_kw".into(), power_kw);
```

### `build_asset_timeline` — history section

Before:
```rust
data.history.slice(back_window, now).into_iter()
    .map(|p| {
        let mut values = data.config.state_values(&p.state);
        values.insert("power_kw".into(), p.power_kw);
        AssetTimelinePoint { ts: p.ts, values }
    })
```

After:
```rust
data.history.iter()
    .filter(|p| p.ts >= past_start)
    .map(|p| {
        let mut values = p.state_values.clone();
        values.insert("power_kw".into(), p.power_kw);
        AssetTimelinePoint { ts: p.ts, values }
    })
```

### `build_asset_timeline` — trajectory section

Before:
```rust
let mut plan_traj = snap.assets.get(asset_id)
    .and_then(|d| d.config.plan_trajectory(&d.current_state));
```

After:
```rust
let mut plan_traj = snap.assets.get(asset_id)
    .and_then(|d| d.plan_trajectory.clone());
// (HeaterPlanTrajectory is now in domain ring — Clone derive added)
```

---

## Invariant Verification Greps (post-implementation)

```bash
grep "use crate::assets" VEN/src/controller/timeline.rs         # → empty
grep "use crate::simulator" VEN/src/controller/timeline.rs      # → empty
grep "HeaterPlanTrajectory" VEN/src/assets/heater.rs            # → only construction helper remains (if any)
wc -l VEN/src/simulator/mod.rs                                  # → ≤ 500
```

## Pre-existing Violations (not fixed in this feature)

| File | Issue | Lines |
|------|-------|-------|
| `controller/timeline.rs` | Exceeds 500-line limit | 1316 |

This pre-dates feature 027. The file will shrink slightly after the refactoring but
will still exceed 500 lines. A dedicated file-split task should follow.
