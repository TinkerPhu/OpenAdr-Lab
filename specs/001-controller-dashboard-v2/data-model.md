# Data Model: VEN Controller Dashboard V2

> **Nomenclature**: **Tariff** = price per unit of energy (X/kWh). **Rate** = instantaneous flow per unit of time (X/h).
> The VEN API endpoint `GET /rates` and struct `RateSnapshot` return tariff data (per-kWh values). This naming predates the distinction and will be corrected in a future API rename (`/tariffs`, `TariffSnapshot`). In this document, display entities use correct nomenclature; API references note the current (misnamed) endpoint.

**Phase**: 1 — Design
**Date**: 2026-03-14
**Branch**: `001-controller-dashboard-v2`

---

## Display Entities

These are the UI-level entities — what the dashboard displays. They are derived from VEN API responses.

---

### AssetSummary

One per configured asset. Built from `/sim` + `/rates` + `/user-requests`.

| Field | Type | Derived From | Notes |
|---|---|---|---|
| `assetId` | `"ev" \| "heater" \| "pv" \| "battery" \| "base_load"` | constant / sim key | Stable identifier per asset type |
| `label` | `string` | constant | Display name: "EV", "Heater", "PV", "Battery", "Base Load" |
| `color` | `string` | constant (palette) | Fixed hex per assetId |
| `powerKw` | `number` | GET /sim | Signed: positive = import, negative = export |
| `costRateEurH` | `number` | Derived | `\|powerKw\| × current rate (import or export price)` |
| `co2RateGH` | `number` | Derived | `powerKw × co2_g_kwh` (sign-preserving) |
| `socPct` | `number \| null` | GET /sim | `ev.soc × 100` or `battery.soc × 100`; null for non-SoC assets |
| `forecastEnergyKwh` | `number \| null` | GET /plan | Sum of allocations for this asset within visible time window; null if no plan data |
| `activeRequest` | `UserRequestSummary \| null` | GET /user-requests | Closest/largest active request; null if none |

---

### UserRequestSummary

Subset of UserRequest fields for display in asset cell left section.

| Field | Type | Source | Notes |
|---|---|---|---|
| `requestedEnergyKwh` | `number` | `/user-requests[].target_energy_kwh` | |
| `dueTime` | `Date` | `/user-requests[].value_curve.deadline_tiers[active_tier_index].deadline` | |

**Selection logic**: Among all non-terminal requests for `assetId`, pick the one with the earliest `dueTime` that is still active (not yet passed). If multiple are simultaneously active (dueTime in the past but not yet abandoned), pick the one with the largest `requestedEnergyKwh`.

---

### AssetTimePoint

One entry per time step in the asset's timeline graph. Built from `/trace` (past) and `/plan` (future).

| Field | Type | Source | Notes |
|---|---|---|---|
| `ts` | `number` | trace entry timestamp / slot start (ms epoch) | X-axis value |
| `powerKw` | `number \| null` | trace setpoint or plan allocation | null if no data for this timestamp |
| `costRateEurH` | `number \| null` | Derived from powerKw × rate at ts | null if no rate data |
| `co2RateGH` | `number \| null` | Derived from powerKw × co2_g_kwh at ts | null if no rate data |
| `isPast` | `boolean` | `ts < Date.now()` | Used to distinguish solid vs. dashed rendering |

**Graph rendering**:
- Past points (`isPast = true`): solid line
- Future points (`isPast = false`): dashed line
- Missing points: `connectNulls={false}` — gaps shown where no data exists

---

### TariffSnapshot (current)

Current tariff conditions for the Tariff Cell left section. Built from `/rates` current interval + `/sim`.

| Field | Type | Source | Notes |
|---|---|---|---|
| `importPriceEurKwh` | `number \| null` | GET /rates, current interval | |
| `exportPriceEurKwh` | `number \| null` | GET /rates, current interval | |
| `co2GKwh` | `number \| null` | GET /rates, current interval | CO₂eq **tariff** (g/kWh) — not a rate |
| `totalCostRateEurH` | `number` | Derived | `net_power_kw × applicable price` |
| `gridPowerKw` | `number` | GET /sim | `net_power_w / 1000` |

---

### TariffTimePoint

One entry per rate interval for the Tariff Cell right section graph. Built from `/rates` + `/trace` (for past grid power).

| Field | Type | Source |
|---|---|---|
| `ts` | `number` | `interval_start` (ms epoch) |
| `importPriceEurKwh` | `number \| null` | `import_price_eur_kwh` |
| `exportPriceEurKwh` | `number \| null` | `export_price_eur_kwh` |
| `co2GKwh` | `number \| null` | `co2_g_kwh` | CO₂eq **tariff** (g/kWh) — graph y-axis label should reflect per-kWh unit |
| `totalCostRateEurH` | `number \| null` | Derived at each interval |
| `gridPowerKw` | `number \| null` | From /trace (past) or /plan net_import_kw (future) |
| `isForecast` | `boolean` | `rate.is_forecast` |

**Graph line styles** (per spec FR-029):
- Import tariff: red dashed
- Import CO₂eq: red dotted
- Export tariff: green dashed
- Total cost rate: black dashed
- Grid power: black solid

---

### StackedAreaPoint

One entry per time step for the Accumulated Asset Power Cell stacked area chart. Built from `/trace` (past) + `/plan` (future).

