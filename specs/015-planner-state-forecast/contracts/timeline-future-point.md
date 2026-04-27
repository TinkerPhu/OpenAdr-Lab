# Contract: Timeline Future Point — Asset State Values

**Feature**: 015-planner-state-forecast  
**Endpoint**: `GET /timeline/:asset_id` and `GET /timeline/all`  
**Layer**: VEN HTTP API (axum)

## Overview

Timeline points whose `ts > now` (future plan points) gain additional keys in their `values` map
when an active MILP plan covers the corresponding time bucket. Historical points are unchanged.

## Future Point Structure

```json
{
  "ts": "2026-04-27T23:00:00Z",
  "values": {
    "power_kw":        0.0,
    "cost_rate_eur_h": 0.0,
    "co2_rate_g_h":    0.0
  }
}
```

### After this feature (controllable assets with active plan)

**Battery (`asset_id = "battery"` or configured id):**
```json
{
  "ts": "2026-04-27T23:00:00Z",
  "values": {
    "power_kw":        7.4,
    "cost_rate_eur_h": 0.18,
    "co2_rate_g_h":    120.0,
    "soc":             0.63
  }
}
```

**EV Charger (`asset_id = "ev"` or configured id):**
```json
{
  "ts": "2026-04-27T23:00:00Z",
  "values": {
    "power_kw":        11.0,
    "cost_rate_eur_h": 0.22,
    "co2_rate_g_h":    145.0,
    "soc":             0.55
  }
}
```

**Heater (`asset_id = "heater"` or configured id):**
```json
{
  "ts": "2026-04-27T23:00:00Z",
  "values": {
    "power_kw":        3.0,
    "cost_rate_eur_h": 0.15,
    "co2_rate_g_h":    98.0,
    "temp_c":          52.4
  }
}
```

## Key Definitions

| Key | Type | Range | Semantics |
|-----|------|--------|-----------|
| `soc` | f64 | [0.0, 1.0] | State of charge at the **start** of the plan slot (fraction of nameplate capacity) |
| `temp_c` | f64 | °C (physically bounded by heater config) | Tank temperature at the **start** of the plan slot |

## Invariants

1. **State is start-of-slot**: `soc` and `temp_c` reflect the asset state *entering* the slot,
   before any power is applied.
2. **Clamped SoC**: `soc` is always in [0.0, 1.0]; no float overflow reaches the API.
3. **Consistent with `power_kw`**: Both `soc`/`temp_c` and `power_kw` come from the same MILP
   solution. A new plan replaces all future points atomically.
4. **Absent when no plan**: If no MILP plan is active, future points contain only `power_kw`,
   `cost_rate_eur_h`, `co2_rate_g_h` (existing behaviour; `soc`/`temp_c` absent, not null).
5. **Non-controllable assets unaffected**: `pv`, `base_load`, `grid` virtual asset never gain
   `soc` or `temp_c` keys.
6. **Additive-only**: Existing keys are never renamed, removed, or reordered. Consumers that
   ignore unknown keys continue to work.

## Backward Compatibility

- Existing consumers of `power_kw`, `cost_rate_eur_h`, `co2_rate_g_h` are unaffected.
- The `values` map now contains additional optional keys — consumers must handle unknown keys
  gracefully (standard JSON parsing).
