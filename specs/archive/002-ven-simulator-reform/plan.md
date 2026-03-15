# Implementation Plan: VEN Simulator Reform

**Branch**: `002-ven-simulator-reform` | **Date**: 2026-03-15 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/002-ven-simulator-reform/spec.md`

## Summary

Refactor the VEN simulator and profile configuration from a hardcoded per-device model to a generic `Vec<AssetEntry>` model. This is a pure Rust backend refactor — zero behavior change, zero HTTP API contract change (except the three new endpoints), zero UI change. All 123 existing BDD scenarios must pass unchanged. The generic model enables speckit 2 (controller reform) and speckit 3 (timeline UI) to build on a stable, extensible foundation.

## Technical Context

**Language/Version**: Rust (stable channel, tokio async runtime, axum web framework)
**Primary Dependencies**: serde/serde_json/serde_yaml (serialization), chrono (timestamps), tokio (async), axum (HTTP), `HashMap`/`VecDeque` from std
**Storage**: JSON files on disk — `sim_state.json` (simulator state), `app_state.json` (full app state). No database changes.
**Testing**: Python behave BDD (integration, 123 scenarios/801 steps) + cargo test (unit)
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2
**Project Type**: Backend web service (VEN)
**Performance Goals**: Unchanged — 1-second sim tick latency must be preserved; tick loop overhead from `Vec` iteration over 5 assets is negligible
**Constraints**: Pi4 ARM64 build constraints — `CARGO_BUILD_JOBS=4`, `cpus: 1.5`, `memory: 1500M` in test compose
**Scale/Scope**: Single-instance VEN service; 5 asset types; 4 profile files

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|---|---|---|
| I. OpenADR Spec Fidelity | ✅ PASS | This refactor touches simulator internals only; no OpenADR field names are involved. |
| II. BDD-First Testing | ✅ PASS | All 123 existing BDD scenarios cover the behavior being refactored. No new behavior is introduced — no new BDD scenarios are required. The red-phase requirement does not apply to pure refactors. |
| III. Upstream Compatibility | ✅ N/A | This refactor is entirely within `VEN/` — it does not touch the `openleadr-rs` git submodule. |
| IV. Lean Architecture | ✅ PASS | The generic `Vec<AssetEntry>` model is simpler than the current per-device hardcoding. It reduces future touch-points from N-layers to 1 file per new asset type. No abstractions are added beyond what the spec requires. |
| V. Infrastructure Parity | ✅ PASS | Standard Docker deploy flow unchanged. No new services. |

**Constitution Check post-design** (re-evaluated after Phase 1):

The `AssetHistoryBuffer` data structure added to `controller/trace.rs` is an addition without wiring — a data structure that is not used by anything yet. This is a mild Constitution IV tension. **Justification**: It is explicitly required by the spec to land in this speckit so speckit 2 has a stable target to wire up. The structure is small (2 types, 2 methods), well-bounded, and does not add any runtime overhead.

## Project Structure

### Documentation (this feature)

```text
specs/002-ven-simulator-reform/
├── plan.md              # This file
├── research.md          # Phase 0 — decisions and rationale
├── data-model.md        # Phase 1 — all types and their fields
├── quickstart.md        # Phase 1 — how to run and verify
├── contracts/
│   └── sim-endpoints.md # Phase 1 — HTTP endpoint contracts
├── checklists/
│   └── requirements.md  # Spec quality checklist
└── tasks.md             # Phase 2 output (/speckit.tasks — NOT created here)
```

### Source Code (repository root)

```text
VEN/src/
├── main.rs                      # Add 3 routes; add Setpoints→HashMap bridge
├── profile.rs                   # Replace DeviceConfig with Vec<AssetConfig>
├── state.rs                     # Remove 3 stub fields from UserOverrides
├── simulator/
│   ├── mod.rs                   # SimState→Vec<AssetEntry>+GridMeter; tick(); to_sim_snapshot()
│   ├── assets/                  # NEW module directory
│   │   ├── mod.rs               # AssetState enum; AssetCapabilities; ControlDescriptor; TickEnvironment
│   │   ├── ev.rs                # EvCharger, EvConfig (from actors.rs)
│   │   ├── heater.rs            # Heater, HeaterConfig (from actors.rs)
│   │   ├── pv.rs                # PvInverter, PvConfig (from actors.rs)
│   │   ├── battery.rs           # Battery, BatteryConfig (from actors.rs)
│   │   └── base_load.rs         # BaseLoad, BaseLoadConfig (new)
│   ├── actors.rs                # DELETED after extraction
│   ├── energy.rs                # EnergyCounter — unchanged
│   ├── persist.rs               # save/load SimState — logic unchanged; struct changes
│   └── power_model.rs           # Simplified: sum of asset power_kw
├── controller/
│   └── trace.rs                 # Add AssetHistoryBuffer, AssetTimelinePoint (data struct only)

