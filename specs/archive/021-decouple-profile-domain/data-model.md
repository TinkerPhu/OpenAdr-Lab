# Data Model: Decouple PROFILE from Domain (Phase 4)

**Branch**: `021-decouple-profile-domain` | **Date**: 2026-05-11

All new structs are plain Rust types with no `serde` attributes, no YAML schema concerns, and a
`Default` implementation. They are the "injection contract" from the application layer into the
domain. This document shows: entity name, home file, fields, their types, defaults, and the
corresponding source field from `profile.rs`.

---

## New entities in `entities/planner_params.rs`

### `PlannerObjective` (moved from `profile.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlannerObjective {
    #[default]
    MinCost,
    MinGhg,
    MinGrid,
    MinImport,
    MaxRevenue,
    Custom,
}
```

| Field | Notes |
|-------|-------|
| — (enum) | Moved verbatim from `profile::PlannerObjective`. The `Serialize/Deserialize` derives are removed (domain type; profile.rs adds those on the config side). The `serde(rename_all = "snake_case")` attribute is removed. |

**Migration note**: `profile.rs` retains a `pub use crate::entities::planner_params::PlannerObjective;` bridge re-export during the transition, removed in the final cleanup task.

---

### `PlannerParams`

Single flat struct. All 28 fields from `PlannerConfig` — field names identical, `serde` attributes removed.

| Field | Type | Default | Source (`PlannerConfig`) |
|-------|------|---------|--------------------------|
| `plan_step_s` | `u64` | `300` | `plan_step_s` |
| `plan_horizon_h` | `u64` | `24` | `plan_horizon_h` |
| `replan_interval_s` | `u64` | `300` | `replan_interval_s` |
| `deviation_threshold_kw` | `f64` | `1.0` | `deviation_threshold_kw` |
| `deviation_trigger_ticks` | `u32` | `30` | `deviation_trigger_ticks` |
| `correction_min_kw` | `f64` | `0.2` | `correction_min_kw` |
| `w_energy` | `f64` | `1.0` | `w_energy` |
| `w_ghg` | `f64` | `0.0001` | `w_ghg` |
| `w_grid` | `f64` | `0.0` | `w_grid` |
| `c_bat_wear_eur_kwh` | `f64` | `0.03` | `c_bat_wear_eur_kwh` |
| `c_ev_startup_eur` | `f64` | `0.01` | `c_ev_startup_eur` |
| `c_bat_startup_eur` | `f64` | `0.01` | `c_bat_startup_eur` |
| `c_ev_ramp_eur_kw` | `f64` | `0.005` | `c_ev_ramp_eur_kw` |
| `c_bat_ramp_eur_kw` | `f64` | `0.005` | `c_bat_ramp_eur_kw` |
| `c_bat_ev_coexist_eur_kwh` | `f64` | `0.5` | `c_bat_ev_coexist_eur_kwh` |
| `w_viol` | `f64` | `1.0` | `w_viol` |
| `pen_imp_eur_kwh` | `f64` | `10_000.0` | `pen_imp_eur_kwh` |
| `pen_exp_eur_kwh` | `f64` | `10_000.0` | `pen_exp_eur_kwh` |
| `v_ev_extra_eur_kwh` | `f64` | `0.10` | `v_ev_extra_eur_kwh` |
| `w_tier_penalty_eur` | `f64` | `0.001` | `w_tier_penalty_eur` |
| `objective` | `PlannerObjective` | `PlannerObjective::MinCost` | `objective` |
| `plan_adoption_threshold_eur` | `f64` | `0.0` | `plan_adoption_threshold_eur` |
| `plan_adoption_decay_s` | `f64` | `0.0` | `plan_adoption_decay_s` |
| `phase2_epsilon_eur` | `f64` | `0.02` | `phase2_epsilon_eur` |

**Assembly** (`main.rs`):
```rust
let planner_params = PlannerParams {
    plan_step_s: profile.planner.plan_step_s,
    plan_horizon_h: profile.planner.plan_horizon_h,
    // ... all fields copied verbatim ...
    objective: profile.planner.objective,  // profile.rs re-export bridges type identity
};
```

---

### `AbsorberParams`

| Field | Type | Default | Source (`AbsorberConfig`) |
|-------|------|---------|---------------------------|
| `enabled` | `bool` | `false` | `enabled` |
| `dead_band_kw` | `f64` | `0.1` | `dead_band_kw` |
| `dead_band_clearing_ticks` | `usize` | `1` | `dead_band_clearing_ticks` |
| `assets` | `Vec<AbsorberAssetParams>` | `vec![]` | `assets` (each mapped to `AbsorberAssetParams`) |

### `AbsorberAssetParams`

| Field | Type | Default | Source (`AbsorberAssetConfig`) |
|-------|------|---------|--------------------------------|
| `id` | `String` | — | `id` |
| `priority` | `u8` | — | `priority` |
| `min_state_linger_s` | `u64` | `0` | `min_state_linger_s` |
| `ev_departure_guard_s` | `Option<u64>` | `None` | `ev_departure_guard_s` |

---

### `SimulatorParams`

| Field | Type | Default | Source (`SimulatorConfig`) |
|-------|------|---------|----------------------------|
| `tick_s` | `u64` | `1` | `tick_s` |
| `persist_every_s` | `u64` | `15` | `persist_every_s` |
| `report_interval_s` | `u64` | `60` | `report_interval_s` |

---

## New structs in `assets/` files

### `BatteryParams` in `assets/battery.rs`

| Field | Type | Default | Source (`BatteryConfig`) |
|-------|------|---------|--------------------------|
| `id` | `String` | `ASSET_BATTERY` | `id` |
| `capacity_kwh` | `f64` | `10.0` | `capacity_kwh` |
| `max_charge_kw` | `f64` | `5.0` | `max_charge_kw` |
| `max_discharge_kw` | `f64` | `5.0` | `max_discharge_kw` |
| `initial_soc` | `f64` | `0.5` | `initial_soc` |
| `round_trip_efficiency` | `f64` | `0.92` | `round_trip_efficiency` |
| `min_soc` | `f64` | `0.10` | `min_soc` |

---

### `EvParams` in `assets/ev.rs`

| Field | Type | Default | Source (`EvConfig`) |
|-------|------|---------|---------------------|
| `id` | `String` | `ASSET_EV` | `id` |
| `max_charge_kw` | `f64` | `7.4` | `max_charge_kw` |
| `max_discharge_kw` | `f64` | `0.0` | `max_discharge_kw` |
| `initial_soc` | `f64` | `0.5` | `initial_soc` |
| `battery_kwh` | `f64` | `60.0` | `battery_kwh` |
| `soc_target` | `f64` | `0.8` | `soc_target` |
| `default_charge_kw` | `f64` | `0.0` | `default_charge_kw` |
| `min_charge_kw` | `f64` | `1.4` | `min_charge_kw` |

---

### `HeaterParams` in `assets/heater.rs`

Optional fields in `HeaterConfig` are pre-resolved to their effective values at assembly time.
Only `mid_kw` stays optional because `None` is semantically significant (one-level vs two-level).

| Field | Type | Default | Source / Resolution |
|-------|------|---------|---------------------|
| `id` | `String` | `ASSET_HEATER` | `id` |
| `max_kw` | `f64` | `5.0` | `max_kw` |
| `temp_initial_c` | `f64` | `20.0` | `temp_initial_c` |
| `temp_min_c` | `f64` | `18.0` | `temp_min_c` |
| `temp_max_c` | `f64` | `23.0` | `temp_max_c` |
| `mid_kw` | `Option<f64>` | `None` | `mid_kw` (kept optional — controls two-level model) |
| `thermal_mass_kwh_per_c` | `f64` | `2.0` | `effective_thermal_mass()` |
| `k_loss_kw_per_c` | `f64` | `0.1` | `effective_k_loss()` |
| `draw_kw` | `f64` | `0.0` | `effective_draw_kw()` |
| `switching_penalty_eur` | `f64` | `0.01` | `effective_switching_penalty()` |

**Assembly** (`main.rs`):
```rust
HeaterParams {
    thermal_mass_kwh_per_c: heater_cfg.effective_thermal_mass(),
    k_loss_kw_per_c: heater_cfg.effective_k_loss(),
    draw_kw: heater_cfg.effective_draw_kw(),
    switching_penalty_eur: heater_cfg.effective_switching_penalty(),
    ..
}
```

---

### `PvParams` in `assets/pv.rs`

| Field | Type | Default | Source (`PvConfig`) |
|-------|------|---------|---------------------|
| `id` | `String` | `ASSET_PV` | `id` |
| `rated_kw` | `f64` | `5.0` | `rated_kw` |

**Note**: The `forecast_kw(ts: DateTime<Utc>)` method (currently on `PvConfig`) is moved to
`PvParams` — it depends only on `rated_kw` and the timestamp, making it a pure domain computation.

---

### `BaseLoadParams` in `assets/base_load.rs`

| Field | Type | Default | Source (`BaseLoadConfig`) |
|-------|------|---------|---------------------------|
| `id` | `String` | `ASSET_BASE_LOAD` | `id` |
| `baseline_kw` | `f64` | `0.5` | `baseline_kw` |

---

## `AssetParams` — domain enum in `entities/asset_params.rs`

The `simulator` and `persist` functions need to iterate over all configured assets heterogeneously.
`AssetParams` is a **sum type (enum)** defined in `entities/asset_params.rs` — a proper domain type,
alongside `PlannerObjective` and the other cross-cutting entities.

This placement is required by the dependency rule: `simulator/mod.rs` and `simulator/persist.rs`
accept `&[AssetParams]` in their function signatures, so the type must be importable from the domain
ring. Defining it in `main.rs` would force `simulator/` to import from the composition root
(application layer → inner ring), which inverts the dependency direction.

```rust
// VEN/src/entities/asset_params.rs
use crate::assets::{battery::BatteryParams, ev::EvParams, heater::HeaterParams,
                    pv::PvParams, base_load::BaseLoadParams};

