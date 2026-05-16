# Implementation Plan: Clean Timeline Infra Imports

**Branch**: `027-clean-timeline-infra` | **Date**: 2026-05-15 | **Spec**: [spec.md](spec.md)

## Summary

Remove three infra-ring imports from `controller/timeline.rs` (VG-03 from
`docs/plans/ven_backend_architecture_refactoring_v2.md` Phase 2). Currently
`TimelineAssetData` embeds `AssetConfig`, `AssetHistoryBuffer`, and `AssetState` —
all infra types — making `build_asset_timeline` untestable without a live simulator.

The fix: define domain-only types (`TimelinePoint`, updated `TimelineAssetData`,
updated `TimelineSnapshot`) inside `controller/timeline.rs`. Move `HeaterPlanTrajectory`
(pure maths struct) from `assets/heater.rs` to the domain ring. Update
`simulator/mod.rs::to_timeline_snapshot()` to perform all infra→domain conversions
before releasing the sim lock.

## Technical Context

**Language/Version**: Rust stable 2021 edition  
**Primary Dependencies**: `chrono`, `std::collections::HashMap` (all existing — no new Cargo.toml entries)  
**Storage**: N/A — no persistence changes  
**Testing**: `cargo test` (unit); Python behave BDD on Pi4-Server docker (integration)  
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2  
**Project Type**: VEN backend service (Rust/axum)  
**Performance Goals**: No change — `to_timeline_snapshot()` already clones full history; mapping to `Vec<TimelinePoint>` is equivalent cost  
**Constraints**: `controller/timeline.rs` ≤ 500 lines (pre-existing violation documented below); `simulator/mod.rs` must stay ≤ 500 lines after changes  
**Scale/Scope**: 3 files changed; ~50–80 lines modified net

## Constitution Check

### Principle I — OpenADR Spec Fidelity
✅ PASS — No API field names change. The VTN report interface and OpenADR field names are untouched.

### Principle II — BDD-First Testing
⚠️ CONDITIONAL — No new behavior is introduced; this is a pure structural refactoring.
Existing BDD scenarios for `GET /timeline/{asset_id}` serve as the regression safety net.
No new feature files are needed; existing tests must remain green.
**Action**: Run BDD suite on Pi4-Server after each phase and verify all timeline scenarios pass.

### Principle III — Upstream Compatibility
✅ PASS — Changes are in `VEN/src/` only; no impact on `openleadr-rs` submodule.

### Principle IV — Lean Architecture
✅ PASS — No new abstractions introduced. Moving `HeaterPlanTrajectory` to domain is a
straight struct relocation (no new traits, no new indirection). The design replaces
complex infra-type manipulation with pre-computed plain values.

### Principle V — Infrastructure Parity
✅ PASS — BDD runs on Pi4-Server via SSH as always. No Docker config changes.

### Principle VI — VEN Backend Hexagonal Architecture
✅ THIS IS THE FEATURE — Closes VG-03. After this change:
```bash
grep "use crate::assets" VEN/src/controller/timeline.rs   # → empty
grep "use crate::simulator" VEN/src/controller/timeline.rs # → empty
```

## Project Structure

### Documentation (this feature)

```text
specs/027-clean-timeline-infra/
├── plan.md          ← this file
├── spec.md          ← feature specification
├── research.md      ← Phase 0 findings
├── data-model.md    ← Phase 1 domain type designs
├── quickstart.md    ← developer guide
├── checklists/
│   └── requirements.md
└── tasks.md         ← Phase 2 (created by /speckit.tasks)
```

### Source Code

```text
VEN/src/
├── controller/
│   └── timeline.rs          ← MODIFIED: remove infra imports; add TimelinePoint,
│                                         move HeaterPlanTrajectory here;
│                                         update TimelineAssetData, TimelineSnapshot;
│                                         rewrite build_now_point + build_asset_timeline
├── assets/
│   └── heater.rs            ← MODIFIED: remove HeaterPlanTrajectory struct + next_slot
│                                         (moved to timeline.rs)
└── simulator/
    └── mod.rs               ← MODIFIED: update to_timeline_snapshot() — map history
                                          buffers to Vec<TimelinePoint>, call
                                          state_values() per point, pre-compute
                                          recent_avg_power, build HeaterPlanTrajectory
```

**Structure Decision**: Single-crate backend change. Frontend and BFF are unaffected —
the `GET /timeline/{asset_id}` HTTP response shape is unchanged.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|--------------------------------------|
| `controller/timeline.rs` is 1316 lines (pre-existing) | Pre-dates this feature; file will shrink slightly but not to ≤ 500 | Full file split is a separate task; doing it here would expand scope significantly and risk regressions in the resampling/LOCF logic |

---

## Phase 0: Research (complete)

See [research.md](research.md) for full findings. Summary of resolved decisions:

1. **`TimelinePoint` carries `state_values: HashMap<String, f64>`** — pre-computed from
   `AssetConfig::state_values()` in `to_timeline_snapshot()`. Preserves SoC, temp_c,
   irradiance, etc. in the past timeline display without touching `AssetState` in domain.

