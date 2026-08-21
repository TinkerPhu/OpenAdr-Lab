import { CELL_CHART_HEIGHT } from "./chartLayout";
import {
  ComposedChart,
  Area,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from "recharts";
import type { TooltipProps } from "recharts";
import type { AssetId, StackedAreaPoint } from "../controller/types";
import { ASSET_LABELS, ASSET_PLANNING_ROLE, COLOR_ASSET_FALLBACK, SERIES_COLORS } from "../controller/types";
import type { ZoneDef } from "../../api/types";
import { minSpanDomain, MIN_POWER_SPAN_KW, formatPowerTick, roundedTimeTicks, zeroAnchoredTicks } from "./axisDomain";
import { formatSignedPowerValue } from "./unitFormat";
import { renderNowLine } from "./NowLine";
import { renderZoneShading } from "./ZoneShading";
import { TOOLTIP_BOX_STYLE } from "./tooltipStyle";
import { useLegendToggle } from "./useLegendToggle";
import { ChartLegend, type ChartLegendEntry } from "./ChartLegend";

const COLOR_GRID_LINE = SERIES_COLORS.grid_line;
const GRID_LEGEND_KEY = "grid";

interface StackedTimeSeriesChartProps {
  data: StackedAreaPoint[];
  assetIds: AssetId[];
  colorMap: Record<string, string>;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  height?: number;
  zones?: ZoneDef[];
  /** X-axis ticks every N minutes, snapped to the wall-clock (10:00, 10:10, ...) instead of
   * recharts' default "nice" ticks. */
  xAxisTickIntervalMinutes?: number;
  /** Opt-in: adds a checkbox to each legend entry, live-toggling that asset's (both
   * positive and negative series together) or the grid line's visibility. Unset/false:
   * legend behaves as a plain (still one-entry-per-asset) legend, no checkboxes. */
  interactiveLegend?: boolean;
}

function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function assetLabel(id: string): string {
  const label = ASSET_LABELS[id] ?? id;
  const role = ASSET_PLANNING_ROLE[id] ?? "planned";
  return `${label} (${role})`;
}

/** Merges _pos/_neg series back into a single net kW row per asset. */
export function StackedAreaTooltip({
  active,
  payload,
  label,
  colorMap,
}: TooltipProps<number, string> & { colorMap: Record<string, string> }) {
  if (!active || !payload || payload.length === 0) return null;

  // Aggregate net kW per asset; collect grid separately.
  const netByAsset: Record<string, number> = {};
  let gridKw: number | null = null;
  for (const entry of payload) {
    const name = entry.name ?? "";
    if (name === "Grid [kW]") {
      gridKw = (entry.value as number) ?? null;
      continue;
    }
    // name is either "${assetId} +" or "${assetId} -"
    const assetId = name.replace(/ [+-]$/, "");
    netByAsset[assetId] = (netByAsset[assetId] ?? 0) + ((entry.value as number) ?? 0);
  }

  const time = typeof label === "number" ? new Date(label).toLocaleTimeString() : label;

  return (
    <div style={TOOLTIP_BOX_STYLE}>
      <div style={{ marginBottom: 1, fontWeight: "bold" }}>{time}</div>
      {Object.entries(netByAsset).map(([assetId, kw]) => (
        <div key={assetId} style={{ color: colorMap[assetId] ?? COLOR_ASSET_FALLBACK }}>
          {assetLabel(assetId)}: {formatSignedPowerValue(kw)}
        </div>
      ))}
      {gridKw !== null && (
        <div style={{ color: COLOR_GRID_LINE, borderTop: "1px solid #eee", marginTop: 2, paddingTop: 2 }}>
          Grid: {formatSignedPowerValue(gridKw)}
        </div>
      )}
    </div>
  );
}

/** Creates a zero-valued point for all assets (including dynamic shiftable loads). */
const emptyPt = (assetIds: AssetId[]): Omit<StackedAreaPoint, "ts"> => {
  const pt: Record<string, number | null> = { gridPowerKw: null };
  for (const id of assetIds) {
    pt[`${id}_pos`] = 0;
    pt[`${id}_neg`] = 0;
  }
  return pt as Omit<StackedAreaPoint, "ts">;
};

/**
 * Stacked-areas composition — kept as its own component rather than folded into
 * `TimeSeriesChart` (see openspec/changes/unified-chart-primitives/design.md Decision 1):
 * the pos/neg stacking and net-value tooltip re-aggregation (`StackedAreaTooltip`) are
 * genuinely different logic from a plain multi-line chart, not duplication of it. Built on
 * the same shared axis/tick/color/sizing/NOW-line/zone-shading primitives as
 * `TimeSeriesChart`.
 */
export function StackedTimeSeriesChart({
  data,
  assetIds,
  colorMap,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  height,
  zones,
  xAxisTickIntervalMinutes,
  interactiveLegend = false,
}: StackedTimeSeriesChartProps) {
  const { isHidden, toggle } = useLegendToggle();

  // Domain driven by hoursBack/hoursForward keeps the X-axis stable across refreshes
  // and ensures the NOW reference line is always within the visible range.
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;
  const xAxisTicks = xAxisTickIntervalMinutes
    ? roundedTimeTicks(tMin, tMax, xAxisTickIntervalMinutes)
    : undefined;

  // Ensure at least two boundary points so recharts can render the X scale and
  // the NOW reference line even when there are no data points yet.
  const chartData: StackedAreaPoint[] =
    data.length > 0
      ? data
      : [
          { ts: tMin, ...emptyPt(assetIds) } as StackedAreaPoint,
          { ts: tMax, ...emptyPt(assetIds) } as StackedAreaPoint,
        ];

  // pv first so it sits closest to the X axis in both stacks — PV is generation
  // (negative/export side), so this puts it at the base of the export stack with
  // every consuming asset drawn on top of it.
  const renderOrder: AssetId[] = [
    ...assetIds.filter((id) => id === "pv"),
    ...assetIds.filter((id) => id !== "pv"),
  ];

  // Domain floor uses the summed stack top/bottom per point (not individual series), since
  // that is what the chart actually renders — see MIN_POWER_SPAN_KW in axisDomain.ts.
  const powerDomain = minSpanDomain(
    chartData.flatMap((pt) => {
      const posSum = assetIds.reduce((sum, id) => sum + (pt[`${id}_pos`] ?? 0), 0);
      const negSum = assetIds.reduce((sum, id) => sum + (pt[`${id}_neg`] ?? 0), 0);
      return [posSum, negSum, pt.gridPowerKw];
    }),
    MIN_POWER_SPAN_KW
  );

  // One record per asset drives its positive Area, negative Area, and legend entry —
  // a single shared derivation instead of three independent ones, so an asset's label/
  // color can never drift between what's drawn and what the legend shows (see
  // chart_diagrams.md's StackedTimeSeriesChart section). The grid line stays a separate,
  // hardcoded 4th legend entry — it isn't a member of the per-asset family.
  const assetSeries = renderOrder.map((id) => ({
    id,
    label: assetLabel(id),
    color: colorMap[id] ?? COLOR_ASSET_FALLBACK,
  }));
  const legendEntries: ChartLegendEntry[] = [
    ...assetSeries.map(({ id, label, color }) => ({ key: id, label, color })),
    { key: GRID_LEGEND_KEY, label: "Grid", color: COLOR_GRID_LINE },
  ];

  return (
    <div data-testid="accumulated-area-chart" style={{ width: "100%", height: height ?? CELL_CHART_HEIGHT }}>
      <ResponsiveContainer width="100%" height="100%">
        {/* margin.right=92 provides alignment space matching the two right axes
            (44+44 px) in AssetTimelineChart — no right axis here so the grid
            line shares the same kW scale as the stacked areas. */}
        <ComposedChart data={chartData} margin={{ top: 4, right: 92, left: 0, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
          <XAxis
            dataKey="ts"
            scale="time"
            type="number"
            domain={[tMin, tMax]}
            allowDataOverflow
            ticks={xAxisTicks}
            tickFormatter={formatTs}
            tick={{ fontSize: 10 }}
          />
          <YAxis
            yAxisId="power"
            tick={{ fontSize: 10 }}
            width={46}
            tickFormatter={formatPowerTick}
            domain={powerDomain}
            ticks={zeroAnchoredTicks(powerDomain)}
          />
          <Tooltip content={<StackedAreaTooltip colorMap={colorMap} />} />
          <Legend
            content={
              <ChartLegend
                entries={legendEntries}
                isHidden={isHidden}
                toggle={toggle}
                interactive={interactiveLegend}
              />
            }
          />

          {/* For each asset: positive series (import, stacked above x-axis) */}
          {assetSeries.map(({ id, color }) => (
            <Area
              key={`${id}_pos`}
              yAxisId="power"
              type="stepAfter"
              dataKey={`${id}_pos`}
              name={`${id} +`}
              stackId="positive"
              fill={color}
              stroke="none"
              fillOpacity={0.6}
              dot={false}
              connectNulls={false}
              isAnimationActive={false}
              hide={interactiveLegend && isHidden(id)}
            />
          ))}

          {/* For each asset: negative series (export/generation, stacked below x-axis) */}
          {assetSeries.map(({ id, color }) => (
            <Area
              key={`${id}_neg`}
              yAxisId="power"
              type="stepAfter"
              dataKey={`${id}_neg`}
              name={`${id} -`}
              stackId="negative"
              fill={color}
              stroke="none"
              fillOpacity={0.6}
              dot={false}
              connectNulls={false}
              isAnimationActive={false}
              hide={interactiveLegend && isHidden(id)}
            />
          ))}

          {/* Grid power — shares the left kW axis so scale matches the stacked areas */}
          <Line
            yAxisId="power"
            type="stepAfter"
            dataKey="gridPowerKw"
            name="Grid [kW]"
            stroke={COLOR_GRID_LINE}
            strokeWidth={2}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
            hide={interactiveLegend && isHidden(GRID_LEGEND_KEY)}
          />

          {/* Zone background shading — rendered before data lines so they sit behind */}
          {renderZoneShading("power", zones)}

          {/* NOW reference line */}
          {renderNowLine("power", nowMs)}
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}
