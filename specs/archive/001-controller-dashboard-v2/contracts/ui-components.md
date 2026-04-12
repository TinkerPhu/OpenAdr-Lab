# UI Component Contracts: VEN Controller Dashboard V2

> **Nomenclature**: **Tariff** = X/kWh (unit price). **Rate** = X/h (instantaneous flow).
> The API's `GET /rates` endpoint and `RateSnapshot` type return tariff (per-kWh) data. Parameter names in this document use `tariffs` to reflect the correct term; the API type name `RateSnapshot` is kept as-is until the planned API rename.

**Phase**: 1 — Design
**Date**: 2026-03-14
**Branch**: `001-controller-dashboard-v2`

---

## Route Contract

| Property | Value |
|---|---|
| Route path | `/controller` |
| Nav label | `Controller V2` |
| Position in AppBar | After existing `Controller` tab |
| Page component | `ControllerV2` (named function export from `pages/ControllerV2.tsx`) |

---

## Page Component: ControllerV2

```typescript
// pages/ControllerV2.tsx
export function ControllerV2(): JSX.Element
```

**Responsibilities**:
- Owns `pinnedCellIds: string[]` and `collapseState: CollapseState` React state
- Calls all data hooks (useSim, useTrace, usePlan, useRates, useUserRequests)
- Renders `PinnedZone` + scrollable cell list
- Derives `AssetSummary[]` and `AssetTimePoint[][]` from raw API data via `dataBuilders`

**data-testid values**:
- `data-testid="controller-page"` — root container
- `data-testid="pinned-zone"` — pinned cell area
- `data-testid="scrollable-content"` — scrollable area below pinned zone

---

## Component: PinnedZone

```typescript
// components/controller/PinnedZone.tsx
interface PinnedZoneProps {
  pinnedCellIds: string[];
  children: React.ReactNode;   // Rendered pinned cells (caller provides)
}
export function PinnedZone({ pinnedCellIds, children }: PinnedZoneProps): JSX.Element
```

**Behavior**: Fixed to top of viewport when `pinnedCellIds.length > 0`. Rendered above scrollable content.

**data-testid**: `data-testid="pinned-zone"`

---

## Component: AssetCell

```typescript
// components/controller/AssetCell.tsx
interface AssetCellProps {
  assetId: AssetId;
  summary: AssetSummary;
  timePoints: AssetTimePoint[];
  simOverrides: UserOverrides | undefined;
  collapsed: { left: boolean; right: boolean };
  pinned: boolean;
  onTogglePin: (assetId: string) => void;
  onToggleCollapse: (assetId: string, section: "left" | "right") => void;
  onOverrideChange: (patch: Partial<UserOverrides>) => void;
}
export function AssetCell(props: AssetCellProps): JSX.Element
```

**Layout**: Three horizontal sections (left | mid | right). Left and right sections collapse independently.

**data-testid values**:
- `data-testid="asset-cell-{assetId}"` — root cell wrapper
- `data-testid="asset-cell-{assetId}-pin-btn"` — pin toggle button
- `data-testid="asset-cell-{assetId}-left"` — left section
- `data-testid="asset-cell-{assetId}-mid"` — mid section (graph)
- `data-testid="asset-cell-{assetId}-right"` — right section
- `data-testid="asset-cell-{assetId}-collapse-left"` — collapse left toggle
- `data-testid="asset-cell-{assetId}-collapse-right"` — collapse right toggle

---

## Component: AssetLeftSection

```typescript
interface AssetLeftSectionProps {
  summary: AssetSummary;
}
export function AssetLeftSection({ summary }: AssetLeftSectionProps): JSX.Element
```

**Displays**: power [kW], cost rate [€/h], CO₂eq rate [g CO₂eq/h], SoC [%] (if applicable), forecast energy [kWh] (if available), active user request (if any).