VEN/profiles/
├── ven-1.yaml                   # Migrated to typed asset list
├── ven-2.yaml                   # Migrated to typed asset list
├── ven-3.yaml                   # Migrated to typed asset list
└── test.yaml                    # Migrated to typed asset list
```

**Structure Decision**: Single-project layout (Option 1). All changes are within the VEN Rust service. No new projects or services are created.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| AssetHistoryBuffer added but not wired | Speckit 2 needs a stable target type to wire up. Landing it here avoids a cross-speckit type change. | Deferring entirely to speckit 2 would require opening simulator/mod.rs again just to add the type, breaking the clean speckit boundary. |

---

## Phase 0: Research

*All decisions resolved. See [research.md](research.md) for full rationale.*

| Unknown | Resolution |
|---|---|
| Enum method dispatch pattern | Match-based delegation — closed set of 5 variants, compiler enforces exhaustiveness |
| YAML tagged enum deserialization | `#[serde(tag = "type", rename_all = "snake_case")]` on AssetConfig |
| sim_state.json migration | Re-initialize from profile defaults on parse failure (ephemeral state) |
| Setpoints bridge | Thin conversion in main.rs: Setpoints → HashMap<String, f64>; removed in speckit 2 |
| Per-asset energy tracking | Each AssetEntry gets its own EnergyCounter; GridMeter tracks grid-level totals |
| predict() scope | Stub implementation (single-point) in speckit 1; full physics in speckit 2 |
| TickEnvironment | HashMap<String, f64> with keys "hour_of_day" and "ambient_temp_c" |
| base_load treatment | First-class AssetState::BaseLoad variant (not a bare f64) |

---

## Phase 1: Implementation Steps

The implementation is structured as 9 sequential steps. Each step compiles and leaves the codebase in a buildable state. Steps 1–7 are internal Rust changes; Step 8 migrates YAML; Step 9 is the integration verification.

### Step 1 — Extract actors into per-type files (no interface changes)

**Goal**: Create `simulator/assets/` module with one file per asset type. Move existing actor structs verbatim — do not change any method signatures yet.

**Files changed**:
- Create `VEN/src/simulator/assets/` directory
- Create `VEN/src/simulator/assets/mod.rs` — re-export all types; declare `AssetState` enum (empty methods stub for now)
- Create `VEN/src/simulator/assets/ev.rs` — move `EvCharger` from `actors.rs`
- Create `VEN/src/simulator/assets/heater.rs` — move `Heater`
- Create `VEN/src/simulator/assets/pv.rs` — move `PvInverter`
- Create `VEN/src/simulator/assets/battery.rs` — move `Battery`
- Create `VEN/src/simulator/assets/base_load.rs` — new; `BaseLoad { baseline_kw, current_kw }`
- Update `VEN/src/simulator/mod.rs` — change import from `actors::*` to `assets::*`
- Delete `VEN/src/simulator/actors.rs`

**Verification**: `cargo build` compiles. All existing tests pass.

