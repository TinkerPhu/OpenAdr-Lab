# Sim Inject Redesign — Phased Implementation Plan

> **Status**: Design — implementation not yet started.

## Problem

`POST /sim/override` currently mutates asset **config** fields (device specs such as
`max_charge_kw`, thermostat bounds) on every sim tick. This is wrong for two reasons:

1. **Config is device specification**, not runtime state. Mutating it at runtime pollutes the
   planner's view of what the hardware can do.
2. **The planner never sees the injected condition as real state.** Overrides are applied after
   `build_setpoints()`, so the planner still plans against stale state. Injecting into *physics
   state* is the only way to make the planner reason from the correct starting point.

The existing `POST /sim/reset/:asset_id` already does the right thing for SoC — it mutates
`entry.state` directly via `cfg.reset()`. The redesign generalises that pattern to all injectable
state and environment fields.

---

## Three Injection Behaviours

| Behaviour | Description | Fields |
|---|---|---|
| **A — Jump + free evolution** | Write to physics state once; physics drives it from there. No persistent override held between ticks. | `battery_soc`, `ev_soc`, `heater_temp_c` |
| **B — Frozen + exponential return** | Hold injected value each tick while active. On release (null), blend back to auto model via EMA: `s(n+1) = s(n)*(1−α) + model(n+1)*α`. | `pv_irradiance` |
| **C — Frozen + snap return** | Hold injected value each tick while active. On release, snap back to profile default or no-limit immediately. | `ev_plugged`, `ev_departure_min`, `heater_setpoint_c`, `ambient_temp_c`, `base_load_kw`, `grid_import_limit_kw`, `grid_export_limit_kw` |

### New API endpoint

`POST /sim/inject` replaces `POST /sim/override`. Partial-merge semantics:
- Field **absent** → no change to current state
- Field **set to value** → activate override
- Field **set to null** → release override (triggers return behaviour)

```json
{
  "battery_soc": 0.1,
  "ev_soc": 0.4,
  "ev_plugged": false,
  "ev_departure_min": 120,
  "heater_temp_c": 16.5,
  "heater_setpoint_c": 19.0,
  "ambient_temp_c": 2.0,
  "pv_irradiance": 0.0,
  "pv_irradiance_alpha": 0.05,
  "base_load_kw": 3.5,
  "grid_import_limit_kw": 5.0,
  "grid_export_limit_kw": 3.0
}
```

`POST /sim/inject/reset` — releases all active overrides at once (used by BDD test teardown).

`POST /sim/override` is kept as a backward-compat alias during migration, then removed.

---

## New Structs

### `SimInjectState` — stored in `AppState` (`VEN/src/state.rs`)

```rust
pub struct SimInjectState {
    // Behaviour A (applied once then cleared)
    pub battery_soc: Option<f64>,
    pub ev_soc: Option<f64>,
    pub heater_temp_c: Option<f64>,
    // Behaviour B (frozen + EMA return)
    pub pv_irradiance: Option<f64>,
    pub pv_irradiance_alpha: f64,          // default 0.1
    // Behaviour C (frozen + snap)
    pub ev_plugged: Option<bool>,
    pub ev_departure_min: Option<f64>,     // minutes from now
    pub heater_setpoint_c: Option<f64>,
    pub ambient_temp_c: Option<f64>,
    pub base_load_kw: Option<f64>,
    pub grid_import_limit_kw: Option<f64>,
    pub grid_export_limit_kw: Option<f64>,
}
```

### `PvSmoothingState` — stored on `SimState` (`VEN/src/simulator/mod.rs`)

```rust
pub struct PvSmoothingState {
    pub current_irradiance: f64,   // tracks EMA-blended value between ticks
}
```

Marked `#[serde(skip)]` — ephemeral, resets to 0.0 on restart (EMA quickly recovers to natural
model).

---

## Implementation Groups

Work is split into four groups. Each group ends with a compilable, deployable, BDD-passing state.
Phases 2+3 must be committed together (Phase 2 breaks BDD; Phase 3 restores it).

---

## Group A — Backend Core (Phases 1–3)

**Goal:** Replace config mutations with correct state injection. All existing BDD tests pass at
the end of this group.

### Phase 1 — New structs + stub route (zero behaviour change)

Files changed:

| File | Change |
|---|---|
| `VEN/src/state.rs` | Add `SimInjectState`; add `InnerState.inject_state` (`#[serde(skip)]`); add `inject_state()`, `set_inject_state()`, `clear_inject_field(&str)` on `AppState`; keep `UserOverrides` unchanged |
| `VEN/src/simulator/mod.rs` | Add `PvSmoothingState` on `SimState` (`#[serde(skip)]`, init 0.0 in `from_profile()`) |
| `VEN/src/routes/sim.rs` | Add stub `get_sim_inject` + `post_sim_inject` handlers (read/write `inject_state` only, no injection yet); keep `post_sim_override` unchanged |
| `VEN/src/routes/mod.rs` | Register `GET /sim/inject` + `POST /sim/inject` |

