# Contract: POST /sim/inject

**Branch**: `022-deterministic-test-env`
**Date**: 2026-05-12
**Endpoint**: `POST /sim/inject` on VEN instance (e.g., `http://ven-1:8211`)

---

## Semantics

Partial-merge body. Fields follow a three-way null pattern:

| JSON value | Effect |
|-----------|--------|
| Field absent | No change to current inject state |
| Field present as `null` | Clear override (revert to natural model / default) |
| Field present as number/bool | Activate override with that value |

---

## Request Body (updated schema)

```json
{
  "battery_soc":          <number | null>,
  "ev_soc":               <number | null>,
  "heater_temp_c":        <number | null>,
  "pv_irradiance":        <number | null>,
  "pv_irradiance_alpha":  <number | null>,
  "ev_plugged":           <bool   | null>,
  "ev_soc_target":        <number | null>,
  "heater_setpoint_c":    <number | null>,
  "heater_temp_min_c":    <number | null>,
  "heater_temp_max_c":    <number | null>,
  "ambient_temp_c":       <number | null>,
  "base_load_kw":         <number | null>,
  "base_load_alpha":      <number | null>,
  "grid_import_limit_kw": <number | null>,
  "grid_export_limit_kw": <number | null>,
  "pv_plan_kw":           <number | null>   // NEW in 022
}
```

### New field: `pv_plan_kw`

| Property | Value |
|----------|-------|
| Type | `number` (kW) or `null` |
| Range | ≥ 0.0 (values < 0 clamped to 0 inside the planner) |
| Effect | Replaces the time-varying irradiance model with a constant value for all 288 MILP planning slots |
| Triggers replan | **No** — consistent with `base_load_kw` |
| Visible in GET /sim/inject | Yes |

---

## Response

`204 No Content` on success.

---

## Example: zero out planning forecast

```http
POST /sim/inject
Content-Type: application/json

{
  "pv_irradiance": 0.0,
  "pv_plan_kw": 0.0
}
```

`pv_irradiance=0.0` — physics tick sees 0 irradiance (affects current-tick PV power).
`pv_plan_kw=0.0` — MILP planner sees 0 kW PV for all 288 forecast slots.

## Example: clear the forecast override

```http
POST /sim/inject
Content-Type: application/json

{ "pv_plan_kw": null }
```

Planner reverts to the natural irradiance + decay model from the next solve onward.
