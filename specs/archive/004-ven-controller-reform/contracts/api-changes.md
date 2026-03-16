# API Contracts: VEN Controller Reform (004)

## Removed Endpoints

### `GET /trace` — REMOVED

Previously returned `Vec<TraceEntry>` (reactor decision log). Replaced by two new endpoints below.

### `GET /rates` — REMOVED

Renamed to `GET /tariffs`. All callers (BFF, UI, BDD tests) must update.

---

## New Endpoints

### `GET /trace/events`

Returns the `ControllerEventLog` ring buffer as a flat list, newest first.

**Query parameters**: `limit` (optional, default 100, max 500)

**Response**: `200 OK`
```json
[
  {
    "type": "PlanCycle",
    "ts": "2026-03-15T10:00:05Z",
    "trigger_reason": "RateChange",
    "firm_slots": 3,
    "flexible_slots": 2
  },
  {
    "type": "OpenAdrArrived",
    "ts": "2026-03-15T10:00:00Z",
    "event_name": "peak-reduction-1",
    "signal_type": "IMPORT_CAPACITY_LIMIT",
    "value": 5.0,
    "interval": 0
  },
  {
    "type": "PacketTransition",
    "ts": "2026-03-15T09:55:00Z",
    "packet_id": "550e8400-e29b-41d4-a716-446655440000",
    "asset_id": "ev",
    "from_status": "Scheduled",
    "to_status": "Active"
  }
]
```

**Errors**: None expected (returns empty array if no events logged yet).

---

### `GET /trace/history`

Returns time-series rows from the `AssetHistoryBuffer` for a named asset.

**Query parameters**:
- `asset` (required) — asset ID string (e.g. `ev`, `battery`, `heater`, `pv`, `grid`)
- `limit` (optional, default 100, max 1000)

**Response**: `200 OK`
```json
[
  {
    "ts": "2026-03-15T10:00:05Z",
    "values": {
      "power_kw": 7.2,
      "soc_pct": 42.5,
      "cost_rate_eur_h": 1.44,
      "co2_rate_g_h": 2160.0
    }
  },
  {
    "ts": "2026-03-15T10:00:04Z",
    "values": {
      "power_kw": 6.8,
      "soc_pct": 42.3,
      "cost_rate_eur_h": 1.36,
      "co2_rate_g_h": 2040.0
    }
  }
]
```

For the `grid` asset, additional keys are present in `values`:
```json
{
  "power_kw": 3.1,
  "import_price_eur_kwh": 0.20,
  "export_price_eur_kwh": 0.05,
  "import_limit_kw": 5.0,
  "export_limit_kw": null,
  "cost_rate_eur_h": 0.62,
  "co2_rate_g_h": 930.0
}
```

**Errors**:
- `404 Not Found` — if `asset` parameter is missing or the named asset has no history rows yet.

---

## Modified Endpoints

### `GET /tariffs` (renamed from `GET /rates`)

No structural change to the response — only the route path changes.

**Response**: `200 OK` — same structure as former `GET /rates`
```json
[
  {
    "interval_start": "2026-03-15T10:00:00Z",
    "interval_end": "2026-03-15T11:00:00Z",
    "import_price_eur_kwh": 0.20,
    "export_price_eur_kwh": 0.05,
    "co2_g_kwh": 300.0,
    "source_event_id": "evt-abc123",
    "is_forecast": false
  }
]
```

---

### `POST /sim/override` — body format changed

**Before**: typed `UserOverrides` struct with force-override fields.

**After**: generic `HashMap<String, f64>` — only keys matching the control schema or environmental overrides are applied; unknown keys are silently ignored.

**Request body**:
```json
{
  "ambient_temp_c": 15.0,
  "pv_irradiance": 0.8,
  "ev_desired_kw": 7.0,
  "ev_soc_target": 0.9
}
```

**Removed keys** (rejected if present, or silently ignored):
- `ev_force_kw`
- `heater_force_kw`
- `battery_force_kw`
- `pv_force_export_limit_kw`

**Available environmental/device-spec keys**:
- `pv_irradiance` (0.0–1.0)
- `ambient_temp_c`
- `ev_desired_kw`
- `ev_plugged` (0.0 = false, 1.0 = true)
- `ev_max_charge_kw`
- `ev_soc_target`
- `heater_max_kw`
- `heater_temp_min_c`
- `heater_temp_max_c`
- `pv_rated_kw`
- `base_load_w`

**Response**: `204 No Content` (unchanged)

---

## Unchanged Endpoints

All other VEN endpoints are unaffected by this speckit:
- `GET /health`, `GET /metrics`
- `GET /events`, `GET /programs`, `GET /reports`, `POST /reports`, `PUT /reports/:id`
- `GET /sim`, `GET /sim/schema`, `POST /sim/reset/:asset_id`, `PUT /sim/config/battery`, `GET /sim/override`
- `GET /packets`, `POST /packets`
- `GET /plan`
- `GET /capacity`, `GET /obligations`
- `GET /ledger`
- `GET /user-requests`, `POST /user-requests`, `DELETE /user-requests/:id`
- `GET /flexibility`

---

## BDD Test Impact Summary

| Feature File | Required Change |
|---|---|
| Any file using `GET /trace` | Rewrite to `GET /trace/events` or `GET /trace/history` |
| Any file using `GET /rates` | Rename to `GET /tariffs` |
| Any file POSTing `ev_force_kw`, `heater_force_kw`, `battery_force_kw` | Delete those scenarios |
| Any file testing FSM states | Delete those scenarios |
| Any file testing reactor arbitration | Delete those scenarios |
