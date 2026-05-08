import { CELL_CHART_HEIGHT } from "../chartLayout";
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
import { COLOR_NOW } from "../types";

interface AssetTimelineChartProps {
  data: AssetTimelinePoint[];
  color: string;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  stateKey?: "soc" | "temp_c";
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
  stateKey,
}: AssetTimelineChartProps) {
  // Domain driven by hoursBack/hoursForward keeps the X-axis stable across refreshes.
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;

  // Ensure at least a 2-point range so recharts can compute the X scale and render the
  // NOW reference line even when there are no data points yet.
  const rawData: AssetTimelinePoint[] =
    data.length > 0 ? data : [{ ts: tMin, values: {} }, { ts: tMax, values: {} }];

  // LOCF: carry the last known state value (soc / temp_c) into future slots where
  // the backend emits no state — ensures the tooltip always shows the current state.
  const chartData: AssetTimelinePoint[] = stateKey
    ? (() => {
        let last: number | null = null;
        return rawData.map((pt) => {
          const v = pt.values?.[stateKey] ?? null;
          if (v !== null) { last = v; return pt; }
          if (last === null) return pt;
          return { ...pt, values: { ...(pt.values ?? {}), [stateKey]: last } };
        });
      })()
    : rawData;

  return (
    <ResponsiveContainer width="100%" height={CELL_CHART_HEIGHT}>
      <ComposedChart data={chartData} margin={{ top: 4, right: 4, left: 0, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
        <XAxis
          dataKey="ts"
          scale="time"
          type="number"
          domain={[tMin, tMax]}
          tickFormatter={formatTs}
          tick={{ fontSize: 10 }}
        />
        <YAxis yAxisId="power" tick={{ fontSize: 10 }} width={40} unit=" kW" />
        <YAxis yAxisId="cost" orientation="right" tick={{ fontSize: 10 }} width={44} unit=" €" />
        <YAxis yAxisId="co2" orientation="right" tick={{ fontSize: 10 }} width={44} unit=" g" />
        {stateKey && (
          <YAxis
            yAxisId="state"
            axisLine={false}
            tickLine={false}
            tick={false}
            width={0}
            domain={stateKey === "soc" ? [0, 1] : [0, 100]}
          />
        )}
        <Tooltip
          contentStyle={{ fontSize: 9, padding: "1px 5px" }}
          itemStyle={{ padding: "0" }}
          labelStyle={{ fontSize: 9, marginBottom: 1 }}
          labelFormatter={(v) => new Date(v as number).toLocaleTimeString()}
          formatter={(value: number, name: string) => {
            if (name === "CO₂eq rate [g/h]") return [value.toFixed(1) + " g/h", name];
            if (name === "Cost rate [€/h]") return [value.toFixed(4) + " €/h", name];
            if (name === "SoC [%]") return [(value * 100).toFixed(1) + " %", name];
            if (name === "T_tank [°C]") return [value.toFixed(1) + " °C", name];
            return [value.toFixed(3) + " kW", name];
          }}
        />
        <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />

        {/* Power — solid. Accessor function required; dataKey dot-notation cannot traverse nested maps. */}
        <Line
          yAxisId="power"
          type="stepAfter"
          dataKey={(pt: AssetTimelinePoint) => pt.values?.["power_kw"] ?? null}
          name="Power [kW]"
          stroke={color}
          strokeWidth={2}
          dot={false}
          connectNulls={false}
          isAnimationActive={false}
        />

        {/* Cost rate — dashed, right axis */}
        <Line
          yAxisId="cost"
          type="stepAfter"
          dataKey={(pt: AssetTimelinePoint) => pt.values?.["cost_rate_eur_h"] ?? null}
          name="Cost rate [€/h]"
          stroke={color}
          strokeWidth={1.5}
          strokeDasharray="5 5"
          dot={false}
          connectNulls={false}
          isAnimationActive={false}
        />

        {/* CO₂eq rate — dotted, second right axis */}
        <Line
          yAxisId="co2"
          type="stepAfter"
          dataKey={(pt: AssetTimelinePoint) => pt.values?.["co2_rate_g_h"] ?? null}
          name="CO₂eq rate [g/h]"
          stroke={color}
          strokeWidth={1.5}
          strokeDasharray="2 2"
          dot={false}
          connectNulls={false}
          isAnimationActive={false}
        />

        {/* State line: SoC (EV/battery) or T_tank (heater) — hidden axis, tooltip-only values */}
        {stateKey && (
          <Line
            yAxisId="state"
            type="monotone"
            dataKey={(pt: AssetTimelinePoint) => pt.values?.[stateKey] ?? null}
            name={stateKey === "soc" ? "SoC [%]" : "T_tank [°C]"}
            stroke={color}
            strokeWidth={1.5}
            strokeDasharray="4 2"
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />
        )}

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
  );
}