#[derive(Debug, Clone)]
pub enum AssetParams {
    Battery(BatteryParams),
    Ev(EvParams),
    Heater(HeaterParams),
    Pv(PvParams),
    BaseLoad(BaseLoadParams),
}
```

| Variant | Inner type |
|---------|-----------|
| `Battery(BatteryParams)` | `BatteryParams` from `assets/battery.rs` |
| `Ev(EvParams)` | `EvParams` from `assets/ev.rs` |
| `Heater(HeaterParams)` | `HeaterParams` from `assets/heater.rs` |
| `Pv(PvParams)` | `PvParams` from `assets/pv.rs` |
| `BaseLoad(BaseLoadParams)` | `BaseLoadParams` from `assets/base_load.rs` |

**Design rationale**: An enum (not `Box<dyn Trait>`) keeps dispatch static, requires no heap
allocations, and avoids a new trait. Living in `entities/` means both `main.rs` (assembles the vec)
and `simulator/` (accepts the slice) import from the domain ring — no direction violation.

**Note on creation order**: `entities/asset_params.rs` imports the five asset Params types, so the
file's enum body can only be filled after T008–T012 define those structs. Create the file with a
stub in T007 (add `pub mod asset_params;` to `entities/mod.rs`) and complete the enum body as a
follow-up step within T007's scope once T008–T012 are done (or in a single batch after T012).

**Assembly** (`main.rs`):
```rust
use crate::entities::asset_params::AssetParams;

