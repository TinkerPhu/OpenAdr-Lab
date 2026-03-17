# Data Model: VEN Timeline UI

**Branch**: `005-ven-timeline-ui`
**Date**: 2026-03-16

## Existing Entities (unchanged)

### AssetTimelinePoint *(Rust — controller/trace.rs — already exists)*

Single time-stamped data row from the history buffer or a projected plan slot.

```rust
pub struct AssetTimelinePoint {
    pub ts: DateTime<Utc>,
    pub values: HashMap<String, f64>,   // sparse; NaN = not recorded
}
```

Well-known value keys:

| Key | Applies to | Source |
|-----|-----------|--------|
| `power_kw` | all assets, grid | history buffer / plan allocation |
| `cost_rate_eur_h` | all assets, grid | history buffer / computed from plan |
| `co2_rate_g_h` | all assets, grid | history buffer / computed from plan |
| `soc_pct` | EV, Battery | history buffer |
| `temp_c` | Heater | history buffer |
| `irradiance` | PV | history buffer |
| `import_price_eur_kwh` | grid | tariff snapshot |
| `export_price_eur_kwh` | grid | tariff snapshot |
| `import_limit_kw` | grid | capacity state |
| `export_limit_kw` | grid | capacity state |

No `is_forecast` field — past vs. future is implicit in `ts` relative to `now` at query time.

---

### TimeWindow *(new — query parameters only, no persistent struct)*

```rust
pub struct TimeWindow {
    pub hours_back: f64,     // default 1.0
    pub hours_forward: f64,  // default 1.0
}
```

Used as query parameters: `?hours_back=1&hours_forward=3`. Not stored — derived from the HTTP request.

---

### AssetTimelinePoint *(TypeScript — replaces AssetTimePoint)*

Frontend mirror of the Rust struct. Replaces the existing `AssetTimePoint` type in `types.ts`.

```typescript
export type AssetTimelinePoint = {
  /** Epoch ms — X-axis value */
  ts: number;
  /** Sparse value map — access by well-known key */
  values: Record<string, number>;
};
```

**Replaces**: `AssetTimePoint` (which had flat named fields + `isPast: boolean`).

---

### ControlDescriptor *(Rust — simulator/assets/mod.rs — already exists)*

Describes one simulation override control for a given asset.

```rust
pub struct ControlDescriptor {
    pub key: String,         // override field name (e.g. "ev_plugged", "ev_soc")
    pub kind: ControlKind,   // Slider | Switch | NumberInput
    pub label: String,       // display label
    pub min: Option<f64>,    // for Slider/NumberInput
    pub max: Option<f64>,    // for Slider/NumberInput
    pub step: Option<f64>,   // for Slider
}

pub enum ControlKind {
    Slider,
    Switch,
    NumberInput,
}
```

Already serialised and returned by `GET /sim/schema`.

---

### TariffSnapshot *(Rust — entities/tariff_snapshot.rs — already exists)*

```rust
pub struct TariffSnapshot {
    pub interval_start: DateTime<Utc>,
    pub interval_end: DateTime<Utc>,
    pub import_price_eur_kwh: Option<f64>,
    pub export_price_eur_kwh: Option<f64>,
    pub co2_g_kwh: Option<f64>,
    pub source_event_id: Option<String>,
    pub is_forecast: bool,
}
```

Used as input to `build_asset_timeline` for enriching future grid timeline points with tariff intervals (when `AssetHistoryBuffer["grid"]` history is absent for the future window).

---

### TariffSnapshot *(TypeScript — replaces RateSnapshot)*

```typescript
export type TariffSnapshot = {
  interval_start: string;      // ISO 8601
  interval_end: string;
  import_price_eur_kwh: number | null;
  export_price_eur_kwh: number | null;
  co2_g_kwh: number | null;
  source_event_id: string | null;
  is_forecast: boolean;
};
```

**Replaces**: `RateSnapshot` (identical fields, renamed for consistency with backend).

---

## Entity Relationships

```text
PlanTimeSlot (Plan.firm_slots | flexible_slots)
  └─ allocations: Vec<PacketAllocation>
       └─ asset_id, power_kw, cost_eur, co2_g
  └─ net_import_kw, net_export_kw        ← for grid timeline
  └─ import_price_eur_kwh, co2_g_kwh    ← tariff for cost_rate_eur_h computation

AssetHistoryBuffer (per asset_id in controller_trace)
  └─ to_timeline(window) -> Vec<AssetTimelinePoint>
       └─ columns: power_kw, cost_rate_eur_h, co2_rate_g_h, soc_pct, ...

build_asset_timeline(asset_id, history, plan, tariffs, window)
  ├─ past: history.to_timeline(Some(window_past))
  └─ future: plan slots → project Vec<AssetTimelinePoint>
  └─ merge + sort by ts → Vec<AssetTimelinePoint>
```

---

## Removed Types (this speckit)

| Type | File | Replacement |
|------|------|-------------|
| `AssetTimePoint` | `VEN/ui/src/components/controller-v2/types.ts` | `AssetTimelinePoint` |
| `RateSnapshot` | `VEN/ui/src/api/hooks.ts` + `types.ts` | `TariffSnapshot` |
| `PlannedRates` | `VEN/ui/src/api/hooks.ts` | `TariffSnapshot[]` |
