import {
  ComposedChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceLine,
  Legend,
  ResponsiveContainer,
} from "recharts";
import type { AssetTimePoint } from "../types";

interface AssetTimelineChartProps {
  data: AssetTimePoint[];
  color: string;
  nowMs: number;
}

function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function AssetTimelineChart({ data, color, nowMs }: AssetTimelineChartProps) {
  // When data is empty, provide a fallback domain so the NOW reference line renders
  const xDomain: [number, number] | ["auto", "auto"] =
    data.length > 0
      ? ["auto", "auto"]
      : [nowMs - 3_600_000, nowMs + 3_600_000];

  return (
    <ResponsiveContainer width="100%" height={140}>
      <ComposedChart data={data} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
        <XAxis
          dataKey="ts"
          scale="time"
          type="number"
          domain={xDomain}
          tickFormatter={formatTs}
          tick={{ fontSize: 10 }}
        />
        <YAxis yAxisId="power" tick={{ fontSize: 10 }} width={40} />
        <YAxis yAxisId="rates" orientation="right" tick={{ fontSize: 10 }} width={40} />
        <Tooltip
          labelFormatter={(v) => new Date(v as number).toLocaleTimeString()}
          formatter={(value: number, name: string) => [value.toFixed(3), name]}
        />
        <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />

        {/* Power — solid */}
        <Line
          yAxisId="power"
          type="stepAfter"
          dataKey="powerKw"
          name="Power [kW]"
          stroke={color}
          strokeWidth={2}
          dot={false}
          connectNulls={false}
        />

        {/* Cost rate — dashed */}
        <Line
          yAxisId="rates"
          type="stepAfter"
          dataKey="costRateEurH"
          name="Cost rate [€/h]"
          stroke={color}
          strokeWidth={1.5}
          strokeDasharray="5 5"
          dot={false}
          connectNulls={false}
        />

        {/* CO₂eq rate — dotted */}
        <Line
          yAxisId="rates"
          type="stepAfter"
          dataKey="co2RateGH"
          name="CO₂eq rate [g/h]"
          stroke={color}
          strokeWidth={1.5}
          strokeDasharray="2 2"
          dot={false}
          connectNulls={false}
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
  );
}