let asset_params: Vec<AssetParams> = profile.assets.iter().map(|cfg| match cfg {
    AssetConfig::Battery(c)  => AssetParams::Battery(BatteryParams { id: c.id.clone(), .. }),
    AssetConfig::Ev(c)       => AssetParams::Ev(EvParams { id: c.id.clone(), .. }),
    AssetConfig::Heater(c)   => AssetParams::Heater(HeaterParams {
        thermal_mass_kwh_per_c: c.effective_thermal_mass(), ..
    }),
    AssetConfig::Pv(c)       => AssetParams::Pv(PvParams { rated_kw: c.rated_kw, .. }),
    AssetConfig::BaseLoad(c) => AssetParams::BaseLoad(BaseLoadParams { baseline_kw: c.baseline_kw, .. }),
}).collect();
```

---

## Assembly — `main.rs` helper

A private function `fn domain_params_from_profile(profile: &Profile)` constructs all domain param
structs from the profile. It returns a named bundle or tuple. Callers in `main.rs` use the result
to initialise the simulator, absorber, and planner tasks. The `Profile` object remains in `AppCtx`
only for the `routes/hems.rs` use case (Phase 6 scope).

```
Profile
  └── domain_params_from_profile()
        ├── asset params (BatteryParams, EvParams, HeaterParams, PvParams, BaseLoadParams)
        ├── PlannerParams
        ├── AbsorberParams
        └── SimulatorParams
```

The `active_objective: Arc<RwLock<PlannerObjective>>` field in `AppCtx` initialises from
`planner_params.objective` (which mirrors `profile.planner.objective`). Runtime overrides via
watch-channel continue to work as before — the type just has a different import path.

---

## Entities NOT changed in Phase 4

| Entity | File | Why untouched |
|--------|------|---------------|
| `Profile` | `profile.rs` | Infrastructure ring; stays as YAML target |
| `SimState` | `simulator/mod.rs` | Constructors updated (FR-011) but the struct itself is unchanged |
| `GridConfig` | `profile.rs` | Grid limits reach milp_planner via `SimSnapshot` — no direct domain import |
| `PacketSeed` | `profile.rs` | Only consumed at startup by packet seeding code; out of scope |
| `AppCtx.profile` | `main.rs` | Kept for routes/hems.rs (Phase 6 scope) |