Test: `POST /sim/inject {}` → 204; `GET /sim/inject` → `{}`; full BDD suite passes.

---

### Phase 2 — Remove config mutations from `tick()`; wire injections (BDD temporarily breaks)

Files changed:

| File | Change |
|---|---|
| `VEN/src/simulator/mod.rs` | Remove `overrides: &UserOverrides` param; remove "Apply UserOverride config mutations" block (lines 273–310) and "Set env fields" block (lines 264–271); add PV smoothing logic (see below); new params: `pv_irradiance_override`, `pv_alpha`, `ambient_temp_c`, `base_load_kw`, `heater_setpoint_c`, `ev_plugged` |
| `VEN/src/loops.rs` | Read `inject_state` once per tick; apply Behaviour A one-shots via `cfg.reset()` + `find_asset_mut()` then `clear_inject_field`; compose `effective_capacity` with grid overrides; pass Behaviour C fields into `tick()`; remove old `overrides` read |
| `VEN/src/assets/base_load.rs` | Add `baseline_kw_profile: f64` (original profile value, set in `from_config()`); `tick()` uses override if set, else `baseline_kw_profile` |
| `VEN/src/controller/dispatcher.rs` | Add `heater_setpoint_override: Option<f64>` param; if heater has no plan allocation and override is set, insert clamped setpoint |

PV smoothing logic inside `tick()`:
```rust
let irradiance = if let Some(forced) = pv_irradiance_override {
    self.pv_smoothing.current_irradiance = forced;
    forced
} else {
    let blended = self.pv_smoothing.current_irradiance * (1.0 - pv_alpha)
        + natural_irradiance * pv_alpha;
    self.pv_smoothing.current_irradiance = blended;
    blended
};
```

Grid limit composition in `spawn_sim_tick` (before `build_setpoints`):
```rust
let mut effective_capacity = capacity_snap.clone();
if effective_capacity.import_limit_event_id.is_none() {
    effective_capacity.import_limit_kw = inject.grid_import_limit_kw
        .or(effective_capacity.import_limit_kw);
}
// same for export
```

Test: Unit tests for PV smoothing math. Unit test grid limit composition. BDD: override-dependent
tests fail (known — fixed in Phase 3).

---

### Phase 3 — Alias bridge: `POST /sim/override` → `SimInjectState`

Files changed:

| File | Change |
|---|---|
| `VEN/src/routes/sim.rs` | Rewrite `post_sim_override`: translate `UserOverrides` → `SimInjectState` (ev_plugged, pv_irradiance, ambient_temp_c, base_load_w→kw; drop removed fields); call `set_inject_state()`; send `PlanTrigger::AssetStateChange` via `trigger_tx`. Rewrite `get_sim_override`: translate `SimInjectState` back to `UserOverrides` shape for backward compat |
| `VEN/src/main.rs` or `AppCtx` | Ensure `trigger_tx: Arc<watch::Sender<PlanTrigger>>` is on `AppCtx`; add if missing |

Key compat requirements:
- Empty body `{}` → `SimInjectState::default()` → all overrides released (preserves reset behaviour
  in `sim_ui_steps.py`)
- `GET /sim/override` still returns `ev_plugged` (relied on by `controller_v2_steps.py`)

Test: Full BDD suite passes. Commit Phases 1+2+3 as one PR.

---

## Group B — New Inject Fields + Proper API (Phases 4–5)

**Goal:** Wire `ev_departure_min` into the planner; implement proper partial-merge API with null
semantics; clean up `control_schema()` on all assets.

### Phase 4 — `ev_departure_min` → planner deadline

Files changed:

| File | Change |
|---|---|
| `VEN/src/loops.rs` | In `spawn_planning`: read `inject_state().ev_departure_min`, compute `ev_departure_override = now + Duration::seconds(min * 60)` |
| `VEN/src/controller/planner.rs` | Add `ev_departure_override: Option<DateTime<Utc>>` to `run_planner()`; before planning loop, replace deadline on any non-terminal EV packet with the override value. Check `entities/energy_packet.rs` for exact deadline field name (`latest_end` or similar). |

Test: Unit test `run_planner()` with forced near-term departure → verify EV packet gets higher
urgency. New BDD scenario: "EV departure override shortens charge window".

---

### Phase 5 — Proper partial-update semantics + `control_schema()` cleanup

Files changed:

| File | Change |
|---|---|
| `VEN/src/routes/sim.rs` | Implement `PostSimInjectBody` with `Option<serde_json::Value>` per field (absent = no change, null = release, value = set); add `POST /sim/inject/reset` route |
| `VEN/src/assets/ev.rs` | `control_schema()`: remove `ev_desired_kw`, `ev_soc_target`; add `ev_departure_min` (NumberInput, 0–1440, "min") |
| `VEN/src/assets/heater.rs` | `control_schema()`: remove `heater_max_kw`, `heater_temp_min_c`, `heater_temp_max_c`; add `heater_setpoint_c` (Slider, temp_min–temp_max, "°C") |
| `VEN/src/assets/pv.rs` | `control_schema()`: remove `pv_force_export_limit_kw`; add `pv_irradiance_alpha` (Slider, 0.01–1.0, unitless) |
| `VEN/src/assets/base_load.rs` | `control_schema()`: add `base_load_kw` (NumberInput, 0–20, "kW") |
| `VEN/src/assets/battery.rs` | `control_schema()`: remove `battery_force_kw` (unimplemented) |