2. **`HeaterPlanTrajectory` moves to `controller/timeline.rs`** — struct is pure
   arithmetic with 5 `f64` fields and one mutating method. No infra dependencies.
   Construction (reading heater config + state) inlined into `to_timeline_snapshot()`.

3. **`current_power_kw`** pre-computed as 60-second LOCF rolling average in
   `to_timeline_snapshot()`. Replaces `data.history.recent_avg_power()` call in
   `build_now_point`.

4. **`current_state_values`** pre-computed from `config.state_values(&entry.state)`.
   Replaces `data.config.state_values(&last.state)` in `build_now_point`.

5. **Grid asset** — `grid_history: AssetHistoryBuffer` → `Vec<TimelinePoint>` (no
   state_values — grid has no AssetConfig). New field `grid_current_kw: f64` added to
   `TimelineSnapshot` for the grid now-point.

6. **500-line violation** — pre-existing; not fixed here.

---

## Phase 1: Design (complete)

See [data-model.md](data-model.md) for full type definitions and call-site diffs.

### New / updated domain types (`controller/timeline.rs`)

```rust
pub struct TimelinePoint {
    pub ts:           DateTime<Utc>,
    pub power_kw:     f64,
    pub state_values: HashMap<String, f64>,
}

#[derive(Clone)]
pub struct HeaterPlanTrajectory {
    pub e_kwh:        f64,
    pub temp_min_c:   f64,
    pub thermal_mass: f64,
    pub q_dem_kw:     f64,
    pub e_max_kwh:    f64,
}
impl HeaterPlanTrajectory {
    pub fn next_slot(&mut self, p_heat_kw: f64, dt_h: f64) -> HashMap<String, f64> { ... }
}

pub struct TimelineAssetData {
    pub asset_id:             String,
    pub asset_type:           AssetType,
    pub history:              Vec<TimelinePoint>,
    pub current_power_kw:     f64,
    pub current_state_values: HashMap<String, f64>,
    pub plan_trajectory:      Option<HeaterPlanTrajectory>,
}

pub struct TimelineSnapshot {
    pub assets:          HashMap<String, TimelineAssetData>,
    pub grid_history:    Vec<TimelinePoint>,
    pub grid_current_kw: f64,
}
```

### Infra conversion (`simulator/mod.rs::to_timeline_snapshot`)

For each `AssetEntry`:
- Map full history buffer → `Vec<TimelinePoint>` with `state_values` pre-computed
- Compute `current_power_kw` via `recent_avg_power(60s, now)` (or latest fallback)
- Compute `current_state_values` from `config.state_values(&entry.state)`
- Build `plan_trajectory` from heater config/state match (inline construction)
- Derive `asset_type` from `AssetConfig` variant via `match`

For grid:
- Map `grid_asset.history` → `Vec<TimelinePoint>` (empty `state_values`)
- Read `grid_current_kw` from `grid_asset.history.latest().map(|p| p.power_kw).unwrap_or(0.0)`

### Domain call-site updates (`controller/timeline.rs`)

`build_now_point`:
- Replace `data.history.latest()` + `data.config.state_values()` + `recent_avg_power()`
  with reads from `data.current_power_kw` and `data.current_state_values`

`build_asset_timeline` — history:
- Replace `data.history.slice(back_window, now)` with `data.history.iter().filter(|p| p.ts >= past_start)`
- Replace `data.config.state_values(&p.state)` with `p.state_values.clone()`

`build_asset_timeline` — trajectory:
- Replace `d.config.plan_trajectory(&d.current_state)` with `d.plan_trajectory.clone()`

### Test updates

Existing unit tests (`#[cfg(test)]` in `timeline.rs`) must be updated to use the new
fixture helpers:
- `make_base_snap` / `make_ev_snap`: replace `AssetHistoryBuffer`, `AssetConfig`,
  `AssetState` construction with `Vec<TimelinePoint>` + `AssetType` + pre-computed values
- `make_timeline_snap`: construct `TimelineSnapshot` with new field layout
- All test assertions remain identical — only fixtures change

No `use crate::assets` imports in the test module after the change.

### Contracts

No external API contracts change. The HTTP response shape of `GET /timeline/{asset_id}`
is unchanged — only the internal `TimelineSnapshot` representation changes.

---

## Implementation Sequence

1. **Move `HeaterPlanTrajectory` to `controller/timeline.rs`** — add `Clone` derive,
   add `next_slot` method, remove `HeaterPlanTrajectory` from `assets/heater.rs`.

2. **Add domain types** to `controller/timeline.rs` — `TimelinePoint`, updated
   `TimelineAssetData`, updated `TimelineSnapshot`.

3. **Update `to_timeline_snapshot()`** in `simulator/mod.rs` — convert all infra types
   to domain types before returning.

4. **Update `build_now_point` and `build_asset_timeline`** — use pre-computed values;
   remove infra imports from `controller/timeline.rs`.

5. **Update unit tests** in `controller/timeline.rs` — replace infra fixture helpers
   with domain-only equivalents.

6. **Verify invariants** — run greps, `cargo test`, check line counts.

7. **Push and run BDD** on Pi4-Server.