**data-testid values**:
- `data-testid="asset-power-{assetId}"` — power value display
- `data-testid="asset-cost-rate-{assetId}"` — cost rate display
- `data-testid="asset-co2-rate-{assetId}"` — CO₂eq rate display
- `data-testid="asset-soc-{assetId}"` — SoC display (if applicable)
- `data-testid="asset-forecast-energy-{assetId}"` — forecast energy (if shown)
- `data-testid="asset-request-{assetId}"` — user request summary (if shown)

---

## Component: AssetMidSection

```typescript
interface AssetMidSectionProps {
  assetId: AssetId;
  timePoints: AssetTimePoint[];
  color: string;
}
export function AssetMidSection({ assetId, timePoints, color }: AssetMidSectionProps): JSX.Element
```

**Contains**: `AssetTimelineChart` wrapped in `ResponsiveContainer`.

**data-testid**: `data-testid="asset-timeline-chart-{assetId}"`

---

## Component: AssetRightSection

```typescript
interface AssetRightSectionProps {
  assetId: AssetId;
  simSnapshot: SimSnapshot | undefined;
  overrides: UserOverrides | undefined;
  onOverrideChange: (patch: Partial<UserOverrides>) => void;
}
export function AssetRightSection(props: AssetRightSectionProps): JSX.Element
```

**Contains**: Two MUI Accordion groups — Status Settings and Simulation Characteristics. Asset-type-specific controls rendered per assetId.

**data-testid values**:
- `data-testid="right-section-{assetId}"` — section wrapper
- `data-testid="status-settings-accordion-{assetId}"` — Status Settings accordion
- `data-testid="sim-characteristics-accordion-{assetId}"` — Simulation Characteristics accordion
- `data-testid="ctrl-ev-plugged"` — EV plugged toggle (Switch)
- `data-testid="ctrl-ev-soc"` — EV SoC slider
- `data-testid="ctrl-battery-soc"` — Battery SoC slider
- `data-testid="ctrl-{assetId}-power-toggle"` — power on/off (where applicable)
- `data-testid="ctrl-{assetId}-{fieldName}"` — generic pattern for other controls

---

## Component: GridTariffCell

```typescript
interface GridTariffCellProps {
  snapshot: TariffSnapshot;
  timePoints: TariffTimePoint[];
  pinned: boolean;
  onTogglePin: () => void;
}
export function GridTariffCell(props: GridTariffCellProps): JSX.Element
```

**Layout**: Two sections (left: values, right: graph). No controls (read-only, FR-030).

**data-testid values**:
- `data-testid="grid-tariff-cell"` — root
- `data-testid="grid-tariff-cell-pin-btn"` — pin toggle
- `data-testid="tariff-import-price"` — import tariff value
- `data-testid="tariff-export-price"` — export tariff value
- `data-testid="tariff-co2"` — CO₂eq intensity value
- `data-testid="tariff-total-cost-rate"` — total cost rate
- `data-testid="tariff-grid-power"` — grid power value
- `data-testid="tariff-chart"` — right section chart

---

## Component: GridAccumulatedCell

```typescript
interface GridAccumulatedCellProps {
  assetSummaries: AssetSummary[];
  stackedAreaPoints: StackedAreaPoint[];
  pinned: boolean;
  onTogglePin: () => void;
}
export function GridAccumulatedCell(props: GridAccumulatedCellProps): JSX.Element
```

**Layout**: Two sections (left: current power list per asset, right: stacked area chart). No controls (FR-035).

**data-testid values**:
- `data-testid="grid-accumulated-cell"` — root
- `data-testid="grid-accumulated-cell-pin-btn"` — pin toggle
- `data-testid="accumulated-power-{assetId}"` — per-asset current power entry in left section
- `data-testid="accumulated-area-chart"` — stacked area chart

---

## Pure Functions: dataBuilders.ts

