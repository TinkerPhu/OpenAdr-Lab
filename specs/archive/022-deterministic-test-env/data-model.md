# Data Model: Deterministic Test Environment

**Branch**: `022-deterministic-test-env`
**Date**: 2026-05-12

---

## Extended: SimInjectState (`VEN/src/state.rs`)

`SimInjectState` gains one new optional field:

| Field | Type | Behaviour | Default |
|-------|------|-----------|---------|
| `pv_plan_kw` | `Option<f64>` | Behaviour D — frozen forecast: when `Some(v)`, every MILP planning slot uses `v.max(0.0)` kW as PV generation; when `None`, existing irradiance model applies. Not cleared by the physics tick. | `None` |

The field is serialized/deserialized (Serde derive on `SimInjectState`). It is visible in `GET /sim/inject` responses and settable via `POST /sim/inject`.

**Updated struct sketch** (additions only):

```
SimInjectState {
    // ... existing fields unchanged ...
    pub pv_plan_kw: Option<f64>,   // NEW — planning forecast override
}
```

---

## Extended: PostSimInjectBody (`VEN/src/routes/sim.rs`)

One new field with standard `serde(default)` absent-means-no-change semantics:

| Field | JSON absent | JSON `null` | JSON number |
|-------|-------------|-------------|-------------|
| `pv_plan_kw` | no change | clear override (`None`) | set override (`Some(v)`) |

**Not included in `should_replan`** — consistent with `base_load_kw`.

---

## Updated call chain: `planning.rs` → `run_planner` → `build_milp_inputs`

| Site | Change |
|------|--------|
| `tasks/planning.rs` | Read `inject_snap.pv_plan_kw` after `inject_snap` is snapshotted; pass as `pv_forecast_override: Option<f64>` to `run_planner` inside `spawn_blocking` closure |
| `controller/milp_planner/mod.rs` (`run_planner`) | Add `pv_forecast_override: Option<f64>` parameter; forward to `build_milp_inputs` |
| `controller/milp_planner/inputs.rs` (`build_milp_inputs`) | Add `pv_forecast_override: Option<f64>` parameter; in the per-slot PV loop, check override first |

**`build_milp_inputs` PV slot logic** (pseudocode):

```rust
let pv_kw = if let Some(forced_kw) = pv_forecast_override {
    forced_kw.max(0.0)   // constant for all slots; deterministic
} else if let Some(pv_snap) = assets.assets.get("pv") {
    // ... existing natural + decayed_offset logic unchanged ...
    (natural + decayed_offset).clamp(0.0, 1.0) * rated_kw
} else {
    pv_cfg.map(|c| c.forecast_kw(slot_t)).unwrap_or(0.0)
};
```

---

## New BDD Step Vocabulary (`tests/features/steps/phase_a_physics_steps.py`)

| Step | Parameters | HTTP call | Notes |
|------|-----------|-----------|-------|
| `I set pv plan forecast to {kw:f} kW` | `kw: float` | `POST /sim/inject {"pv_plan_kw": kw}` → 204 | Sets planning forecast override; does not affect physics tick |

**Usage in `deviation_absorber.feature` Background**:

```gherkin
Background:
  Given the VEN is running with the test profile
  And the absorber is enabled
  And I inject pv irradiance 0.0 via sim inject
  And I set pv plan forecast to 0.0 kW         ← NEW
```

---

## Assertion threshold: pre-discharge ≤ 0.1 kW

When `pv_plan_kw=0.0` is active, the MILP solver produces near-zero battery dispatch (no PV headroom incentive with flat tariffs). BDD assertions checking headroom availability must verify:
- Battery pre-discharge in plan: `≤ 0.1 kW`
- Battery headroom for absorber: `≥ 1.5 kW`