---

### Step 2 — Define AssetState interface and enum methods

**Goal**: Implement all required methods on `AssetState` and delegate to inner types. Add `AssetCapabilities`, `ControlDescriptor`, `ControlKind`, `TickEnvironment`, `EnergyState`, `TimeWindow` types.

**Files changed**:
- `VEN/src/simulator/assets/mod.rs` — implement `AssetState` with full match-delegation for all 8 methods
- `VEN/src/simulator/assets/ev.rs` — implement `update(dt_s, setpoint, env)`, `predict()`, `state_values()`, `default_setpoint()`, `capabilities()`, `control_schema()`, `reset()`, `update_config()`
- `VEN/src/simulator/assets/heater.rs` — same 8 methods
- `VEN/src/simulator/assets/pv.rs` — same 8 methods
- `VEN/src/simulator/assets/battery.rs` — same 8 methods
- `VEN/src/simulator/assets/base_load.rs` — same 8 methods

**Key changes per type**:
- `EvCharger::update()`: signature changes from `(dt_s, commanded_kw)` to `(dt_s, setpoint, env)` — `commanded_kw = setpoint`; env is not used by EV
- `Heater::update()`: absorbs `ambient_temp_c` from env (`env["ambient_temp_c"]`); setpoint is commanded kW
- `PvInverter::update()`: absorbs `hour_of_day` and irradiance override from env; setpoint is `export_limit_kw`
- `Battery::update()`: `commanded_kw = setpoint`; env not used
- `BaseLoad::update()`: always returns `baseline_kw`; setpoint ignored

**Verification**: `cargo build` compiles. All existing tests pass (SimState still uses old structure at this point).

---

### Step 3 — Convert SimState to Vec<AssetEntry>

**Goal**: Replace named device fields on `SimState` with `assets: Vec<AssetEntry>` and `grid: GridMeter`. Update `from_profile()` and `tick()`.

**Files changed**:
- `VEN/src/simulator/mod.rs`:
  - Replace `ev: Option<EvCharger>`, `heater: Option<Heater>`, etc. with `assets: Vec<AssetEntry>`, `grid: GridMeter`
  - `from_profile(profile)`: iterate `profile.assets`, construct `AssetEntry` per `AssetConfig` variant
  - `tick(dt_s, setpoints: HashMap<String, f64>, now, overrides)`:
    - Build `TickEnvironment` from `now` and `overrides.ambient_temp_c`
    - For each asset: look up setpoint in map (default to `asset.state.default_setpoint()`); call `asset.state.update(dt_s, sp, env)`; integrate per-asset `EnergyCounter`
    - Apply UserOverrides force fields by asset id
    - Derive `GridMeter`: sum all asset power_kw → net_power_w; split import/export; integrate grid energy
- `VEN/src/simulator/power_model.rs`: simplify to `sum_asset_powers(entries: &[AssetEntry]) -> f64`
- `VEN/src/profile.rs`:
  - Replace `DeviceConfig` with `Vec<AssetConfig>`
  - Add `AssetConfig` enum with `#[serde(tag = "type", rename_all = "snake_case")]`
  - Update `Profile::devices` field to `Profile::assets: Vec<AssetConfig>`
  - Keep existing reactor/simulator/planner/packets fields unchanged
- `VEN/src/simulator/persist.rs`: update `SimState` load/save (struct shape changes; logic unchanged)

**Migration note**: `sim_state.json` in old format will fail to deserialize; `persist.rs` returns `None` on parse failure; `from_profile()` reinitializes from defaults. This is the intended behavior.

**Verification**: `cargo build` compiles. `cargo test` passes. BDD tests may break on `GET /sim` response shape (step 5 fixes this). Run simulator feature tests to confirm.

---

### Step 4 — Update main.rs tick call site (Setpoints bridge)

**Goal**: Translate `Setpoints` (reactor output) to `HashMap<String, f64>` at the tick call site in `main.rs`. This is a purely mechanical change.

