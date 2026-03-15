# Research: VEN Controller Dashboard V2

> **Nomenclature**: **Tariff** = X/kWh (price per unit energy). **Rate** = X/h (instantaneous flow per hour).
> The API endpoint `GET /rates` and struct `RateSnapshot` use "rate" loosely to mean tariff (per-kWh values). This document uses the correct terms throughout and notes the API name where needed.

**Phase**: 0 — Resolve Unknowns
**Date**: 2026-03-14
**Branch**: `001-controller-dashboard-v2`

---

## 1. VEN API Coverage — What Exists vs. What Is Needed

### Decision: Use existing endpoints; add 3 stub fields to UserOverrides

All displayed values are derivable from existing VEN API endpoints. No new GET endpoints required.

| Displayed Value | Source Endpoint | Notes |
|---|---|---|
| Asset current power [kW] | GET /sim | `ev.current_kw`, `heater.current_kw`, `pv.current_kw` (negated for export), `battery.current_kw`, `base_load_w/1000` |
| Asset cost rate [€/h] | GET /sim + GET /rates | Derived: `\|power_kw\| × (import or export price)` |
| Asset CO₂eq rate [g/h] | GET /sim + GET /rates | Derived: `power_kw × co2_g_kwh` |
| Asset SoC | GET /sim | `ev.soc`, `battery.soc` |
| Forecast energy in graph [kWh] | GET /plan | Sum of allocation `power_kw × slot_duration` for visible window |
| Active user request | GET /user-requests | Filter by asset_id; sort by energy/deadline |
| Asset past power (graph) | GET /trace | Per-asset setpoints per reactor tick |
| Asset future power (graph) | GET /plan | `firm_slots` + `flexible_slots` allocations per asset |
| Import/export tariff [€/kWh] | GET /rates | `import_price_eur_kwh`, `export_price_eur_kwh` |
| CO₂eq intensity [g/kWh] | GET /rates | `co2_g_kwh` |
| Grid power [kW] | GET /sim | `net_power_w / 1000` |
| Tariff time series | GET /rates | All intervals (is_forecast=false for past, true for future) |
| Simulation settings (read) | GET /sim/override | All UserOverrides fields |
| Simulation settings (write) | POST /sim/override | Full-replace semantics |
| Battery capacity (write) | **STUB** | Add `battery_capacity_kwh` to UserOverrides |
| Direct SoC set (EV) | **STUB** | Add `ev_initial_soc` to UserOverrides |
| Direct SoC set (Battery) | **STUB** | Add `battery_initial_soc` to UserOverrides |

**Alternatives considered**: New dedicated `/controller-v2/assets/{id}/history` endpoints. Rejected: unnecessary complexity; existing endpoints provide all required data.

---

## 2. Per-Asset Power Time Series

### Decision: Past from /trace; Future from /plan allocations

**Past half (left of "now" line)**:
- Source: `GET /trace?limit=500`
- Controller.tsx already builds per-asset chart data from trace entries via `buildPowerChartData()`
- Each TraceEntry carries a timestamp and per-asset setpoints (confirmed by Controller.tsx LineChart usage)
- 500 entries at ~1s tick rate = ~8 minutes of history

**Future half (right of "now" line)**:
- Source: `GET /plan` → `firm_slots` + `flexible_slots`
- Each slot has `start`, `end`, and `allocations: [{asset_id, power_kw}]`
- Convert to per-asset time-series points at slot boundaries

**"Now" line**: Current timestamp rendered as a recharts `ReferenceLine` at `x={Date.now()}`.

**Rationale**: Controller.tsx already follows this exact pattern for `PowerChart`. The new page extends it with per-asset cell isolation.

**Note**: 500 trace entries covers ~8 minutes of actual history. The spec assumption A-002 (1h past window) may result in sparse data at the left edge; recharts will render correctly with sparse data (gaps or interpolated lines depending on `connectNulls` setting). This is acceptable behavior per the clarification "only draw available data."

---

## 3. Cost Rate and CO₂eq Rate Derivation

### Decision: Frontend-side computation from power × current tariff interval