Test: Cargo tests for partial-merge logic. `GET /sim/schema` returns updated descriptors. BDD
still passes via alias.

---

## Group C — UI Update (Phase 6)

**Goal:** TypeScript types and ControllerV2 hooks switch to `/sim/inject`. `Simulation.tsx`
left on alias for now.

Files changed:

| File | Change |
|---|---|
| `VEN/ui/src/api/types.ts` | Add `SimInjectState` type (all fields optional + nullable); keep `UserOverrides` as deprecated alias |
| `VEN/ui/src/api/client.ts` | Add `getSimInject()` / `postSimInject()` |
| `VEN/ui/src/api/hooks.ts` | Add `useSimInject()` / `useSetSimInject()` (query key `["simInject", ...]`, partial-merge mutation) |
| `VEN/ui/src/pages/ControllerV2.tsx` | Switch to `useSimInject()` / `useSetSimInject()` |
| `VEN/ui/src/components/controller-v2/AssetRightSection.tsx` | Switch field names to `SimInjectState`; `ev_plugged` fallback still reads from `GET /sim` |
| `VEN/ui/src/__tests__/ControllerV2.test.tsx` | Add `useSimInject` / `useSetSimInject` mocks |

Test: `npm test` from `VEN/ui/` passes. BDD controller_v2 scenarios pass.

---

## Group D — BDD Migration + Cleanup (Phase 7)

**Goal:** All BDD tests use `/sim/inject`. Alias route and `UserOverrides` removed.

BDD steps to migrate:

| File | Step | Change |
|---|---|---|
| `tests/features/steps/uc_steps.py` | `ev_plugged: false/true` | → `POST /sim/inject` |
| `tests/features/steps/uc_steps.py` | `pv_irradiance: 1.0` | → `POST /sim/inject` |
| `tests/features/steps/uc_steps.py` | `ev_desired_kw: 0` | Remove (field was never applied) |
| `tests/features/steps/sim_ui_steps.py` | "reset overrides" | → `POST /sim/inject/reset` |
| `tests/features/steps/controller_v2_steps.py` | `GET /sim/override` | → `GET /sim/inject` |

Backend cleanup:

| File | Change |
|---|---|
| `VEN/src/routes/sim.rs` | Remove `post_sim_override` handler |
| `VEN/src/state.rs` | Remove `UserOverrides` struct, `overrides` field, `overrides()` / `set_overrides()` methods |
| `VEN/src/routes/mod.rs` | Remove `POST /sim/override` route |

UI cleanup:

| File | Change |
|---|---|
| `VEN/ui/src/api/types.ts` | Remove `UserOverrides` type |
| `VEN/ui/src/api/client.ts` | Remove `postSimOverride` / `getSimOverride` |
| `VEN/ui/src/api/hooks.ts` | Remove `useSimOverride` / `useSetSimOverride` |

Test: Full BDD suite passes with no references to `/sim/override`.

---

## Phase 8 — Simulation.tsx cleanup (optional, low priority)

Migrate `Simulation.tsx` `OverridableControl` usages from alias to `POST /sim/inject`. Remove
`GET /sim/override`. Deferred — no functional impact.

---

## Reusable Patterns

| Pattern | Location | Notes |
|---|---|---|
| One-shot state injection | `routes/sim.rs:35-63` (`POST /sim/reset/:asset_id`) | Exact pattern: `cfg.reset()` + `find_asset_mut()` |
| Grid limit enforcement | `loops.rs:375`, `dispatcher.rs:build_setpoints()` | Compose before passing; VTN event wins if `import_limit_event_id.is_some()` |
| Replan trigger | `loops.rs:221` (`trigger_tx.send(PlanTrigger::RateChange)`) | Same pattern; use `PlanTrigger::AssetStateChange` |

## Verification

```bash
# BDD (Pi4-Server):
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner

# Rust unit tests (Pi4-Server via docker):
docker compose run --rm cargo-test cargo test --workspace --jobs 2

# UI unit tests (local):
cd /c/DriveD/Tinker/OpenAdr-Lab/VEN/ui && npm test
```

Passing criteria per group:
- **Group A**: all BDD passing; `POST /sim/override` behaviour unchanged
- **Group B**: `POST /sim/inject {"pv_irradiance": 0.5}` holds value; `GET /sim/schema` updated
- **Group C**: ControllerV2 controls update sim state without page reload; Vitest passes
- **Group D**: zero references to `/sim/override` in tests; all BDD passing
