# API Contracts: Simulator Endpoints

**Branch**: `002-ven-simulator-reform`
**Date**: 2026-03-15

These contracts define the HTTP interface changes introduced by this refactor.

---

## GET /sim — Simulator Snapshot

**Status**: Modified (response body structure changes)

### Response Body

```json
{
  "ts": "2026-03-15T14:00:00Z",
  "net_power_w": 1200.5,
  "import_w": 1200.5,
  "export_w": 0.0,
  "import_kwh": 45.3,
  "export_kwh": 2.1,
  "assets": {
    "ev": {
      "power_kw": 7.4,
      "values": {
        "soc_pct": 62.0,
        "plugged": 1.0,
        "current_kw": 7.4
      }
    },
    "heater": {
      "power_kw": 2.1,
      "values": {
        "temp_c": 20.5,
        "current_kw": 2.1
      }
    },
    "pv": {
      "power_kw": -5.0,
      "values": {
        "irradiance": 0.8,
        "current_kw": -5.0
      }
    },
    "battery": {
      "power_kw": -1.5,
      "values": {
        "soc_pct": 55.0,
        "current_kw": -1.5
      }
    },
    "base_load": {
      "power_kw": 0.5,
      "values": {
        "current_kw": 0.5
      }
    }
  }
}
```

### Breaking Changes from Previous Format

The previous response had named top-level device fields (`ev`, `heater`, `pv`, `battery`). The new format nests all device data under `assets` as a map. UI consumers of `GET /sim` must be updated to read `response.assets["ev"]` instead of `response.ev` etc.

**Note**: UI changes are out of scope for this speckit. The BFF and VTN UI do not consume `GET /sim` directly. The VEN UI simulation tab (speckit 3) will be updated when it consumes this endpoint.

---

## GET /sim/schema — Control Schema

**Status**: New endpoint

### Response Body

```json
{
  "ev": [
    {
      "key": "ev_desired_kw",
      "label": "Charge Rate",
      "kind": "Slider",
      "min": 0.0,
      "max": 11.0,
      "unit": "kW"
    },
    {
      "key": "ev_plugged",
      "label": "Plugged In",
      "kind": "Switch",
      "min": null,
      "max": null,
      "unit": ""
    },
    {
      "key": "ev_soc_target",
      "label": "SoC Target",
      "kind": "Slider",
      "min": 0.0,
      "max": 1.0,
      "unit": "%"
    }
  ],
  "heater": [
    {
      "key": "heater_max_kw",
      "label": "Max Heating Power",
      "kind": "NumberInput",
      "min": 0.0,
      "max": 10.0,
      "unit": "kW"
    }
  ],
  "pv": [
    {
      "key": "pv_irradiance",
      "label": "Irradiance Override",
      "kind": "Slider",
      "min": 0.0,
      "max": 1.0,
      "unit": ""
    },
    {
      "key": "pv_force_export_limit_kw",
      "label": "Export Limit",
      "kind": "NumberInput",
      "min": 0.0,
      "max": 20.0,
      "unit": "kW"
    }
  ],
  "battery": [
    {
      "key": "battery_force_kw",
      "label": "Force Power",
      "kind": "Slider",
      "min": -5.0,
      "max": 5.0,
      "unit": "kW"
    }
  ],
  "base_load": []
}
```

---

## POST /sim/reset/ev — Reset EV State

**Status**: New endpoint (replaces `ev_initial_soc` stub in UserOverrides)

### Request Body

```json
{ "soc": 0.8 }
```

| Field | Type | Constraints | Description |
|---|---|---|---|
| `soc` | `f64` | [0.0, 1.0] | New state of charge |

### Response

`200 OK` — empty body on success.
`400 Bad Request` — if `soc` is outside [0, 1] or asset `ev` is not configured.
`404 Not Found` — if asset `ev` is not present in the current profile.

### Behavior

Sets the EV actor's SoC directly (bypasses physics tick). Persists `sim_state.json` after the update. Change is visible in the next `GET /sim` response.

---

## POST /sim/reset/battery — Reset Battery State

**Status**: New endpoint (replaces `battery_initial_soc` stub in UserOverrides)

### Request Body

```json
{ "soc": 0.5 }
```

| Field | Type | Constraints | Description |
|---|---|---|---|
| `soc` | `f64` | [0.0, 1.0] | New state of charge |

### Response

`200 OK` — empty body on success.
`400 Bad Request` — if `soc` is outside [0, 1] or validation fails.
`404 Not Found` — if asset `battery` is not present in the current profile.

### Behavior

Sets the battery actor's SoC directly. Persists `sim_state.json` after the update.

---

## PUT /sim/config/battery — Update Battery Configuration

**Status**: New endpoint (replaces `battery_capacity_kwh` stub in UserOverrides)

### Request Body

```json
{ "capacity_kwh": 20.0 }
```

| Field | Type | Constraints | Description |
|---|---|---|---|
| `capacity_kwh` | `f64` | > 0.0 | New usable capacity |

### Response

`200 OK` — empty body on success.
`400 Bad Request` — if value is invalid.
`404 Not Found` — if asset `battery` is not present in the current profile.

### Behavior

Updates the battery actor's `capacity_kwh` config field in place. The physics model uses the new capacity from the next tick onward. Persists `sim_state.json` after the update.

---

## Unchanged Endpoints

All other simulator endpoints are unchanged in this refactor:

| Endpoint | Status |
|---|---|
| `GET /sim/override` | Unchanged (UserOverrides struct changes: 3 fields removed) |
| `POST /sim/override` | Unchanged (same behavior, 3 fields no longer accepted) |
| `GET /trace` | Unchanged |
| `GET /sensors` | Unchanged (sensor snapshot derives from SimState) |

### UserOverrides Fields Removed

The following fields are removed from the `POST /sim/override` body. Clients that previously sent these fields will have them silently ignored (standard serde behavior with `#[serde(deny_unknown_fields)]` absent, else 400).

- `ev_initial_soc` → use `POST /sim/reset/ev`
- `battery_initial_soc` → use `POST /sim/reset/battery`
- `battery_capacity_kwh` → use `PUT /sim/config/battery`
