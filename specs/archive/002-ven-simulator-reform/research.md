# Research: VEN Simulator Reform

**Branch**: `002-ven-simulator-reform`
**Date**: 2026-03-15

## Decision 1: Enum Method Dispatch Strategy

**Decision**: Match-based delegation — each method on `AssetState` is a `match self { ... }` that forwards to the inner type.

**Rationale**: The asset set is closed (5 known types). Exhaustive match means the compiler enforces every new variant to implement all methods. Trait objects (`Box<dyn Asset>`) require all methods to be object-safe, but `predict()` returns `Vec<(DateTime<Utc>, f64)>` which is fine — however `capabilities()` and `control_schema()` return concrete types by value, which is also fine. Despite trait objects being technically feasible, they add allocation overhead and hide exhaustiveness errors. A `match` on 5 variants is not verbose and directly reflects the spec's `enum AssetState` design. Aligns with Lean Architecture principle: no indirection without concrete need.

**Alternatives considered**:
- `Box<dyn Asset>` trait objects: rejected — adds heap allocation, prevents exhaustiveness checks, complicates serialization/deserialization via serde.
- Macro delegation: rejected — unnecessary complexity for 5 variants.

---

## Decision 2: YAML Deserialization for AssetConfig

**Decision**: Use serde's internally-tagged enum: `#[serde(tag = "type", rename_all = "snake_case")]` on `AssetConfig`. Each YAML asset entry includes a `type: ev|heater|pv|battery|base_load` field.

**Rationale**: This is the canonical Rust pattern for polymorphic YAML configuration. The `type` field is the discriminator; serde deserializes the remaining fields into the matched variant's struct. No custom deserializer code needed. The `rename_all = "snake_case"` ensures `type: base_load` maps to `AssetConfig::BaseLoad`.

**Alternatives considered**:
- Externally-tagged (`{ "Ev": { ... } }`): rejected — not idiomatic for human-written YAML config.
- Adjacently-tagged: rejected — more verbose in YAML and no advantage over internal tagging here.

---

## Decision 3: sim_state.json Migration Strategy

**Decision**: On load failure (old format incompatible with new schema), log a warning and reinitialize `SimState` from profile defaults. Do not attempt a forward-migration of the old JSON.

**Rationale**: `sim_state.json` holds running physics state (SoC values, temperatures). This state is ephemeral — losing it costs one simulation warm-up period, not real data. Writing a migration for an internal JSON format couples the old and new schemas, adding maintenance overhead for no user-visible benefit. The existing `persist.rs` already returns `None` on parse failure and falls back to profile defaults; this same behavior applies after the refactor.

**Alternatives considered**:
- Forward-migration (parse old format, convert to new): rejected — couples old/new schemas, fragile, not worth maintenance burden for ephemeral state.
- Version field in sim_state.json: rejected — overengineering for single-instance local state file.

---

## Decision 4: Setpoints Bridge (Reactor→Simulator Interface)

**Decision**: Add a thin conversion function at the simulator tick call site in `main.rs` that translates the reactor's named `Setpoints` struct into a `HashMap<String, f64>` keyed by asset ID. Asset IDs in profiles match the type names (`"ev"`, `"heater"`, `"pv"`, `"battery"`, `"base_load"`).

```
Setpoints.ev_charge_kw       → HashMap["ev"]
Setpoints.heater_kw          → HashMap["heater"]
Setpoints.pv_export_limit_kw → HashMap["pv"]  (None → asset default_setpoint)
Setpoints.battery_kw         → HashMap["battery"]
```
`base_load` is non-flexible and never receives a setpoint from the reactor; it uses `default_setpoint()` always.

**Rationale**: The reactor (`reactor/`) is explicitly out of scope for this refactor. Changing the reactor's output type to `HashMap<String, f64>` would pull reactor into scope. The bridge is one small conversion function. In speckit 2, when the reactor is refactored, the bridge is deleted and replaced by native HashMap output.