```
// For any asset at time t:
const tariff = findCurrentTariff(tariffs, t);  // GET /rates → nearest tariff interval

cost_rate_eur_h = power_kw >= 0
  ? power_kw * (tariff.import_price_eur_kwh ?? 0)     // import tariff [€/kWh] × power [kW] = rate [€/h]
  : Math.abs(power_kw) * (tariff.export_price_eur_kwh ?? 0)

co2_rate_g_h = power_kw * (tariff.co2_g_kwh ?? 0)   // CO₂ tariff [g/kWh] × power [kW] = CO₂ rate [g/h]
```

**Unit**: Display CO₂eq rate as `g CO₂eq/h`. At household scale (1–5 kW, 200–500 g/kWh grid intensity), values range 200–2500 g/h — readable without scientific notation.

**Total cost rate (Tariff Cell)**: `net_power_kw × applicable_tariff` where net = sum of all asset powers.

**Rationale**: Simple multiplication. Consistent with how Controller.tsx derives cost fields from packet allocations.

---

## 4. Backend Stubs — Three UserOverrides Fields

### Decision: Add ev_initial_soc, battery_initial_soc, battery_capacity_kwh to VEN/src/state.rs

These three simulation characteristics are required by the spec but absent from the existing `UserOverrides` struct:

| Stub Field | Type | Behavior |
|---|---|---|
| `ev_initial_soc` | `Option<f64>` | One-shot: when Some, simulator jumps EV SoC to this value on next tick, then field is cleared |
| `battery_initial_soc` | `Option<f64>` | One-shot: same pattern for battery SoC |
| `battery_capacity_kwh` | `Option<f64>` | Persistent: overrides battery `capacity_kwh` while set |

**Implementation scope**: Add 3 fields to `UserOverrides` struct in `VEN/src/state.rs`. Apply in the simulator tick handler. No new HTTP routes; `/sim/override` GET and POST already handle the full struct.

