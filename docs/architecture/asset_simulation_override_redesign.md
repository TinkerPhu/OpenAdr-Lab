# Asset Simulation — Inject API

> **Status**: All groups complete. `POST /sim/override` alias and `UserOverrides` removed.
> `POST /sim/inject` is the sole API. All BDD steps and Simulation.tsx migrated.

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
| **C — Frozen + snap return** | Held at the injected value each tick while active. On release (`null`), snaps back to profile default immediately. | `ev_plugged`, `ev_departure_min`, `ev_soc_target`, `heater_setpoint_c`, `heater_temp_min_c`, `heater_temp_max_c`, `ambient_temp_c`, `base_load_kw`, `grid_import_limit_kw`, `grid_export_limit_kw` |

**Behaviour B note**: `pv_irradiance_alpha` EMA is only active during blend-back from an override.
Normal operation uses the natural irradiance model directly (no ramp-up lag on restart). Tracked
via `PvSmoothingState.override_was_active: bool`.

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
  "ev_soc_target": 0.9,
  "heater_setpoint_c": null,
  "heater_temp_min_c": null,
  "heater_temp_max_c": null,
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
  "ev_soc_target": 0.8,
  "heater_temp_c": 16.5,
  "heater_setpoint_c": 19.0,
  "heater_temp_min_c": 16.0,
  "heater_temp_max_c": 22.0,
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
`null`. Used by BDD test teardown (`environment.py` `after_scenario`).

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

## Field Reference

| Field | Type | Behaviour | Unit | Effect |
|---|---|---|---|---|
| `battery_soc` | `f64 \| null` | A | [0–1] | Jump battery SoC to value; cleared next tick |
| `ev_soc` | `f64 \| null` | A | [0–1] | Jump EV SoC to value; cleared next tick |
| `heater_temp_c` | `f64 \| null` | A | °C | Jump heater temperature to value; cleared next tick |
| `pv_irradiance` | `f64 \| null` | B | [0–1] | Freeze PV irradiance; EMA blend-back on release |
| `pv_irradiance_alpha` | `f64` | — | — | EMA coefficient for blend-back (default 0.1) |
| `ev_plugged` | `bool \| null` | C | — | Override EV plugged state; snaps back to `true` (plugged) on release |
| `ev_departure_min` | `f64 \| null` | C | min | Override departure time; replaces active EV packet tier deadline in planner |
| `ev_soc_target` | `f64 \| null` | C | [0–1] | Override EV BMS charge ceiling; charging stops at this SoC. Snaps to `soc_target_profile` on release |
| `heater_setpoint_c` | `f64 \| null` | C | °C | Target temperature for heater dispatcher (ON if temp < target, OFF otherwise) |
| `heater_temp_min_c` | `f64 \| null` | C | °C | Override heater comfort band lower bound; heater forces on below this temperature. Snaps to `temp_min_c_profile` on release |
| `heater_temp_max_c` | `f64 \| null` | C | °C | Override heater comfort band upper bound; heater cuts off above this temperature. Snaps to `temp_max_c_profile` on release |
| `ambient_temp_c` | `f64 \| null` | C | °C | Override outdoor temperature used in heater thermal model |
| `base_load_kw` | `f64 \| null` | C | kW | Override base load power; snaps to `baseline_kw_profile` on release |
| `grid_import_limit_kw` | `f64 \| null` | C | kW | Override import capacity limit (ignored when a VTN event holds the limit) |
| `grid_export_limit_kw` | `f64 \| null` | C | kW | Override export capacity limit (ignored when a VTN event holds the limit) |

**Grid limit priority**: VTN event always wins. `inject.grid_import_limit_kw` only applies when
`capacity_snap.import_limit_event_id.is_none()`.

**`heater_setpoint_c` vs comfort band**: `heater_setpoint_c` is a dispatcher-level comfort target
(binary ON/OFF). `heater_temp_min_c` / `heater_temp_max_c` are physics-level thermostat bounds —
the heater is forced on below min and forced off above max, regardless of dispatcher setpoint.

