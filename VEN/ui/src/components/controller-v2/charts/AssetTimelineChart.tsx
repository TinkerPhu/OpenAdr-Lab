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
import type { AssetTimelinePoint } from "../types";

interface AssetTimelineChartProps {
  data: AssetTimelinePoint[];
  color: string;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
}

function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function AssetTimelineChart({
  data,
  color,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
}: AssetTimelineChartProps) {
  // Ensure at least a 2-point range so recharts can compute the X scale and render the
  // NOW reference line even when there are no data points yet.
  const chartData: AssetTimelinePoint[] =
    data.length > 0
      ? data
      : [
          { ts: nowMs - hoursBack * 3_600_000, values: {} },
          { ts: nowMs + hoursForward * 3_600_000, values: {} },
        ];

  // Domain driven by hoursBack/hoursForward keeps the X-axis stable across refreshes.
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;

  return (
    <ResponsiveContainer width="100%" height={140}>
      <ComposedChart data={chartData} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
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
        <YAxis yAxisId="rates" orientation="right" tick={{ fontSize: 10 }} width={40} />
        <Tooltip
          labelFormatter={(v) => new Date(v as number).toLocaleTimeString()}
          formatter={(value: number, name: string) => [value.toFixed(3), name]}
        />
        <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />

        {/* Power — solid. Accessor function required; dataKey dot-notation cannot traverse nested maps. */}
        <Line
          yAxisId="power"
          type="stepAfter"
          dataKey={(pt: AssetTimelinePoint) => pt.values["power_kw"] ?? null}
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
          dataKey={(pt: AssetTimelinePoint) => pt.values["cost_rate_eur_h"] ?? null}
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
          dataKey={(pt: AssetTimelinePoint) => pt.values["co2_rate_g_h"] ?? null}
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
