# Plan A — Explicit Forecast / Plan Split

## Goal

Make the forecast-vs-plan distinction a first-class concept in the API and UI.
Currently the data is already correct (`baseline_kw` and `pv_forecast_kw` are forecast inputs;
`allocations` are plan outputs) but the API surface is inconsistent and the UI never renders a
clean power-stack chart that shows users what is _given_ vs what the optimizer _decided_.

---

## Background and Problem

`PlanTimeSlot` (entities/plan.rs:37) already carries both categories:

| Field | Category | Meaning |
|---|---|---|
| `baseline_kw` | **Forecast** | Background load regardless of decisions |
| `pv_forecast_kw` | **Forecast** | PV generation regardless of decisions |
| `surplus_available_kw` | derived forecast | PV − baseline when positive |
| `allocations: Vec<AssetAllocation>` | **Plan** | Optimizer decisions |
| `bat_charge_kw`, `bat_discharge_kw` | **Plan** | Battery (duplicate of allocations) |
| `net_import_kw`, `net_export_kw` | derived total | Forecast + plan combined |

Problems:
1. **Battery is duplicated** — `bat_charge_kw` / `bat_discharge_kw` exist as top-level fields AND inside `allocations`. Everything else (EV, WM, heater) is only in `allocations`.
2. **EV has no top-level field** — to read EV charging power in a slot the UI must iterate `allocations` and filter by `asset_id == "ev"`. Inconsistent with battery.
3. **UI never shows the stacked breakdown** — the planner chart (`PlanDecisionMatrix.tsx`) is a heat-map matrix, not a stacked power timeline. There is no chart showing baseline + EV + WM + battery stacked against PV.
4. **`net_import_kw` is opaque** — the total import figure is not decomposed in any visible way.

---

## Key Code References

### Backend
- `PlanTimeSlot` struct: `VEN/src/entities/plan.rs:37–86`
- `AssetAllocation` struct: `VEN/src/entities/plan.rs:18–33`
  - fields: `asset_id`, `power_kw`, `surplus_power_kw`, `grid_power_kw`, `marginal_value`, `cost_eur`, `co2_g`
- `translate_to_plan()`: `VEN/src/controller/milp_planner.rs:~1237`
  - fills `baseline_kw: inputs.p_base_kw[t]` (~line 1386)
  - fills `pv_forecast_kw: inputs.p_pv_kw[t]` (~line 1387)
  - fills `bat_charge_kw` / `bat_discharge_kw` from `sol.p_bat_ch_kw[t]` / `sol.p_bat_dis_kw[t]`
  - fills `allocations` with one entry per non-zero asset
- `MilpInputs`: `VEN/src/controller/milp_planner.rs:71–159`
  - `p_base_kw: Vec<f64>` — baseline, one value per slot
  - `p_pv_kw: Vec<f64>` — PV forecast, one value per slot

### Frontend
- `PlanTimeSlot` TypeScript type: `VEN/ui/src/api/types.ts:168–182`
- `AssetAllocation` TypeScript type: `VEN/ui/src/api/types.ts:158–166`
- Decision matrix: `VEN/ui/src/components/planner/PlanDecisionMatrix.tsx`
- Controller data builders: `VEN/ui/src/components/controller/dataBuilders.ts`
- Tariff/chart builders: `VEN/ui/src/components/controller/tariffBuilders.ts`
- Route for planner page: check `VEN/ui/src/pages/` for the planner/controller page

---

## What to Build

### Step 1 — Normalize `PlanTimeSlot` top-level plan fields (backend)

Remove the `bat_charge_kw` / `bat_discharge_kw` duplicates and replace with a clean
`planned_kw_by_asset: HashMap<String, f64>` map populated from `allocations`.

```rust
// entities/plan.rs — replace bat_charge_kw / bat_discharge_kw with:
#[serde(default)]
pub planned_kw_by_asset: std::collections::HashMap<String, f64>,
```

Populated in `translate_to_plan()`:

```rust
let planned_kw_by_asset = allocations.iter()
    .map(|a| (a.asset_id.clone(), a.power_kw))
    .collect();
```

Keep `bat_charge_kw` / `bat_discharge_kw` as `#[serde(default)]` computed aliases for backward
compat, or remove them if no consumer depends on them (grep first).

Why a map not individual fields: assets are dynamic (wm, ev, battery, heater, any future type).
Hard-coding each as a field means every new asset type requires a schema change.

### Step 2 — Add `forecast_kw` derived field (backend, optional — defer until a second forecast source exists)

> **Recommendation**: skip this step for now. `forecast_load_kw == baseline_kw` at all times
> until a second forecast input (e.g. heat-pump forecast) is introduced. Adding an alias
> field with zero new information adds struct noise and an AC with no real value.
> Revisit when a second forecast source is added.

If implemented:

