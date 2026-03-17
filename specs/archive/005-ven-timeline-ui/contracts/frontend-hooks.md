# Frontend Hooks Contract: VEN Timeline UI

**Branch**: `005-ven-timeline-ui`
**Date**: 2026-03-16

All hooks live in `VEN/ui/src/api/hooks.ts`. They use TanStack React Query.

---

## New Hooks

### `useTimeline(assetId, hoursBack, hoursForward)`

Per-asset timeline for independent cell time windows.

```typescript
function useTimeline(
  assetId: string,
  hoursBack: number,
  hoursForward: number
): UseQueryResult<AssetTimelinePoint[]>
```

- Calls `GET /timeline/{assetId}?hours_back={hoursBack}&hours_forward={hoursForward}`
- `staleTime`: 10 000 ms (10s)
- Returns `AssetTimelinePoint[]` (empty array on loading)
- Query key: `["timeline", assetId, hoursBack, hoursForward]`

---

### `useAllTimelines(hoursBack, hoursForward)`

All-asset timelines for stacked area chart and page-level prefetch.

```typescript
function useAllTimelines(
  hoursBack: number,
  hoursForward: number
): UseQueryResult<Record<string, AssetTimelinePoint[]>>
```

- Calls `GET /timeline/all?hours_back={hoursBack}&hours_forward={hoursForward}`
- `staleTime`: 10 000 ms (10s)
- Returns `Record<string, AssetTimelinePoint[]>` (empty object on loading)
- Query key: `["timeline", "all", hoursBack, hoursForward]`

---

### `useSimSchema()`

Control schema for dynamic sim override rendering. Fetched once per session.

```typescript
function useSimSchema(): UseQueryResult<Record<string, ControlDescriptor[]>>
```

- Calls `GET /sim/schema`
- `staleTime`: `Infinity` (static per session — schema does not change at runtime)
- Returns `Record<string, ControlDescriptor[]>` keyed by asset ID
- Query key: `["sim", "schema"]`

---

## Renamed Hook

### `useTariffs()` *(renamed from `useRates`)*

```typescript
function useTariffs(): UseQueryResult<TariffSnapshot[]>
```

- Calls `GET /tariffs` (unchanged internal endpoint)
- All consumers updated from `useRates` → `useTariffs`
- Return type: `TariffSnapshot[]` (renamed from `RateSnapshot[]`)

---

## Removed Hooks / Types

| Item | File | Reason |
|------|------|--------|
| `useRates()` | `hooks.ts` | Renamed to `useTariffs()` |
| `RateSnapshot` type | `hooks.ts` | Renamed to `TariffSnapshot` |
| `PlannedRates` type | `hooks.ts` | Replaced by `TariffSnapshot[]` |

---

## TypeScript Types (additions to `types.ts`)

```typescript
// Replaces AssetTimePoint
export type AssetTimelinePoint = {
  ts: number;                         // epoch ms
  values: Record<string, number>;     // sparse; use ?? 0 when accessing
};

// Replaces RateSnapshot
export type TariffSnapshot = {
  interval_start: string;
  interval_end: string;
  import_price_eur_kwh: number | null;
  export_price_eur_kwh: number | null;
  co2_g_kwh: number | null;
  source_event_id: string | null;
  is_forecast: boolean;
};

// Already exists in assets mod — mirrored to TS
export type ControlKind = "Slider" | "Switch" | "NumberInput";

export type ControlDescriptor = {
  key: string;
  kind: ControlKind;
  label: string;
  min?: number;
  max?: number;
  step?: number;
};
```

---

## UI Component Contracts

### `AssetTimelineChart` (updated)

```typescript
interface AssetTimelineChartProps {
  data: AssetTimelinePoint[];   // replaces AssetTimePoint[]
  color: string;
  nowMs: number;
}
```

Renders three series from `values` map:
- `values.power_kw` — solid line, left Y-axis (kW)
- `values.cost_rate_eur_h` — dashed line, right Y-axis (€/h)
- `values.co2_rate_g_h` — dotted line, right Y-axis (g/h)

X-axis domain: `[nowMs - hoursBack*3_600_000, nowMs + hoursForward*3_600_000]` (default ±1h).
NOW reference line at `x={nowMs}` (red dotted).

---

### `DynamicControl`

```typescript
interface DynamicControlProps {
  descriptor: ControlDescriptor;
  value: number | boolean | null;
  onChange: (key: string, val: number | boolean) => void;
}
```

Renders based on `descriptor.kind`:
- `"Switch"` → MUI `Switch`; `value` is boolean; emits `true`/`false`
- `"Slider"` → MUI `Slider` with `min`/`max`/`step`; emits number
- `"NumberInput"` → MUI `TextField type="number"`; emits number

---

### `AssetRightSection` (updated)

```typescript
interface AssetRightSectionProps {
  assetId: string;
  simOverrides: Record<string, number | boolean>;
  onOverrideChange: (partial: Record<string, number | boolean>) => void;
}
```

Renders `controls.map(d => <DynamicControl descriptor={d} ... />)` where `controls = schema[assetId] ?? []`.

Override POST body: the full current `simOverrides` merged with the changed `{ [key]: value }`.
