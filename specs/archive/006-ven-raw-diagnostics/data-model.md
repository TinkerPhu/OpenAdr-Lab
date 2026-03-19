# Data Model: VEN Raw Data Diagnostics Page

**Feature**: 006-ven-raw-diagnostics
**Date**: 2026-03-18

---

## Upstream API Shapes (consumed, not modified)

### GET /sim → SimSnapshot

```typescript
interface SimSnapshot {
  ts: string;           // ISO-8601 UTC timestamp of snapshot
  net_power_w: number;  // Grid net power (W); positive = import, negative = export
  import_w: number;     // Grid import power (W)
  export_w: number;     // Grid export power (W)
  voltage_v: number;    // Grid voltage (V)
  import_kwh: number;   // Cumulative energy import (kWh)
  export_kwh: number;   // Cumulative energy export (kWh)
  base_load_w: number;  // Base load power (W)

  // Generic per-asset map (new model)
  assets: Record<string, AssetSnapshot>;

  // Legacy named fields (backward compat)
  ev?: EvSnapshot;
  heater?: HeaterSnapshot;
  pv?: PvSnapshot;
  battery?: BatterySnapshot;
}

interface AssetSnapshot {
  power_kw: number;
  [key: string]: number;  // Additional asset-specific state fields (flattened)
}
```

**Chart mapping**:
- X-axis: asset ID (categorical — each asset + "grid" is one x-tick)
- Y-axis: `power_kw` for assets; `net_power_w / 1000` for grid
- One connected line series across all assets

---

### GET /rates (= /tariffs) → TariffSnapshot[]

```typescript
interface TariffSnapshot {
  interval_start: string;           // ISO-8601 UTC — x-axis
  interval_end: string;
  import_price_eur_kwh: number | null;
  export_price_eur_kwh: number | null;
  co2_g_kwh: number | null;
  source_event_id: string | null;
  is_forecast: boolean;
}
```

**Chart mapping**:
- X-axis: `interval_start` (epoch ms)
- Series 1 (green): `import_price_eur_kwh`
- Series 2 (red): `export_price_eur_kwh`
- Series 3 (grey): `co2_g_kwh`
- `null` values appear as gaps in the line

---

### GET /timeline/all → Record\<string, AssetTimelinePoint[]\>

```typescript
type TimelineResponse = Record<string, AssetTimelinePoint[]>;

interface AssetTimelinePoint {
  ts: string;   // ISO-8601 UTC — x-axis
  values: {
    power_kw: number;
    cost_rate_eur_h?: number;
    co2_rate_g_h?: number;
    // Grid-only extras:
    import_price_eur_kwh?: number;
    export_price_eur_kwh?: number;
    import_limit_kw?: number;
    export_limit_kw?: number;
  };
}

// Available series keys (derived from response at fetch time):
// "ev", "battery", "heater", "pv", "base_load", "grid"
```

**Query parameters**: `?hours_back=1.0&hours_forward=1.0`

**Chart mapping**:
- Selected series: one key from the response (chosen via dropdown)
- X-axis: `ts` (epoch ms)
- Series (teal): `values.power_kw` for selected key
- No downsampling in the chart — all points rendered

---

## UI-Only State (not persisted)

### DiagnosticCell local state

```typescript
interface CellState<T> {
  data: T | null;
  isLoading: boolean;
  isError: boolean;
  errorMessage: string | null;
}
```

### Timeline dropdown selection

```typescript
interface TimelineCellState {
  availableSeries: string[];    // Populated after first fetch
  selectedSeries: string;       // Default: "grid"
}
```

No state is shared between cells. Each cell manages its own fetch state independently.