| Field | Type | Source | Notes |
|---|---|---|---|
| `ts` | `number` | trace ts / plan slot start (ms epoch) | |
| `ev_pos` | `number` | `Math.max(0, ev_power_kw)` | Positive contribution to grid import |
| `ev_neg` | `number` | `Math.min(0, ev_power_kw)` | Negative contribution (export) |
| `heater_pos` | `number` | `Math.max(0, heater_power_kw)` | |
| `heater_neg` | `number` | `Math.min(0, heater_power_kw)` | |
| `pv_pos` | `number` | `Math.max(0, pv_power_kw)` | PV is almost always negative (exporting) |
| `pv_neg` | `number` | `Math.min(0, pv_power_kw)` | |
| `battery_pos` | `number` | `Math.max(0, battery_power_kw)` | |
| `battery_neg` | `number` | `Math.min(0, battery_power_kw)` | |
| `base_load_pos` | `number` | `base_load_w / 1000` (always ≥ 0) | Base load always imports |
| `base_load_neg` | `number` | `0` (constant) | Base load never exports |

**Stacking**: `_pos` series share `stackId="positive"` (stack above x-axis). `_neg` series share `stackId="negative"` (stack below x-axis). Sum of all ≈ grid power (FR-034).

---

### SimulationControls

Controls state for the right section of an asset cell. Read from `/sim` (current values) and `/sim/override` (current overrides).

**EV Controls**:
| Control | Read Source | Write Field | Type |
|---|---|---|---|
| Plugged in toggle | `/sim/override.ev_plugged` | `ev_plugged` | `boolean` |
| SoC (direct set, one-shot) | `/sim.ev.soc × 100` | `ev_initial_soc` (stub) | `0.0–1.0` |
| Max charge kW | `/sim/override.ev_max_charge_kw` | `ev_max_charge_kw` | `number` |
| SOC target | `/sim/override.ev_soc_target` | `ev_soc_target` | `0.0–1.0` |
| Force power kW | `/sim/override.ev_force_kw` | `ev_force_kw` | `number \| null` |

**Battery Controls**:
| Control | Read Source | Write Field | Type |
|---|---|---|---|
| Force charge/discharge kW | `/sim/override.battery_force_kw` | `battery_force_kw` | `number \| null` |
| SoC (direct set, one-shot) | `/sim.battery.soc × 100` | `battery_initial_soc` (stub) | `0.0–1.0` |
| Capacity kWh | `/sim.battery.capacity_kwh` | `battery_capacity_kwh` (stub) | `number` |

**Heater Controls**:
| Control | Read Source | Write Field | Type |
|---|---|---|---|
| Power override kW | `/sim/override.heater_force_kw` | `heater_force_kw` | `number \| null` |
| Max kW | `/sim/override.heater_max_kw` | `heater_max_kw` | `number` |
| Temp range | `/sim/override.heater_temp_min_c`, `heater_temp_max_c` | same | `number` each |
| Ambient temp | `/sim/override.ambient_temp_c` | `ambient_temp_c` | `number \| null` |

**PV Controls**:
| Control | Read Source | Write Field | Type |
|---|---|---|---|
| Export limit kW | `/sim/override.pv_force_export_limit_kw` | `pv_force_export_limit_kw` | `number \| null` |
| Irradiance (0–1) | `/sim/override.pv_irradiance` | `pv_irradiance` | `number \| null` |
| Rated kW | `/sim/override.pv_rated_kw` | `pv_rated_kw` | `number` |

**BaseLoad Controls**:
| Control | Read Source | Write Field | Type |
|---|---|---|---|
| Base load W | `/sim/override.base_load_w` | `base_load_w` | `number` |

---

## Backend Stub: UserOverrides Extensions

Three fields to add to `UserOverrides` struct in `VEN/src/state.rs`:

```rust
// In UserOverrides struct:
pub ev_initial_soc: Option<f64>,         // One-shot: jump EV SoC to value, then clear
pub battery_initial_soc: Option<f64>,    // One-shot: jump battery SoC to value, then clear
pub battery_capacity_kwh: Option<f64>,   // Persistent: override battery capacity_kwh
```

Application logic in simulator tick handler (VEN/src/simulator/):
- If `ev_initial_soc.is_some()`: set `ev_state.soc = value`; clear field from shared state
- If `battery_initial_soc.is_some()`: same for battery
- If `battery_capacity_kwh.is_some()`: use as battery `capacity_kwh` for all calculations

---

## UI State Model

Ephemeral state managed in React (not persisted to API).

### PinnedState

```typescript
type PinnedState = {
  pinnedCellIds: string[];   // Ordered list of pinned cell IDs (insertion order)
};
```

- Cell ID format: `"asset:{assetId}"` or `"grid:tariff"` or `"grid:accumulated"`
- Pinning prepends to pinned zone (most recently pinned at bottom of zone per FR-008)

### CollapseState

```typescript
type CollapseState = Record<string, {
  leftCollapsed: boolean;   // FR-014
  rightCollapsed: boolean;  // FR-026
}>;
```

Keyed by cell ID. Defaults to all `false` (expanded).

---

## Data Refresh Intervals

Follows existing VEN UI refresh patterns:

| Endpoint | Refresh | Rationale |
|---|---|---|
| GET /sim | 10s | Current device state |
| GET /sim/override | staleTime: ∞ | Only changes on user POST |
| GET /trace | 10s | Near-real-time reactor history |
| GET /rates | 30s | Rate intervals change infrequently |
| GET /plan | 10s | Plan updates on DR events |
| GET /packets | 10s | Packet status changes |
| GET /user-requests | 10s | User request status |

All using existing hooks from `VEN/ui/src/api/hooks.ts` — no new hooks needed except optionally `useSimOverride()` already exists.
