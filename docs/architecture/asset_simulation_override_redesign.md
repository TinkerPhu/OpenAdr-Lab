# Asset Simulation — Inject API

> **Status**: Groups A, B, C implemented and BDD-green (207 scenarios, 1190 steps).
> Group D (BDD migration + alias cleanup) and Phase 8 (Simulation.tsx migration) pending.

---

## Why inject, not override

The old `POST /sim/override` mutated device **config** fields (specs such as `max_charge_kw`,
thermostat bounds) on every sim tick. This was wrong:

- Config is device specification, not runtime state. Mutating it at runtime pollutes the planner's
  view of hardware capability.
- The planner never saw the injected condition as real state — overrides were applied after
  `build_setpoints()`, so the planner still planned against stale state.

The replacement (`POST /sim/inject`) writes directly into **physics state** (`soc`, `temp_c`,
`plugged`) and **environment inputs** (`irradiance`, `ambient_temp_c`). Physics then evolves
naturally from the injected starting point, and the planner reasons from the corrected reality
on the very next tick.

---

## Three Injection Behaviours

| Behaviour | Description | Fields |
|---|---|---|
| **A — Jump + free evolution** | Written to physics state once. Auto-cleared after the next tick. Physics drives freely from there. | `battery_soc`, `ev_soc`, `heater_temp_c` |
| **B — Frozen + EMA blend-back** | Held at the injected value each tick while active. On release (`null`), blends back to the natural model via first-order IIR: `s(n+1) = s(n)*(1−α) + model(n+1)*α`. Converges when delta < 0.005. | `pv_irradiance` |
| **C — Frozen + snap return** | Held at the injected value each tick while active. On release (`null`), snaps back to profile default or no-limit immediately. | `ev_plugged`, `ev_departure_min`, `heater_setpoint_c`, `ambient_temp_c`, `base_load_kw`, `grid_import_limit_kw`, `grid_export_limit_kw` |

**Behaviour A note — startup lag**: `pv_irradiance_alpha` EMA is only active during blend-back
from an override. Normal operation uses the natural irradiance model directly (no ramp-up lag
on restart). Tracked via `PvSmoothingState.override_was_active: bool`.

---

## API Reference

### `GET /sim/inject`

Returns the current inject state. Fields that are not overridden are omitted (or `null`).

**Response** `200 application/json`:
```json
{
  "battery_soc": null,
  "ev_soc": null,
  "heater_temp_c": null,
  "pv_irradiance": 0.8,
  "pv_irradiance_alpha": 0.05,
  "ev_plugged": null,
  "ev_departure_min": 90.0,
  "heater_setpoint_c": null,
  "ambient_temp_c": null,
  "base_load_kw": null,
  "grid_import_limit_kw": null,
  "grid_export_limit_kw": null
}
```

---

### `POST /sim/inject`

Partial-merge update. Each field independently:
- **Absent** → no change to current state
- **`null`** → release override (triggers return behaviour for B/C; no-op for already-released fields)
- **Value** → activate override

**Request body** (all fields optional):
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

**Response** `204 No Content`.

Fires `PlanTrigger::AssetStateChange` after each call to trigger reactive replanning.

---

### `POST /sim/inject/reset`

Releases all active overrides at once. Equivalent to `POST /sim/inject` with every field set to
`null`. Used by BDD test teardown (`sim_ui_steps.py`).

**Response** `204 No Content`.

---

### `GET /sim/schema`

Returns `control_schema()` descriptors per asset. These define which inject fields the ControllerV2
UI renders as interactive controls.

**Response** `200 application/json`:
```json
{
  "ev": [
    { "key": "ev_plugged",       "label": "Plugged In",    "kind": "switch",       "min": null, "max": null,   "unit": "" },
    { "key": "ev_departure_min", "label": "Departure In",  "kind": "number_input", "min": 0,    "max": 1440,   "unit": "min" }
  ],
  "heater": [
    { "key": "heater_setpoint_c", "label": "Temperature Setpoint", "kind": "slider", "min": 16, "max": 24, "unit": "°C" }
  ],
  "pv": [
    { "key": "pv_irradiance",       "label": "Irradiance Override", "kind": "slider", "min": 0,    "max": 1,   "unit": "" },
    { "key": "pv_irradiance_alpha", "label": "Blend-back Speed",    "kind": "slider", "min": 0.01, "max": 1.0, "unit": "" }
  ],
  "base_load": [
    { "key": "base_load_kw", "label": "Base Load Override", "kind": "number_input", "min": 0, "max": 20, "unit": "kW" }
  ],
  "battery": []
}
```

---

### `POST /sim/override` (deprecated alias)