```rust
/// Total forecast-driven load for this slot: baseline + any forecast-only assets.
/// Equals baseline_kw when only PV and base load are forecast inputs.
#[serde(default)]
pub forecast_load_kw: f64,   // = baseline_kw (extendable for heat-pump forecast later)
```

This makes the field naming symmetric: `forecast_load_kw` vs `pv_forecast_kw` vs `planned_kw_by_asset`.

### Step 3 — Frontend type update

Update `VEN/ui/src/api/types.ts`:
```typescript
export interface PlanTimeSlot {
  // ... existing fields ...
  planned_kw_by_asset?: Record<string, number>;  // optional: backend uses #[serde(default)], test fixtures omit it
  forecast_load_kw?: number;                     // optional for same reason
  // bat_charge_kw / bat_discharge_kw were never in the TS type — no removal needed here
}
```

> **Test fixture note**: Both `makeSlot()` factories in `PlanHeaderBar.test.tsx` and
> `PlannerPage.test.tsx` enumerate all `PlanTimeSlot` fields explicitly. Declaring
> `planned_kw_by_asset` as **required** would cause TypeScript compile errors in both.
> Keep it `?` (optional). In real API responses the field is always present because
> the Rust struct uses `#[serde(default)]`.

### Step 4 — Stacked power timeline chart (frontend, main deliverable)

Add a new chart component `PlanPowerStack.tsx` in `VEN/ui/src/components/planner/`.

Chart type: `recharts` `ComposedChart` with:
- X-axis: time (slot start)
- **Stacked area/bar (positive, load side)**:
  - `baseline_kw` — grey, "Base load (forecast)"
  - `planned_kw_by_asset["ev"]` — blue, "EV (planned)"
  - `planned_kw_by_asset["wm"]` — orange, "Washing machine (planned)"
  - `planned_kw_by_asset["heater"]` — red, "Heater (planned)"
  - battery charge — green, "Battery charge (planned)": render `max(0, planned_kw_by_asset["battery"] ?? 0)` (positive when charging; MILP mutual exclusivity guarantees charge and discharge are never both nonzero in the same slot)
- **Stacked area/bar (negative, generation side)**:
  - `-pv_forecast_kw` — yellow, "PV (forecast)"
  - battery discharge — teal, "Battery discharge (planned)": render `min(0, planned_kw_by_asset["battery"] ?? 0)` (negative when discharging)
- **Line**: `net_import_kw` — dark, "Net grid import"

Legend distinguishes forecast (dashed border) from plan (solid) — communicate this via color/label, not necessarily style.

Place this chart on the planner / controller page where the user can see the 24h schedule at a glance. It answers: "What will my grid import look like, and why?"

### Step 5 — No changes to `dataBuilders.ts`

`dataBuilders.ts` does **not** iterate `allocations` — it reads `sim.assets` (live simulator
snapshot). The `allocations` loop lives in `PlanDecisionMatrix.tsx` (lines 80, 92, 104) and
correctly stays there for the heat-map matrix.

The new `PlanPowerStack` chart (Step 4) reads `planned_kw_by_asset` directly from plan slots.
No refactor of `dataBuilders.ts` is required.

---

## Files to Modify

| File | Change |
|---|---|
| `VEN/src/entities/plan.rs` | Add `planned_kw_by_asset`, `forecast_load_kw`; remove `bat_*` fields |
| `VEN/src/controller/milp_planner.rs` | Populate new fields in `translate_to_plan()`; remove `bat_*` assignments (lines 1209–1210, 1394–1395) |
| `VEN/src/controller/timeline.rs` | Remove `bat_charge_kw: 0.0` / `bat_discharge_kw: 0.0` from two test fixture structs (lines 553–554, 1079–1080) |
| `VEN/ui/src/api/types.ts` | Add new fields to `PlanTimeSlot` |
| `VEN/ui/src/components/planner/PlanPowerStack.tsx` | New chart component |
| Planner page component | Add `PlanPowerStack` chart |
| `VEN/ui/src/components/controller/dataBuilders.ts` | Simplify to use `planned_kw_by_asset` |

---

## Acceptance Criteria

1. `GET /plan` response includes `planned_kw_by_asset` with correct values per asset per slot.
2. `GET /plan` response includes `forecast_load_kw == baseline_kw` for now.
3. `bat_charge_kw` / `bat_discharge_kw` are still present (backward compat) or removed only after confirming no BDD step reads them by name.
4. Planner page shows the stacked power chart: baseline, EV, WM, battery, PV clearly separated.
5. Chart legend distinguishes forecast assets (PV, base load) from planned assets (EV, WM, battery).
6. All existing BDD tests pass.

---

## Dependency

Plan B (shiftable load runtime) should be done first or in parallel — without WM sim state,
`planned_kw_by_asset["wm"]` will only be visible in the plan chart (future slots), not in the live sim chart.
That is acceptable for Plan A in isolation.