**`ev_soc_target` and physics**: `soc_target` is enforced in `EvCharger.step_inner()` — charging
stops when `soc_pct >= soc_target`. This mirrors real BMS behaviour. The profile value is stored
as `soc_target_profile` for snap-back. The planner also uses `soc_target` (via
`resolve_request_target`) to size energy packets.

---

## Backend Architecture

### State storage (`VEN/src/state.rs`)

```rust
pub struct SimInjectState {
    // Behaviour A — one-shot
    pub battery_soc: Option<f64>,
    pub ev_soc: Option<f64>,
    pub heater_temp_c: Option<f64>,
    // Behaviour B — frozen + EMA return on release
    pub pv_irradiance: Option<f64>,
    pub pv_irradiance_alpha: f64,          // default 0.1
    // Behaviour C — frozen while active, snap to profile default on release
    pub ev_plugged: Option<bool>,
    pub ev_departure_min: Option<f64>,
    pub ev_soc_target: Option<f64>,
    pub heater_setpoint_c: Option<f64>,
    pub heater_temp_min_c: Option<f64>,
    pub heater_temp_max_c: Option<f64>,
    pub ambient_temp_c: Option<f64>,
    pub base_load_kw: Option<f64>,
    pub grid_import_limit_kw: Option<f64>,
    pub grid_export_limit_kw: Option<f64>,
}
```

Stored as `InnerState.inject_state` with `#[serde(skip)]` — ephemeral, not persisted to disk.
Accessors: `inject_state()`, `set_inject_state()`, `clear_inject_field(&str)`.

### Profile snap-back fields (`VEN/src/assets/`)

Each Behaviour C field that overrides a profile value has a corresponding `_profile` field for
snap-back. Applied in `tick()` as `active = override.unwrap_or(profile_default)`:

| Asset | Active field | Profile field |
|---|---|---|
| `BaseLoad` | `baseline_kw` | `baseline_kw_profile` |
| `EvCharger` | `soc_target` | `soc_target_profile` |
| `Heater` | `temp_min_c` | `temp_min_c_profile` |
| `Heater` | `temp_max_c` | `temp_max_c_profile` |

`ev_plugged` snaps back to `true` via `unwrap_or(true)` in `tick()` — there is no profile field
because the profile default is always plugged.

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
4. **Behaviour C env/state** — pass `ambient_temp_c`, `heater_temp_min_c`, `heater_temp_max_c`,
   `base_load_kw`, `ev_plugged`, `ev_soc_target` into `tick()` as params
5. Call `sim.tick(...)` — PV EMA smoothing + all Behaviour C applications run inside
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
  // Behaviour A: one-shot jumps (auto-cleared after application)
  battery_soc?: number | null;
  ev_soc?: number | null;
  heater_temp_c?: number | null;
  // Behaviour B: frozen + EMA blend-back on release
  pv_irradiance?: number | null;
  pv_irradiance_alpha?: number;
  // Behaviour C: frozen while active, snap to profile on release
  ev_plugged?: boolean | null;
  ev_departure_min?: number | null;
  ev_soc_target?: number | null;
  heater_setpoint_c?: number | null;
  heater_temp_min_c?: number | null;
  heater_temp_max_c?: number | null;
  ambient_temp_c?: number | null;
  base_load_kw?: number | null;
  grid_import_limit_kw?: number | null;
  grid_export_limit_kw?: number | null;
};
```

### Client methods (`client.ts`)

| Method | Endpoint | Notes |
|---|---|---|
| `getSimInject()` | `GET /sim/inject` | Returns `SimInjectState` |
| `postSimInject(patch)` | `POST /sim/inject` | Partial-merge; sends only changed fields |

### Hooks (`hooks.ts`)

| Hook | Purpose |
|---|---|
| `useSimInject()` | Fetches inject state on mount (`staleTime: Infinity`) |
| `useSetSimInject()` | Mutation: partial-merge POST; invalidates `["simInject"]` on success |

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
