# UI Component Contracts: VEN Raw Data Diagnostics Page

**Feature**: 006-ven-raw-diagnostics
**Date**: 2026-03-18

---

## Page: RawDiagnostics

**Route**: `/raw-diagnostics`
**Nav label**: "Raw Data"
**File**: `VEN/ui/src/pages/RawDiagnostics.tsx`

**Responsibility**: Render three stacked `DiagnosticCell` instances. No shared state between cells.

**Test IDs**:
- `data-testid="raw-diagnostics-page"` — page root

---

## Component: DiagnosticCell

**File**: `VEN/ui/src/components/raw-diagnostics/DiagnosticCell.tsx`

**Props**:
```typescript
interface DiagnosticCellProps {
  title: string;
  isLoading: boolean;
  isError: boolean;
  onRefresh: () => void;
  children: React.ReactNode;   // Chart or empty state
}
```

**Behavior**:
- Renders a MUI `Card` with a title bar, small `IconButton` (RefreshIcon) top-right, and the chart in the card body.
- While `isLoading`, shows a `CircularProgress` centered in the chart area (charts are hidden).
- While `isError`, shows a red inline error message ("Failed to load data") in the chart area.
- `onRefresh` called on button click; does NOT auto-call on mount.

**Test IDs**:
- `data-testid="diagnostic-cell-{title}"` — card root (title normalized to kebab-case)
- `data-testid="refresh-btn-{title}"` — refresh button
- `data-testid="loading-indicator-{title}"` — loading spinner
- `data-testid="error-msg-{title}"` — error message

---

## Component: SimProfileChart

**File**: `VEN/ui/src/components/raw-diagnostics/SimProfileChart.tsx`

**Props**:
```typescript
interface SimProfileChartProps {
  data: SimSnapshot;
}
```

**Rendered chart**: recharts `LineChart` (categorical x-axis).

**Chart data shape** (derived from props):
```typescript
type Point = { name: string; power_kw: number };
// e.g. [
//   { name: "grid", power_kw: 2.1 },
//   { name: "ev",   power_kw: 3.5 },
//   { name: "battery", power_kw: -1.0 },
//   ...
// ]
```

**Derivation**: Collect `assets` entries as `{ name: assetId, power_kw: snapshot.power_kw }`. Prepend a "grid" point from `net_power_w / 1000`.

**Visual spec**:
- Single `<Line>` series (one color, `CHART_COLORS[0]`).
- X-axis: asset name labels (categorical — `xAxisId` type `"category"`).
- Y-axis: power in kW.
- `connectNulls={false}`.
- `dot={true}` so individual data points are visible.

**Test IDs**:
- `data-testid="sim-profile-chart"` — chart root

---

## Component: TariffsLineChart

**File**: `VEN/ui/src/components/raw-diagnostics/TariffsLineChart.tsx`

**Props**:
```typescript
interface TariffsLineChartProps {
  data: TariffSnapshot[];
}
```

**Rendered chart**: recharts `LineChart` (time-series x-axis).

**Chart data shape** (derived from props):
```typescript
type Point = {
  ts: number;                         // Date.parse(snapshot.interval_start)
  import_price_eur_kwh: number | null;
  export_price_eur_kwh: number | null;
  co2_g_kwh: number | null;
};
```

**Visual spec**:
- Three `<Line>` series, each a distinct color from `CHART_COLORS[0..2]`.
- `connectNulls={false}` — gaps where data is null.
- X-axis: time, Y-axis: price (€/kWh) or CO₂ (g/kWh) — shared axis (different scales are acceptable for raw diagnostic view).
- Empty state "No tariff data" when array is empty.

**Test IDs**:
- `data-testid="tariffs-line-chart"` — chart root

---

## Component: TimelineSeriesChart

**File**: `VEN/ui/src/components/raw-diagnostics/TimelineSeriesChart.tsx`

**Props**:
```typescript
interface TimelineSeriesChartProps {
  data: Record<string, AssetTimelinePoint[]>;
  selectedSeries: string;
  onSeriesChange: (series: string) => void;
}
```

**Rendered chart**: recharts `LineChart` (time-series x-axis).

**Chart data shape** (derived from props):
```typescript
const points = (data[selectedSeries] ?? []).map(p => ({
  ts: Date.parse(p.ts),
  power_kw: p.values.power_kw,
}));
```

**Dropdown**: MUI `Select` listing all keys of `data`. Default = `"grid"` (if present) or first key. Changing selection updates `onSeriesChange`; refresh is NOT triggered automatically on selection change.

**Visual spec**:
- Single `<Line>` series (`CHART_COLORS[3]`, teal).
- `dot={false}` for dense time-series.
- X-axis: time, Y-axis: power_kw.
- Empty state "No data for selected series" when series array is empty.

**Test IDs**:
- `data-testid="timeline-series-chart"` — chart root
- `data-testid="timeline-series-select"` — dropdown

---

## Shared Constant: CHART_COLORS

**File**: `VEN/ui/src/components/raw-diagnostics/colors.ts`

```typescript
export const CHART_COLORS = [
  "#1976d2",  // blue   — index 0
  "#ed6c02",  // orange — index 1
  "#2e7d32",  // green  — index 2
  "#0097a7",  // teal   — index 3
  "#7b1fa2",  // purple — index 4
  "#c62828",  // red    — index 5
];
```

---

## Nav Registration

In `App.tsx`, add to the nav button list:
```tsx
<Button onClick={() => navigate("/raw-diagnostics")}>Raw Data</Button>
```

And add to Routes:
```tsx
<Route path="/raw-diagnostics" element={<RawDiagnosticsPage />} />
```
