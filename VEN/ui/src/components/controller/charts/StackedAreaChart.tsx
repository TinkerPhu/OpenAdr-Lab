import { CELL_CHART_HEIGHT } from "../chartLayout";
import {
  ComposedChart,
  Area,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceLine,
  Legend,
  ResponsiveContainer,
} from "recharts";
import type { TooltipProps } from "recharts";
import type { AssetId, StackedAreaPoint } from "../types";
import { ASSET_LABELS, ASSET_PLANNING_ROLE, COLOR_NOW, COLOR_ASSET_FALLBACK } from "../types";

const COLOR_GRID_LINE = "#212121";

interface StackedAreaChartProps {
  data: StackedAreaPoint[];
  assetIds: AssetId[];
  colorMap: Record<string, string>;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  height?: number;
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
    <div
      style={{
        background: "rgba(255,255,255,0.95)",
        border: "1px solid #ccc",
        borderRadius: 4,
        padding: "1px 5px",
        fontSize: 9,
      }}
    >
      <div style={{ marginBottom: 1, fontWeight: "bold" }}>{time}</div>
      {Object.entries(netByAsset).map(([assetId, kw]) => (
        <div key={assetId} style={{ color: colorMap[assetId] ?? COLOR_ASSET_FALLBACK }}>
          {assetLabel(assetId)}: {kw >= 0 ? "+" : ""}
          {kw.toFixed(2)} kW
        </div>
      ))}
      {gridKw !== null && (
        <div style={{ color: COLOR_GRID_LINE, borderTop: "1px solid #eee", marginTop: 2, paddingTop: 2 }}>
          Grid: {gridKw >= 0 ? "+" : ""}
          {gridKw.toFixed(2)} kW
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

export function StackedAreaChart({
  data,
  assetIds,
  colorMap,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  height,
}: StackedAreaChartProps) {
  // Domain driven by hoursBack/hoursForward keeps the X-axis stable across refreshes
  // and ensures the NOW reference line is always within the visible range.
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;

  // Ensure at least two boundary points so recharts can render the X scale and
  // the NOW reference line even when there are no data points yet.
  const chartData: StackedAreaPoint[] =
    data.length > 0
      ? data
      : [
          { ts: tMin, ...emptyPt(assetIds) } as StackedAreaPoint,
          { ts: tMax, ...emptyPt(assetIds) } as StackedAreaPoint,
        ];

  // base_load first so it sits closest to the X axis in both stacks.
  const renderOrder: AssetId[] = [
    ...assetIds.filter((id) => id === "base_load"),
    ...assetIds.filter((id) => id !== "base_load"),
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
            tickFormatter={formatTs}
            tick={{ fontSize: 10 }}
          />
          <YAxis yAxisId="power" tick={{ fontSize: 10 }} width={40} />
          <Tooltip content={<StackedAreaTooltip colorMap={colorMap} />} />
          <Legend
            iconSize={10}
            wrapperStyle={{ fontSize: 10 }}
            formatter={(value: string) => {
              const id = value.replace(/ [+-]$/, "");
              const suffix = value.endsWith(" +") ? " +" : " -";
              return `${assetLabel(id)}${suffix}`;
            }}
          />

          {/* For each asset: positive series (import, stacked above x-axis) */}
          {renderOrder.map((id) => (
            <Area
              key={`${id}_pos`}
              yAxisId="power"
              type="stepAfter"
              dataKey={`${id}_pos`}
              name={`${id} +`}
              stackId="positive"
              fill={colorMap[id] ?? COLOR_ASSET_FALLBACK}
              stroke={colorMap[id] ?? COLOR_ASSET_FALLBACK}
              fillOpacity={0.6}
              dot={false}
              connectNulls={false}
              isAnimationActive={false}
            />
          ))}

          {/* For each asset: negative series (export/generation, stacked below x-axis) */}
          {renderOrder.map((id) => (
            <Area
              key={`${id}_neg`}
              yAxisId="power"
              type="stepAfter"
              dataKey={`${id}_neg`}
              name={`${id} -`}
              stackId="negative"
              fill={colorMap[id] ?? COLOR_ASSET_FALLBACK}
              stroke={colorMap[id] ?? COLOR_ASSET_FALLBACK}
              fillOpacity={0.6}
              dot={false}
              connectNulls={false}
              isAnimationActive={false}
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
          />

          {/* NOW reference line */}
          <ReferenceLine
            yAxisId="power"
            x={nowMs}
            stroke={COLOR_NOW}
            strokeDasharray="3 3"
            label={{ value: "NOW", position: "top", fontSize: 9, fill: COLOR_NOW }}
          />
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}