```typescript
// Build AssetTimePoint[] for one asset from trace entries + plan
function buildAssetTimeline(
  assetId: AssetId,
  traceEntries: TraceEntry[],    // chronological order
  plan: Plan | null,
  tariffs: RateSnapshot[],   // from GET /rates — per-kWh tariff data despite endpoint name
  nowMs: number
): AssetTimePoint[]

// Build StackedAreaPoint[] from trace + plan for all assets
function buildStackedAreaData(
  traceEntries: TraceEntry[],
  plan: Plan | null,
  nowMs: number
): StackedAreaPoint[]

// Build TariffTimePoint[] from rates + trace (grid power)
function buildTariffTimeline(
  tariffs: RateSnapshot[],   // from GET /rates — per-kWh tariff data despite endpoint name
  traceEntries: TraceEntry[],
  plan: Plan | null,
  nowMs: number
): TariffTimePoint[]

// Find the tariff interval covering a given timestamp
// Note: RateSnapshot is the API type name; the data it holds is tariff data (per-kWh)
function findCurrentTariff(tariffs: RateSnapshot[], tsMs: number): RateSnapshot | null

// Derive AssetSummary from sim + rates + user-requests
function deriveAssetSummaries(
  sim: SimSnapshot,
  tariffs: RateSnapshot[],   // from GET /rates — per-kWh tariff data despite endpoint name
  userRequests: UserRequest[],
  plan: Plan | null,
  nowMs: number
): AssetSummary[]
```

All functions are pure (no side effects) and testable in isolation via Vitest.

---

## Chart Components

### AssetTimelineChart

```typescript
interface AssetTimelineChartProps {
  data: AssetTimePoint[];
  color: string;        // Asset color (all 3 lines share this color)
  nowMs: number;
}
export function AssetTimelineChart(props: AssetTimelineChartProps): JSX.Element
```

**recharts structure**:
- `ComposedChart` with `CartesianGrid`, `XAxis` (time), `YAxis` (left: power kW, right: rate €/h or g/h)
- Power line: `Line` with solid stroke, `dataKey="powerKw"`
- Cost rate line: `Line` with dashed stroke (`strokeDasharray="5 5"`), `dataKey="costRateEurH"`
- CO₂eq rate line: `Line` with dotted stroke (`strokeDasharray="2 2"`), `dataKey="co2RateGH"`
- `ReferenceLine` at `x={nowMs}` with red dotted stroke (the "NOW" marker)
- `Legend` showing line types and units
- `connectNulls={false}` — gaps for missing data

### TariffChart

```typescript
interface TariffChartProps {
  data: TariffTimePoint[];
  nowMs: number;
}
export function TariffChart(props: TariffChartProps): JSX.Element
```

**recharts structure**: `ComposedChart` with 5 series (import tariff red dashed, import CO₂ red dotted, export tariff green dashed, total cost rate black dashed, grid power black solid). `ReferenceLine` at nowMs.

### StackedAreaChart

```typescript
interface StackedAreaChartProps {
  data: StackedAreaPoint[];
  assetIds: AssetId[];   // Determines which series to render
  colorMap: Record<AssetId, string>;
  nowMs: number;
}
export function StackedAreaChart(props: StackedAreaChartProps): JSX.Element
```

**recharts structure**: `AreaChart` with split positive/negative series per asset. `_pos` series use `stackId="positive"`, `_neg` series use `stackId="negative"`. `ReferenceLine` at nowMs.

---

## UserOverrides Patch Pattern

The right section controls use a **read-current-merge-write** pattern to avoid overwriting unrelated fields (POST /sim/override is full-replace):

```typescript
// In ControllerV2 (or AssetRightSection):
const { data: currentOverrides } = useSimOverride();
const { mutate: setOverride } = useSetSimOverride();

function handleOverrideChange(patch: Partial<UserOverrides>) {
  setOverride({ ...currentOverrides, ...patch });
}
```

This ensures every POST includes all current override values plus the changed field.
