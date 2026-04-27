# Implementation Plan: Planner State Forecast in Timeline API

**Branch**: `015-planner-state-forecast` | **Date**: 2026-04-27 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/015-planner-state-forecast/spec.md`

## Summary

The MILP planner already computes full state trajectories for battery (`e_bat_kwh`, n+1 points),
heater tank (`e_heat_tank_kwh`, n points), and EV charge power (`p_ev_kw`, n points). However,
none of these trajectories are stored per-slot in the `Plan` after translation — only `power_kw`
flows through to the timeline API. This feature stores per-asset future state (SoC, T_tank) in
each `PlanTimeSlot`, derived by each asset module from the MILP output at plan assembly time,
and surfaces them in the timeline API's future points.

## Technical Context

**Language/Version**: Rust stable 2021 (VEN backend)  
**Primary Dependencies**: `axum`, `tokio`, `serde`, `uuid`, `chrono`, `good_lp`/HiGHS  
**Storage**: In-memory only — `HashMap` per `PlanTimeSlot`; no DB changes  
**Testing**: `cargo test --workspace` (unit tests inside `#[cfg(test)]` blocks)  
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2  
**Project Type**: Rust library/service (VEN backend `VEN/src/`)  
**Performance Goals**: Negligible — per-slot map inserts (3 entries per slot, 288 slots) during plan assembly (~10 µs extra on Pi4)  
**Constraints**: Must not block the MILP solve or add measurable query latency to the timeline API; must compile with `SQLX_OFFLINE=true`

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. OpenADR Spec Fidelity | ✅ Pass | No OpenADR fields touched. New keys (`soc`, `temp_c`) are VEN-internal timeline values, consistent with existing historical key names. |
| II. BDD-First Testing | ⚠️ Partial | This feature is purely Rust-internal (VEN backend). BDD scenarios would require a full Docker stack + a running plan. Unit tests in cargo are the appropriate first layer. A minimal BDD scenario tag `@planner-state` can be added as a follow-up when a plan-smoke test exists. |
| III. Upstream Compatibility | ✅ N/A | Changes are in `VEN/src/` — not the `openleadr-rs` submodule. |
| IV. Lean Architecture | ✅ Pass | New field uses `#[serde(default)]`, no existing consumers break. Asset methods are minimal (single arithmetic expression each). No new abstractions. |
| V. Infrastructure Parity | ✅ Pass | No new Docker services or environment variables. Build and test via existing `cargo test` path. |

**BDD gate note**: The constitution requires BDD for "new behavior". This feature adds new JSON keys
to an existing API endpoint. A BDD step `Then the future battery points include a "soc" key` is
straightforward to add to the existing `timeline.feature`. A `@planner-state` scenario is included
in the tasks below and satisfies the gate.

## Research

### R-001 — How MILP state trajectories map to per-slot plan data

**Confirmed from codebase inspection:**

| Asset | MILP output variable | Length | Semantics |
|-------|---------------------|--------|-----------|
| Battery | `SolveOutput.e_bat_kwh` | n+1 | `e_bat_kwh[t]` = energy (kWh) at **start** of slot t (index 0 = initial) |
| Heater | `SolveOutput.e_heat_tank_kwh` | n | `e_heat_tank_kwh[t]` = tank energy above T_min (kWh) at **start** of slot t |
| EV | `SolveOutput.p_ev_kw` | n | Charge power per slot; no SoC trajectory variable in the MILP |

For consistency, all three assets will emit **start-of-slot** state at each plan timestamp.

### R-002 — EV initial SoC availability

**Problem**: `translate_to_plan` receives `inputs: &MilpInputs` and `ev_session: Option<&EvSession>`.
`MilpInputs.e_ev_core_kwh = ((target_soc − current_soc) × battery_kwh).max(0.0)`.
When `current_soc ≥ target_soc`, `e_ev_core_kwh = 0`, so we cannot back-derive the live SoC.

**Decision**: Add `soc_ev_init: Option<f64>` to `MilpInputs` — populated when an EV asset exists
in profile, carrying the live SoC snapshot at plan-build time. This is the single source of truth
for the initial SoC used in trajectory integration.

**Rationale**: Avoids ambiguity in the already-charged edge case. Single-field addition with
`#[serde(default)]` semantics; no existing callers need updating.

### R-003 — "Implement in the asset" — interpretation

**Decision**: Each asset module (`battery.rs`, `ev.rs`, `heater.rs`) owns the conversion logic
(energy → SoC, energy → temperature). `translate_to_plan` calls asset methods; the planner
remains unaware of the physics. This mirrors how `state_values()` is already asset-owned for
historical points.

Specifically:
- `Battery::future_state_values(&self, e_kwh: f64) → HashMap<String, f64>` — uses `self.capacity_kwh`
- `EvCharger::soc_trajectory(p_ev_kw: &[f64], soc_init: f64, battery_kwh: f64, dt_h: f64) → Vec<f64>` (n+1) — integration loop
- `EvCharger::future_state_values_at(soc: f64) → HashMap<String, f64>` — wraps clamped soc
- `Heater::future_state_values(&self, e_tank_kwh: f64) → HashMap<String, f64>` — uses `self.thermal_mass_kwh_per_c` and `self.temp_min_c`

`EvCharger::soc_trajectory` is a free/associated function (no `&self` needed) placed on `EvCharger`
for organisational clarity. `translate_to_plan` builds lightweight asset objects from profile config
(`Battery::from_config`, `EvCharger::from_config`, `Heater::from_config`), calls the methods, and
stores results in `PlanTimeSlot.planned_state_by_asset`.

