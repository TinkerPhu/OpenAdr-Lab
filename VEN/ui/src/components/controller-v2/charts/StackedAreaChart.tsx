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
}

function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function StackedAreaChart({ data, assetIds, colorMap, nowMs }: StackedAreaChartProps) {
  return (
    <div data-testid="accumulated-area-chart" style={{ width: "100%", height: 160 }}>
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={data} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
          <XAxis
            dataKey="ts"
            scale="time"
            type="number"
            domain={["auto", "auto"]}
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
            />
          ))}

          {/* NOW reference line */}
          <ReferenceLine
            x={nowMs}
            stroke="#f44336"
            strokeDasharray="3 3"
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}