**Alternatives considered**:
- Change reactor output to HashMap now: rejected — out of scope; would make the diff larger and risk breaking reactor behavior.
- Keep Setpoints as the tick argument and unpack inside tick(): rejected — couples the generic simulator to the reactor's named struct, defeating the purpose of the refactor.

---

## Decision 5: Per-Asset Energy Tracking

**Decision**: Each `AssetEntry` carries its own `EnergyCounter` tracking cumulative `import_kwh` and `export_kwh` for that asset. The `GridMeter` carries separate grid-level `import_kwh`/`export_kwh` derived by summing all asset net power each tick.

**Rationale**: Per-asset energy counters enable the ledger and future per-asset reporting. The existing global `EnergyCounter` in `SimState` is migrated to `GridMeter`. Each actor's individual contribution is now tracked. The `EnergyCounter` type is reused unchanged — it moves from `simulator/mod.rs` into `simulator/energy.rs` (already exists) and is instantiated per asset entry plus once for the grid meter.

**Alternatives considered**:
- Single global counter with per-asset contribution map: rejected — more complex than two plain EnergyCounters (per-asset + grid).

---

## Decision 6: predict() Implementation Scope

**Decision**: `predict()` is implemented as a stub returning a single-point projection `[(now, setpoint)]` for all asset types in this speckit. The real physics-based prediction (using thermal model for heater, SoC trajectory for EV/battery) is implemented in speckit 2 when the planner is refactored to call it.

**Rationale**: The planner (`controller/planner.rs`) is out of scope for this refactor. Implementing full predict() physics before the caller exists would be untestable through BDD. A stub satisfies the interface contract while keeping the diff minimal.

**Alternatives considered**:
- Implement full predict() now: rejected — no caller exists in this speckit; untestable change.
- Omit predict() from AssetState entirely: rejected — the spec mandates the interface; omitting it breaks the speckit 2 contract.

---

## Decision 7: TickEnvironment Construction

**Decision**: Build `TickEnvironment` (`HashMap<String, f64>`) in the main tick loop with keys:
- `"hour_of_day"`: fractional hour derived from `chrono::Utc::now()`
- `"ambient_temp_c"`: from `UserOverrides.ambient_temp_c` (or default 8.0 °C if None)

Each asset reads what it needs and ignores the rest. This replaces the per-actor positional arguments currently passed in `actors.rs::update()` (e.g., `hour_of_day` and `irradiance_override` passed to PvInverter separately).

**Rationale**: A generic map decouples environment provisioning from asset interfaces. Adding a new environment variable (e.g., `"wind_speed_m_s"`) requires no interface change. The existing per-actor arguments are subsumed by the map keys.

**Alternatives considered**:
- Typed TickEnvironment struct: rejected — requires interface change each time a new env variable is added; map is simpler for a small closed set.

---

## Decision 8: base_load as Asset vs. Primitive

**Decision**: `base_load` becomes a first-class `AssetState::BaseLoad(BaseLoad)` variant, not a bare `f64`. `BaseLoad` actor has `baseline_kw: f64` and `current_kw: f64`. Its `update()` always returns `baseline_kw` (potentially overridden by `UserOverrides.base_load_w`). Its `default_setpoint()` returns `baseline_kw`. It is non-flexible.

**Rationale**: The spec includes `base_load.rs` in the asset module and `BaseLoad(BaseLoad)` in `AssetState`. Making it a proper actor avoids a special case in the tick loop. The `power_model.rs` simplification (sum of all asset powers) requires base_load to participate in the Vec<AssetEntry> like any other asset.

**Alternatives considered**:
- Keep `base_load_w: f64` as a primitive on SimState: rejected — the spec explicitly includes BaseLoad as an AssetState variant; keeping it as a primitive undermines the "adding a new type requires only new file + variant" extensibility criterion.