### R-004 — Heater thermal_mass: profile vs live Heater struct

**Decision**: In `translate_to_plan`, use `Heater::from_config(heater_cfg)` to get a `Heater`
instance, then call `heater.future_state_values(e_tank_kwh)`. This ensures the same
`thermal_mass_kwh_per_c` and `temp_min_c` that the MILP used (derived from `HeaterConfig` via
`HeaterConfig::effective_thermal_mass()`) are used for conversion — no runtime overrides can
contaminate the plan-time values.

### R-005 — Timeline controller integration

**Decision**: In `build_asset_timeline` (timeline.rs ~line 345), after the existing `values.insert`
calls for `power_kw`, `cost_rate_eur_h`, `co2_rate_g_h`, add:
```rust
if let Some(state_map) = slot.planned_state_by_asset.get(asset_id) {
    values.extend(state_map.iter().map(|(k, v)| (k.clone(), *v)));
}
```
This is additive (no existing keys overwritten) and zero-cost when the map is empty.

### R-006 — No-plan / missing-heater edge cases

- When no plan is active: `plan` is `None` in `build_asset_timeline` — existing `if let Some(plan) = plan` guard covers this. No change needed.
- When heater is absent (`e_heat_tank_kwh` is empty vec in `SolveOutput`): guard `!sol.e_heat_tank_kwh.is_empty()` before calling `heater.future_state_values(...)`.
- When EV is MustNotRun (no session or unplugged): `soc_ev_init` is `None` — skip EV SoC trajectory entirely.

## Project Structure

### Documentation (this feature)

```text
specs/015-planner-state-forecast/
├── plan.md              # This file
├── research.md          # Inline above (R-001 through R-006)
├── data-model.md        # See below
├── contracts/
│   └── timeline-future-point.md   # Updated API contract
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code

```text
VEN/src/
├── entities/
│   └── plan.rs                    # Add planned_state_by_asset to PlanTimeSlot
├── assets/
│   ├── battery.rs                 # Add Battery::future_state_values
│   ├── ev.rs                      # Add EvCharger::soc_trajectory + future_state_values_at
│   └── heater.rs                  # Add Heater::future_state_values
└── controller/
    ├── milp_planner.rs            # MilpInputs: add soc_ev_init; translate_to_plan: populate planned_state_by_asset
    └── timeline.rs                # build_asset_timeline: merge planned_state_by_asset into future values
```

## Data Model

### Changed: `PlanTimeSlot` (VEN/src/entities/plan.rs)

Add one field to the existing struct:

```rust
/// Per-asset state values (SoC, temperature) at the start of this slot,
/// computed by each asset from the MILP solution at plan assembly time.
/// Key: asset_id → (key → value), e.g. "battery" → {"soc": 0.82}
/// Empty when the asset has no planned state (non-controllable assets, no plan).
#[serde(default)]
pub planned_state_by_asset: HashMap<String, HashMap<String, f64>>,
```

**Migration**: Field uses `#[serde(default)]` → `HashMap::new()`. Existing persisted JSON (no
such field) deserialises to an empty map. No schema change required.

### Changed: `MilpInputs` (VEN/src/controller/milp_planner.rs)

```rust
/// Live SoC of the EV at plan-build time [0.0..1.0].
/// None when no EV asset is present or EV is unplugged.
soc_ev_init: Option<f64>,
```

### New asset methods

#### `Battery::future_state_values(&self, e_kwh: f64) → HashMap<String, f64>`
- `soc = (e_kwh / self.capacity_kwh).clamp(0.0, 1.0)`
- Returns `{"soc": soc}`

#### `EvCharger::soc_trajectory(p_ev_kw: &[f64], soc_init: f64, battery_kwh: f64, dt_h: f64) → Vec<f64>`
- Returns `Vec<f64>` of length `n+1`; index 0 = `soc_init`
- `soc[t+1] = (soc[t] + p_ev_kw[t] * dt_h / battery_kwh).clamp(0.0, 1.0)`

#### `EvCharger::future_state_values_at(soc: f64) → HashMap<String, f64>`
- Returns `{"soc": soc.clamp(0.0, 1.0)}`

#### `Heater::future_state_values(&self, e_tank_kwh: f64) → HashMap<String, f64>`
- `temp_c = self.temp_min_c + e_tank_kwh / self.thermal_mass_kwh_per_c`
- Returns `{"temp_c": temp_c}`

## API Contract

### GET /timeline/:asset_id (and /timeline/all)

Future points (timestamp > now, from plan slots) now include additional keys in the `values` map:

| Asset type | Existing keys | New keys (when plan active) |
|------------|---------------|---------------------------|
| battery    | `power_kw`, `cost_rate_eur_h`, `co2_rate_g_h` | `soc` (f64, 0.0–1.0) |
| ev         | `power_kw`, `cost_rate_eur_h`, `co2_rate_g_h` | `soc` (f64, 0.0–1.0) |
| heater     | `power_kw`, `cost_rate_eur_h`, `co2_rate_g_h` | `temp_c` (f64, °C) |
| pv / base_load / grid | unchanged | (none) |

Historical points are unchanged. `values: null` slots (beyond plan horizon) are unchanged.

Full contract documented in `contracts/timeline-future-point.md`.

## Complexity Tracking

No constitution violations. This plan introduces no new abstractions beyond the minimum required.