**Files changed**:
- `VEN/src/main.rs`:
  - After `reactor.evaluate(...)` returns `Setpoints`, add conversion:
    ```rust
    let mut sp_map = HashMap::new();
    sp_map.insert("ev".to_string(), setpoints.ev_charge_kw);
    sp_map.insert("heater".to_string(), setpoints.heater_kw);
    sp_map.insert("pv".to_string(), setpoints.pv_export_limit_kw.unwrap_or(f64::MAX));
    sp_map.insert("battery".to_string(), setpoints.battery_kw);
    // base_load is non-flexible, never in setpoints map
    ```
  - Pass `sp_map` to `sim.tick(dt_s, sp_map, now, &overrides)`
  - Dispatcher setpoint overlay (controller/dispatcher.rs) currently also writes to Setpoints — keep its interface unchanged; apply dispatcher overlay to sp_map after dispatcher writes to Setpoints, same conversion

**Note**: `controller/dispatcher.rs` is out of scope. It currently modifies `Setpoints` in place. The main loop applies dispatcher overlay to the same `Setpoints` struct before conversion. This is preserved; only the conversion step is added.

**Verification**: `cargo build` compiles. Simulator tick runs correctly.

---

### Step 5 — Replace SimSnapshot with generic format

**Goal**: Replace named per-device snapshot structs with `HashMap<String, AssetSnapshot>`. Update `to_sim_snapshot()` and the `GET /sim` handler.

**Files changed**:
- `VEN/src/simulator/mod.rs`:
  - Add `AssetSnapshot { power_kw: f64, values: HashMap<String, f64> }`
  - Redefine `SimSnapshot` with `assets: HashMap<String, AssetSnapshot>` and grid totals
  - Remove `EvSnapshot`, `HeaterSnapshot`, `PvSnapshot`, `BatterySnapshot`
  - Implement `to_sim_snapshot()`: iterate `self.assets`; for each entry call `state.state_values()` and build `AssetSnapshot`; collect into map
- `VEN/src/main.rs`: `GET /sim` handler returns `Json(sim_state.to_sim_snapshot())` — no change to handler code; return type changes