Kept for backward compatibility with `Simulation.tsx` and legacy BDD steps. Translates the old
`UserOverrides` shape into `SimInjectState`:

| Old field | Maps to |
|---|---|
| `ev_plugged` | `inject.ev_plugged` |
| `pv_irradiance` | `inject.pv_irradiance` |
| `ambient_temp_c` | `inject.ambient_temp_c` |
| `base_load_w` | `inject.base_load_kw = w / 1000.0` |
| All other old fields | Silently dropped |
| Empty body `{}` | Releases all overrides |

`GET /sim/override` translates `SimInjectState` back into the old `UserOverrides` shape
(needed by `controller_v2_steps.py` which reads `ev_plugged` via this endpoint).

**Will be removed** in Group D cleanup.

---

## Field Reference

| Field | Type | Behaviour | Unit | Effect |
|---|---|---|---|---|
| `battery_soc` | `f64 \| null` | A | [0–1] | Jump battery SoC to value; cleared next tick |
| `ev_soc` | `f64 \| null` | A | [0–1] | Jump EV SoC to value; cleared next tick |
| `heater_temp_c` | `f64 \| null` | A | °C | Jump heater temperature to value; cleared next tick |
| `pv_irradiance` | `f64 \| null` | B | [0–1] | Freeze PV irradiance; EMA blend-back on release |
| `pv_irradiance_alpha` | `f64` | — | — | EMA coefficient for blend-back (default 0.1) |
| `ev_plugged` | `bool \| null` | C | — | Override EV plugged state |
| `ev_departure_min` | `f64 \| null` | C | min | Override departure time; replaces active EV packet tier deadline in planner |
| `heater_setpoint_c` | `f64 \| null` | C | °C | Target temperature for heater dispatcher (ON if temp < target, OFF otherwise) |
| `ambient_temp_c` | `f64 \| null` | C | °C | Override outdoor temperature used in heater thermal model |
| `base_load_kw` | `f64 \| null` | C | kW | Override base load power; snaps to `baseline_kw_profile` on release |
| `grid_import_limit_kw` | `f64 \| null` | C | kW | Override import capacity limit (ignored when a VTN event holds the limit) |
| `grid_export_limit_kw` | `f64 \| null` | C | kW | Override export capacity limit (ignored when a VTN event holds the limit) |

**Grid limit priority**: VTN event always wins. `inject.grid_import_limit_kw` only applies when
`capacity_snap.import_limit_event_id.is_none()`.

**`heater_setpoint_c` note**: Handled in the dispatcher only (binary ON/OFF vs target). Does not
change thermostat bounds or thermal model parameters.

---

## Backend Architecture

### State storage (`VEN/src/state.rs`)

```rust
pub struct SimInjectState {
    pub battery_soc: Option<f64>,
    pub ev_soc: Option<f64>,
    pub heater_temp_c: Option<f64>,
    pub pv_irradiance: Option<f64>,
    pub pv_irradiance_alpha: f64,          // default 0.1
    pub ev_plugged: Option<bool>,
    pub ev_departure_min: Option<f64>,
    pub heater_setpoint_c: Option<f64>,
    pub ambient_temp_c: Option<f64>,
    pub base_load_kw: Option<f64>,
    pub grid_import_limit_kw: Option<f64>,
    pub grid_export_limit_kw: Option<f64>,
}
```

Stored as `InnerState.inject_state` with `#[serde(skip)]` — ephemeral, not persisted to disk.
Accessors: `inject_state()`, `set_inject_state()`, `clear_inject_field(&str)`.

### PV smoothing (`VEN/src/simulator/mod.rs`)

```rust
pub struct PvSmoothingState {
    pub current_irradiance: f64,
    pub override_was_active: bool,
}
```

Stored on `SimState` with `#[serde(skip)]`. The `override_was_active` flag prevents startup lag:
EMA tracking only activates when blending back from a released override.

### Tick loop (`VEN/src/loops.rs` — `spawn_sim_tick`)

Each tick (1 Hz):
1. Read `inject_state` once
2. **Behaviour A** — apply `battery_soc`, `ev_soc`, `heater_temp_c` via `cfg.reset()` +
   `find_asset_mut()`, then `clear_inject_field()` for each applied field
3. **Grid limits** — compose `effective_capacity`: inject overrides applied only when no VTN event holds the limit
4. **Behaviour C env/state** — pass `ambient_temp_c`, `base_load_kw`, `ev_plugged` into `tick()` as params
5. Call `sim.tick(...)` — PV EMA smoothing runs inside
6. Call `build_setpoints(plan, assets, configs, &effective_capacity, inject.heater_setpoint_c, now)`

### Planning loop (`VEN/src/loops.rs` — `spawn_planning`)

