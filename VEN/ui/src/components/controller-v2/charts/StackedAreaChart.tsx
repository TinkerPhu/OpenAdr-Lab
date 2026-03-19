import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceLine,
  Legend,
  ResponsiveContainer,
} from "recharts";
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

const EMPTY_PT = (): Omit<StackedAreaPoint, "ts"> => ({
  ev_pos: 0, ev_neg: 0,
  heater_pos: 0, heater_neg: 0,
  pv_pos: 0, pv_neg: 0,
  battery_pos: 0, battery_neg: 0,
  base_load_pos: 0, base_load_neg: 0,
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

  return (
    <div data-testid="accumulated-area-chart" style={{ width: "100%", height: 160 }}>
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={chartData} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
          <XAxis
            dataKey="ts"
            scale="time"
            type="number"
            domain={[tMin, tMax]}
            tickFormatter={formatTs}
            tick={{ fontSize: 10 }}
          />
          <YAxis tick={{ fontSize: 10 }} width={40} />
          <Tooltip
            labelFormatter={(v) => new Date(v as number).toLocaleTimeString()}
            formatter={(value: number, name: string) => [value.toFixed(2), name]}
          />
          <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />

          {/* For each asset: positive series (import, stacked above x-axis) */}
          {assetIds.map((id) => (
            <Area
              key={`${id}_pos`}
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
          {assetIds.map((id) => (
            <Area
              key={`${id}_neg`}
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

          {/* NOW reference line */}
          <ReferenceLine
            x={nowMs}
            stroke="#f44336"
            strokeDasharray="3 3"
            label={{ value: "NOW", position: "top", fontSize: 9, fill: "#f44336" }}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}