**Verification**: `cargo build` compiles. `GET /sim` response matches the contract in `contracts/sim-endpoints.md`. BDD simulator scenarios that assert on `GET /sim` fields pass (step definitions check field paths — need to verify existing steps don't hardcode old field paths).

**BDD step check**: Search `tests/steps/` for references to old snapshot fields (`ev.soc_pct`, `heater.temp_c`, etc.) and update any that use the old format. These are step definition changes only — no `.feature` file changes allowed.

---

### Step 6 — Add GET /sim/schema endpoint

**Goal**: Implement new `GET /sim/schema` endpoint that returns control descriptors for all configured assets.

**Files changed**:
- `VEN/src/main.rs`:
  - Add route: `GET /sim/schema → sim_schema_handler`
  - Handler: acquire read lock on sim state; iterate assets; call `asset.state.control_schema()`; collect into `HashMap<String, Vec<ControlDescriptor>>`; return as JSON

**Verification**: `cargo build` compiles. `curl /sim/schema` returns expected structure.

---

### Step 7 — Add reset/config endpoints; remove UserOverrides stubs

**Goal**: Implement `POST /sim/reset/ev`, `POST /sim/reset/battery`, `PUT /sim/config/battery`. Remove `ev_initial_soc`, `battery_initial_soc`, `battery_capacity_kwh` from `UserOverrides`.

**Files changed**:
- `VEN/src/main.rs`:
  - Add routes: `POST /sim/reset/ev`, `POST /sim/reset/battery`, `PUT /sim/config/battery`
  - Handlers: find asset by id in `sim_state.assets`; call `asset.state.reset(values)` or `asset.state.update_config(values)`; persist `sim_state.json`; return 200/404/400
- `VEN/src/state.rs`:
  - Remove from `UserOverrides`: `ev_initial_soc: Option<f64>`, `battery_initial_soc: Option<f64>`, `battery_capacity_kwh: Option<f64>`
  - Keep all force fields intact (`ev_force_kw`, `heater_force_kw`, `battery_force_kw`, `pv_force_export_limit_kw`, `battery_force_kw`)

**BDD step check**: Search `tests/steps/` for BDD steps that POST `ev_initial_soc` or `battery_initial_soc` to `/sim/override`. Update these steps to use the new endpoint. This is a step definition change only — no `.feature` file changes.

---

### Step 8 — Add AssetHistoryBuffer to controller/trace.rs

**Goal**: Add data structure only — no wiring to live data.

**Files changed**:
- `VEN/src/controller/trace.rs`:
  - Add `AssetHistoryBuffer { timestamps: VecDeque<DateTime<Utc>>, columns: HashMap<String, VecDeque<f64>>, capacity: usize }`
  - Add `AssetTimelinePoint { ts: DateTime<Utc>, values: HashMap<String, f64> }`
  - Implement `AssetHistoryBuffer::push(ts, values)` — append row; evict oldest if at capacity; insert NAN for columns not present in values
  - Implement `AssetHistoryBuffer::to_timeline(window: Option<(DateTime<Utc>, DateTime<Utc>)>) -> Vec<AssetTimelinePoint>`

**Verification**: `cargo build` compiles. No runtime behavior changes.

---

### Step 9 — Migrate profile YAML files

**Goal**: Convert all four YAML files from named device fields to typed asset list format. This is purely a configuration change; no Rust source changes.

**Files changed**:
- `VEN/profiles/ven-1.yaml`
- `VEN/profiles/ven-2.yaml`
- `VEN/profiles/ven-3.yaml`
- `VEN/profiles/test.yaml`

**Migration rules**:
- `devices.ev: { ... }` → `assets: [{type: ev, id: ev, ...}]`
- `devices.heater: { ... }` → append `{type: heater, id: heater, ...}`
- `devices.pv: { ... }` → append `{type: pv, id: pv, ...}`
- `devices.battery: { ... }` → append `{type: battery, id: battery, ...}`
- `devices.base_load_w: 500` → append `{type: base_load, id: base_load, baseline_kw: 0.5}`
- If a device was absent from the old profile, it is simply absent from the assets list
- Preserve all reactor/simulator/planner/packets sections unchanged

**Critical**: `test.yaml` has `ev.initial_soc: 0.05` — this must be preserved exactly for the planner FIRM/FLEXIBLE split BDD scenarios.

**Verification**: All four VEN containers start successfully with migrated profiles. Full BDD suite passes.

---

### Step 10 — Full BDD verification

**Goal**: Confirm all 123 scenarios / 801 steps pass on Pi4 with the refactored code.

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"
```

**Expected**: 0 failures. Any failure is a regression introduced by this refactor and must be fixed before the branch is considered complete.

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| BDD step definitions hardcode old `GET /sim` response field paths | Medium — steps would silently pass or fail on wrong fields | Search all step files for `sim` field references before running BDD; update step definitions to use `assets["ev"]` etc. |
| test.yaml EV initial_soc migration breaks planner tests | High — FIRM/FLEXIBLE split is SoC-sensitive | Verify `initial_soc: 0.05` is preserved verbatim in migrated test.yaml |
| sim_state.json on Pi4 has old format | Low — re-initializes from profile defaults | Log a warning on mismatch; document in quickstart |
| Setpoints bridge misses an asset when dispatcher writes to Setpoints | Medium — asset gets wrong setpoint | Audit dispatcher.rs for all Setpoints field writes; include battery setpoint in bridge |
| Per-asset EnergyCounter introduces energy double-counting with GridMeter | Low — two separate counters track different things | Per-asset counters track asset contribution; GridMeter tracks grid boundary; they are independent |
