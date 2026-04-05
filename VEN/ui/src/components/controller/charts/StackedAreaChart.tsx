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

interface StackedAreaChartProps {
  data: StackedAreaPoint[];
  assetIds: AssetId[];
  colorMap: Record<string, string>;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
}

function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
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
        padding: "6px 10px",
        fontSize: 12,
      }}
    >
      <div style={{ marginBottom: 4, fontWeight: "bold" }}>{time}</div>
      {Object.entries(netByAsset).map(([assetId, kw]) => (
        <div key={assetId} style={{ color: colorMap[assetId] ?? "#888" }}>
          {assetId}: {kw >= 0 ? "+" : ""}
          {kw.toFixed(2)} kW
        </div>
      ))}
      {gridKw !== null && (
        <div style={{ color: "#212121", borderTop: "1px solid #eee", marginTop: 4, paddingTop: 4 }}>
          Grid: {gridKw >= 0 ? "+" : ""}
          {gridKw.toFixed(2)} kW
        </div>
      )}
    </div>
  );
}

const EMPTY_PT = (): Omit<StackedAreaPoint, "ts"> => ({
  ev_pos: 0, ev_neg: 0,
  heater_pos: 0, heater_neg: 0,
  pv_pos: 0, pv_neg: 0,
  battery_pos: 0, battery_neg: 0,
  base_load_pos: 0, base_load_neg: 0,
  gridPowerKw: null,
});

export function StackedAreaChart({
  data,
  assetIds,
  colorMap,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
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
          { ts: tMin, ...EMPTY_PT() },
          { ts: tMax, ...EMPTY_PT() },
        ];

  // base_load first so it sits closest to the X axis in both stacks.
  const renderOrder: AssetId[] = [
    ...assetIds.filter((id) => id === "base_load"),
    ...assetIds.filter((id) => id !== "base_load"),
  ];

  return (
    <div data-testid="accumulated-area-chart" style={{ width: "100%", height: 160 }}>
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
          <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />

          {/* For each asset: positive series (import, stacked above x-axis) */}
          {renderOrder.map((id) => (
            <Area
              key={`${id}_pos`}
              yAxisId="power"
              type="stepAfter"
              dataKey={`${id}_pos`}
              name={`${id} +`}
              stackId="positive"
              fill={colorMap[id] ?? "#888"}
              stroke={colorMap[id] ?? "#888"}
              fillOpacity={0.6}
              dot={false}
              connectNulls={false}
              isAnimationActive={false}
            />
          ))}

          {/* For each asset: negative series (export, stacked below x-axis) */}
          {renderOrder.map((id) => (
            <Area
              key={`${id}_neg`}
              yAxisId="power"
              type="stepAfter"
              dataKey={`${id}_neg`}
              name={`${id} -`}
              stackId="negative"
              fill={colorMap[id] ?? "#888"}
              stroke={colorMap[id] ?? "#888"}
              fillOpacity={0.3}
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
            stroke="#212121"
            strokeWidth={2}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />

          {/* NOW reference line */}
          <ReferenceLine
            yAxisId="power"
            x={nowMs}
            stroke="#f44336"
            strokeDasharray="3 3"
            label={{ value: "NOW", position: "top", fontSize: 9, fill: "#f44336" }}
          />
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}