Each planning cycle:
- Read `inject_state().ev_departure_min`
- Compute `ev_departure_override = now + Duration::seconds(min * 60)` if set
- Pass `ev_departure_override` to `run_planner()`
- Inside `run_planner()`: replace active EV packet tier deadline before the planning loop

---

## TypeScript API (`VEN/ui/src/api/`)

### `SimInjectState` type (`types.ts`)

```typescript
export type SimInjectState = {
  battery_soc?: number | null;
  ev_soc?: number | null;
  heater_temp_c?: number | null;
  pv_irradiance?: number | null;
  pv_irradiance_alpha?: number;
  ev_plugged?: boolean | null;
  ev_departure_min?: number | null;
  heater_setpoint_c?: number | null;
  ambient_temp_c?: number | null;
  base_load_kw?: number | null;
  grid_import_limit_kw?: number | null;
  grid_export_limit_kw?: number | null;
};
```

`UserOverrides` is kept as a separate deprecated type with the old field names (used by
`Simulation.tsx` via the `POST /sim/override` alias). Will be removed in Phase 8.

### Client methods (`client.ts`)

| Method | Endpoint | Notes |
|---|---|---|
| `getSimInject()` | `GET /sim/inject` | Returns `SimInjectState` |
| `postSimInject(patch)` | `POST /sim/inject` | Partial-merge; sends only changed fields |
| `getSimOverride()` ⚠️ | `GET /sim/inject` | Deprecated; casts result to `UserOverrides` |
| `postSimOverride(o)` ⚠️ | `POST /sim/inject` | Deprecated; casts `UserOverrides` to `SimInjectState` |

### Hooks (`hooks.ts`)

| Hook | Purpose |
|---|---|
| `useSimInject()` | Fetches inject state on mount (`staleTime: Infinity`) |
| `useSetSimInject()` | Mutation: partial-merge POST; invalidates `["simInject"]` on success |
| `useSimOverride()` ⚠️ | Deprecated alias; returns `UserOverrides`-typed data via `getSimOverride()` |
| `useSetSimOverride()` ⚠️ | Deprecated alias; kept for `Simulation.tsx` |

### ControllerV2 usage pattern

```typescript
const { data: simInject } = useSimInject();
const { mutate: setSimInject } = useSetSimInject();

function handleOverrideChange(patch: Partial<SimInjectState>) {
  setSimInject(patch);  // backend handles partial-merge; no client-side spread needed
}
```

`AssetRightSection` reads control values from `SimInjectState` via `getValue(key)`, with a
fallback to `sim.assets.ev.plugged` for `ev_plugged` when no override is active.

---

## Field Classification — What Belongs Where

This chapter records the reasoning behind each old `UserOverrides` field — whether it was moved,
dropped, or retained — and identifies gaps in the current `SimInjectState`.

### Decision criteria

A field belongs in `SimInjectState` if it is:
- **Runtime state** — a physics quantity that can be observed and changed during operation
  (SoC, temperature, plugged status, irradiance reading)
- **Environment input** — external condition that the physical model consumes each tick
  (outdoor temperature, base load, grid limits)

A field belongs in **profile YAML only** if it is:
- **Hardware specification** — a physical capability of the installed device that cannot change
  at runtime (panel peak wattage, inverter capacity, EVSE breaker limit, heating element rating)

A field belongs in **another API** if it is:
- **User intent / planner input** — better expressed as a scheduling request
  (`POST /user-requests` with `target_soc`, `latest_end`, `desired_power_kw`)

A field should be **dropped entirely** if it is:
- A raw setpoint bypass that ignores the planner — these were debug tools with no physical
  meaning in a system with an active dispatcher

---

### Old `UserOverrides` field audit

