# Backend API Contract: VEN Timeline UI

**Branch**: `005-ven-timeline-ui`
**Date**: 2026-03-16

All endpoints are relative to the VEN HTTP base URL (e.g., `http://ven-ven-1-1:8080`).

---

## New Endpoints

### `GET /timeline/{asset_id}`

Returns a merged past+future timeline for a single asset.

**Path parameters**:
- `asset_id` — any key in `sim.assets` plus `"grid"`. Returns 404 if unrecognised.

**Query parameters**:
- `hours_back: f64` (default `1.0`) — how many hours of history to include
- `hours_forward: f64` (default `1.0`) — how many hours of plan future to include

**Response** `200 OK`:
```json
[
  {
    "ts": "2026-03-16T10:00:00Z",
    "values": {
      "power_kw": 2.3,
      "cost_rate_eur_h": 0.46,
      "co2_rate_g_h": 690.0,
      "soc_pct": 42.5
    }
  },
  ...
]
```

- Array sorted ascending by `ts`
- Past points (`ts < now`): from `AssetHistoryBuffer`; values may be sparse (NaN omitted in JSON)
- Future points (`ts >= now`): projected from `Plan` slots; only `power_kw`, `cost_rate_eur_h`, `co2_rate_g_h` populated
- Empty array `[]` is a valid response when no data exists in the requested window

**Response** `404 Not Found`:
```json
{ "error": "unknown asset: {asset_id}" }
```

---

### `GET /timeline/all`

Returns timelines for all configured assets plus grid in a single request.

**Query parameters**: same as `GET /timeline/{asset_id}` (`hours_back`, `hours_forward`).

**Response** `200 OK`:
```json
{
  "ev": [ { "ts": "...", "values": { "power_kw": 2.3, ... } }, ... ],
  "battery": [ ... ],
  "heater": [ ... ],
  "pv": [ ... ],
  "base_load": [ ... ],
  "grid": [ ... ]
}
```

- Object keys match `sim.assets` keys plus `"grid"`
- Each value is sorted `Vec<AssetTimelinePoint>`
- All assets share the same query window

---

### `GET /timeline/grid`

Alias — handled by `GET /timeline/{asset_id}` with `asset_id = "grid"`.

**Grid-specific values**:
- Past: from `AssetHistoryBuffer["grid"]` (net site power stored per-tick by `monitor.rs`)
- Future: from `PlanTimeSlot.net_import_kw` / `net_export_kw`; grid-only keys:

| Key | Description |
|-----|-------------|
| `power_kw` | net site import power (positive = import, negative = export) |
| `cost_rate_eur_h` | instantaneous cost rate |
| `co2_rate_g_h` | instantaneous CO₂ rate |
| `import_price_eur_kwh` | tariff import price (future only, from `TariffSnapshot`) |
| `export_price_eur_kwh` | tariff export price (future only) |
| `import_limit_kw` | grid import capacity limit (from `CapacityState`, future only) |
| `export_limit_kw` | grid export capacity limit (future only) |

---

## Existing Endpoints (unchanged)

### `GET /sim/schema`

Already implemented. Returns dynamic control descriptors per asset.

**Response** `200 OK`:
```json
{
  "ev": [
    { "key": "ev_plugged", "kind": "Switch", "label": "Plugged in" },
    { "key": "ev_soc", "kind": "Slider", "label": "SoC (%)", "min": 0.0, "max": 100.0, "step": 1.0 },
    { "key": "ev_target_soc", "kind": "Slider", "label": "Target SoC (%)", "min": 0.0, "max": 100.0, "step": 1.0 }
  ],
  "battery": [ ... ],
  "heater": [ ... ],
  "pv": [ ... ],
  "base_load": [ ... ]
}
```

First UI consumer wired up in this speckit.

---

## Routing Registration (main.rs additions)

```rust
.route("/timeline/:asset_id", get(get_timeline))
.route("/timeline/all",       get(get_timeline_all))
```

Note: `GET /timeline/all` must be registered **before** `GET /timeline/:asset_id` so the literal `"all"` path is not captured as an `:asset_id` parameter.