**Alternatives considered**:
- Separate `/sim/soc` endpoint. Rejected: adds a new route; struct extension is simpler.
- Frontend-only mock (show slider but don't POST). Rejected: controls would have no actual effect on simulation, violating FR-027.

**Existing coverage** (no stubs needed): `ev_plugged`, `ev_force_kw`, `ev_max_charge_kw`, `ev_soc_target`, `heater_force_kw`, `heater_max_kw`, `heater_temp_min_c`, `heater_temp_max_c`, `battery_force_kw`, `pv_force_export_limit_kw`, `pv_rated_kw`, `base_load_w`, `ambient_temp_c`, `pv_irradiance`.

---

## 5. Stacked Area Chart — Bidirectional Stacking

### Decision: Split each asset into positive and negative series with dual stackId

recharts supports `stackId` grouping on `Area` components. To achieve positive-above / negative-below:

```typescript
// For each asset at each time point, pre-compute two fields:
const posKey = `${assetId}_pos`;   // Math.max(0, power_kw)
const negKey = `${assetId}_neg`;   // Math.min(0, power_kw)

// In AreaChart:
<Area dataKey={posKey} stackId="positive" fill={assetColor} stroke={assetColor} />
<Area dataKey={negKey} stackId="negative" fill={assetColor} stroke={assetColor} />
```

Assets that switch sign (e.g., battery charging vs. discharging) naturally appear on both sides at different time points. The sum of all positive contributions and all negative contributions equals grid power.

**Rationale**: Standard recharts bidirectional stacking pattern. Recharts v2+ stacks negative values downward when using `stackId`. No custom rendering required.

**Alternatives considered**: Two separate `AreaChart` components (one positive, one negative). Rejected: synchronizing their X-axes is complex; a single chart is cleaner.

---

## 6. Right Section Space Management

### Decision: Two MUI Accordion groups — Status Settings expanded by default, Simulation Characteristics collapsed — within an expanded right section

**Layout**:
- Right section: toggleable (expand/collapse the whole panel) — defaults to expanded
- Inside: two `Accordion` groups
  - **Status Settings** (expanded by default): SoC slider, power on/off toggle
  - **Simulation Characteristics** (collapsed by default): capacity, power limits, delays
- Controls per asset type:
  - EV: plugged toggle, SoC slider (stub), max charge kW, SOC target
  - Battery: force kW, SoC slider (stub), capacity kWh (stub), max charge/discharge kW
  - Heater: force kW, max kW, temp min/max dual slider, ambient temp
  - PV: export limit, irradiance, rated kW
  - BaseLoad: base load W override

**Vertical space estimate**: Each accordion ~40px header + ~160px content = ~200px per group when open. Default state (Status Settings open, Sim Characteristics closed) = ~240px. Both open = ~440px. This fits within a reasonable cell height for the default state.

**Alternatives considered**: Tabbed interface for the two groups. Rejected: tabs add navigation complexity; accordions are consistent with MUI patterns already used in the codebase.

---

## 7. Asset Identification and Color Palette

### Decision: Asset presence from /sim null checks; fixed color palette

Assets are detected from `GET /sim`:
- EV: `sim.ev !== null`
- Heater: `sim.heater !== null`
- PV: `sim.pv !== null`
- Battery: `sim.battery !== null`
- BaseLoad: always present (`sim.base_load_w`)

**Color palette** (fixed per asset type — includes current assets, expected near-future assets, and reserve slots):
| Asset | Hex | Rationale |
|---|---|---|
| EV | `#2196F3` | Blue — cool, electric |
| Heater | `#FF5722` | Deep orange — heat |
| PV | `#FFC107` | Amber — sun |
| Battery | `#9C27B0` | Purple — storage |
| BaseLoad | `#607D8B` | Blue-grey — neutral baseline |
| WashingMachine | `#00BCD4` | Cyan — water/laundry |
| Dishwasher | `#009688` | Teal — water/kitchen |
| Stove | `#F44336` | Red — high heat, cooking |
| CoffeeMachine | `#795548` | Brown — coffee |
| Reserve 1 | `#4CAF50` | Green |
| Reserve 2 | `#FF9800` | Orange |
| Reserve 3 | `#E91E63` | Pink |
| Reserve 4 | `#3F51B5` | Indigo |
| Reserve 5 | `#8BC34A` | Light green |

Colors are used for: asset cell left-border accent, all graph lines in the asset's mid section, and the stacked area chart in the Accumulated Power cell.

**Alternatives considered**: Dynamic color generation from palette library. Rejected: unstable (ordering-dependent); fixed assignment is reproducible.

---

## 8. Baseline / No-Asset Fallback

### Decision: BaseLoad is always present in normal operation; /sim errors show an error state, not a placeholder

`GET /sim` always returns `base_load_w: f64`. Even if `ev`, `heater`, `pv`, and `battery` are all null, `base_load_w` is always non-null. Therefore a "BaseLoad" cell is always shown under normal operation.

If `GET /sim` itself fails (network error, VEN unreachable, HTTP error), the page MUST display an error state (via TanStack Query's `isError` flag) — not an empty "Baseline" placeholder cell. The placeholder is only relevant if the API responds successfully but reports no assets at all, which is not the current API's behavior.

---

## 9. BDD Test Scope

### Decision: 4 feature files, Playwright-driven E2E via VEN test-runner Docker

| Feature File | Acceptance Scenarios Covered |
|---|---|
| `01_layout.feature` | Grid cells appear above asset cells; page is scrollable; grid cells scroll with page |
| `02_asset_cells.feature` | Power/cost/CO₂ values visible; NOW line present; solid/dashed/dotted lines; sign below x-axis |
| `03_simulation_controls.feature` | EV plugged toggle visible; SoC slider visible; POST /sim/override triggered |
| `04_navigation.feature` | Pin cell → stays in viewport while scrolling; unpin → returns to position; collapse left/right sections |

Tests run via `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/controller_v2/`.

---

## 10. Routing and Navigation

### Decision: New route /controller-v2 with "Controller V2" nav tab

- Add `<Route path="/controller-v2" element={<ControllerV2Page />} />` to App.tsx
- Add nav link "Controller V2" to AppBar alongside existing tabs
- No changes to existing `/controller` route

**Transition plan**: V2 lives alongside V1 until it passes all acceptance tests and is approved for replacement. Replacement (removing V1 route) is a follow-up task, not in this feature's scope.