| Field | Old action | Decision | Reason |
|---|---|---|---|
| `pv_irradiance` | Set `PvInverter.irradiance` | **Retained** as `SimInjectState.pv_irradiance` (Behaviour B) | Valid environment input — a sensor reading, not a spec |
| `ambient_temp_c` | Set `Heater.ambient_temp_c` | **Retained** as `SimInjectState.ambient_temp_c` (Behaviour C) | Valid environment input — outdoor temperature measured by sensor |
| `ev_plugged` | Set `EvState.plugged` | **Retained** as `SimInjectState.ev_plugged` (Behaviour C) | Valid physical state — EV connectivity is observable and injectable |
| `base_load_w` | Set `BaseLoad.baseline_kw` | **Retained** as `SimInjectState.base_load_kw` (Behaviour C); unit fixed to kW | Valid — simulates variable background load. See note below. |
| `pv_rated_kw` | Mutated `PvInverter.rated_kw` | **Dropped → profile only** | Hardware spec: physical panel peak wattage. Cannot change at runtime. |
| `ev_max_charge_kw` | Mutated `EvCharger.max_charge_kw` | **Dropped → profile only** | Hardware spec: EVSE breaker limit or on-board charger maximum. Cannot change at runtime. |
| `heater_max_kw` | Mutated `Heater.max_kw` | **Dropped → profile only** | Hardware spec: heating element rated power. Cannot change at runtime. |
| `heater_temp_min_c` | Mutated `Heater.temp_min_c` | **Dropped → profile only** | Installer-set thermostat safety floor. Not a runtime user action. |
| `heater_temp_max_c` | Mutated `Heater.temp_max_c` | **Dropped → profile only** | Installer-set thermostat safety ceiling. Not a runtime user action. |
| `ev_soc_target` | Mutated `EvCharger.soc_target` | **Dropped → `POST /user-requests`** | User intent expressed as a scheduling request with `target_soc`; planner handles it |
| `ev_desired_kw` | Mutated `EvCharger.default_charge_kw` | **Dropped** | `default_charge_kw` was the idle setpoint before the planner existed. The planner now issues all setpoints. There is no "desired idle rate" separate from the active plan. |
| `ev_force_kw` | Forced EV setpoint bypassing planner | **Dropped** | Raw setpoint bypass has no physical meaning with a dispatcher running. Force-testing a setpoint is done by pausing or cancelling the packet. |
| `heater_force_kw` | Forced heater setpoint bypassing planner | **Dropped** | Same reason. `heater_setpoint_c` in SimInjectState replaces the intent correctly (comfort target → dispatcher translates to ON/OFF). |
| `battery_force_kw` | Forced battery setpoint | **Dropped** | Was never implemented (control_schema returned it but no injection code existed). Battery is fully automatic. |
| `pv_force_export_limit_kw` | Set `PvInverter.export_limit_kw` | **Dropped for now** | Per-inverter curtailment is a distinct concept from site-level `grid_export_limit_kw`. Could be re-added as `pv_export_limit_kw` (Behaviour C) if needed. See future candidates below. |

---

### `base_load_kw` — the `Option<f64>` convention

The old `base_load_w` was a bare `f64`. There was no way to express "revert to the profile
default" — sending `0` meant "set to 0 W", not "release override". An operator would need to
know the profile value to restore it.

The new `base_load_kw: Option<f64>` in `SimInjectState` solves this cleanly:

- JSON `null` → Rust `None` → `tick()` uses `bl.baseline_kw_profile` (the original profile value,
  stored separately from the mutable `bl.baseline_kw`)
- JSON `3.5` → Rust `Some(3.5)` → `tick()` sets `bl.baseline_kw = 3.5`

This `Option<f64>` / null-means-release pattern applies to **all** Behaviour C fields. The unit
was also corrected from watts to kilowatts to match every other field in the system.

---

### What SHOULD be in `SimInjectState` but is not yet

| Candidate field | Behaviour | Reason |
|---|---|---|
| `pv_export_limit_kw: Option<f64>` | C | Per-PV-inverter export curtailment. Distinct from `grid_export_limit_kw` (site-level). Real grid operators can curtail individual inverters via DRED or export limitation signals. Useful for testing PV curtailment scenarios where the grid limit is on the inverter rather than the site meter. |

No other gaps identified. The existing fields cover all meaningful runtime-injectable quantities
for the current asset set (EV, PV, battery, heater, base load, grid).

---

## Pending Work

### Group D — BDD migration + alias cleanup

BDD steps still targeting `/sim/override`:

| File | Step | Target |
|---|---|---|
| `tests/features/steps/uc_steps.py` | `ev_plugged: false/true` | → `POST /sim/inject` |
| `tests/features/steps/uc_steps.py` | `pv_irradiance: 1.0` | → `POST /sim/inject` |
| `tests/features/steps/sim_ui_steps.py` | "reset overrides" | → `POST /sim/inject/reset` |
| `tests/features/steps/controller_v2_steps.py` | `GET /sim/override` | → `GET /sim/inject` |

Backend to remove after migration:
- `POST /sim/override` route + `post_sim_override` handler
- `UserOverrides` struct and `overrides`/`set_overrides` accessors on `AppState`

UI to remove:
- `getSimOverride()`, `postSimOverride()` in `client.ts`
- `useSimOverride()`, `useSetSimOverride()` in `hooks.ts`

### Phase 8 — Simulation.tsx (low priority)

`Simulation.tsx` still uses `UserOverrides` and `POST /sim/override`. Many of its controls
(`heater_max_kw`, `ev_desired_kw`, `pv_rated_kw`, etc.) map to fields that no longer exist
in `SimInjectState` and are silently dropped by the alias bridge. Migration requires either
removing those controls or reimplementing them against the correct new fields.
