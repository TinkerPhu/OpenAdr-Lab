import { CELL_CHART_HEIGHT } from "../chartLayout";
import {
  ComposedChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceLine,
  ReferenceArea,
  Legend,
  ResponsiveContainer,
} from "recharts";
import type { AssetTimelinePoint } from "../types";
import { COLOR_NOW } from "../types";
import type { ZoneDef } from "../../../api/types";
import {
  minSpanDomain,
  MIN_COST_RATE_SPAN_EUR_H,
  MIN_CO2_RATE_SPAN_G_H,
  MIN_POWER_SPAN_KW,
  formatPowerTick,
} from "./axisDomain";

interface AssetTimelineChartProps {
  data: AssetTimelinePoint[];
  color: string;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  stateKey?: "soc" | "temp_c";
  zones?: ZoneDef[];
  /** PV only: shade curtailment bands derived from `values` (generation_limit_kw,
   * curtailment_source, inverter_max_kw for past points; pv_forecast_kw for future points).
   * See openspec/changes/pv-curtailment-history/. */
  pvCurtailment?: boolean;
  /** Minimum power-axis span [kW]. The power Y-axis never auto-zooms narrower than this, even
   * when every visible point is near zero — see `MIN_POWER_SPAN_KW` in `axisDomain.ts`. Defaults
   * to that 1 W floor for every caller (Controller and History tabs both render through this one
   * component); override only for a chart that genuinely needs a different floor. */
  minPowerSpanKw?: number;
}

function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

const CURTAILMENT_EPS_KW = 0.05;

type CurtailmentKind = "hardware" | "planned" | "unplanned";

interface CurtailmentZone {
  x1: number;
  x2: number;
  kind: CurtailmentKind;
}

/** Classify one point's curtailment state from its `values` map. `null` = no shading.
 *
 * Past points carry `generation_limit_kw` (the commanded limit, present only when a limit was
 * active), `curtailment_source` (0=none, 1=plan, 2=capacity, 3=arbiter, 4=manual — only
 * meaningful alongside generation_limit_kw), and `inverter_max_kw` (the static hardware
 * ceiling). Capacity, arbiter, and manual sources are all externally-imposed/reactive, not the
 * plan's own forecasted choice, so all three classify as "unplanned" (see
 * `openspec/changes/deviation-arbiter/`). Future (plan) points instead carry `pv_forecast_kw`
 * next to `power_kw` — the plan's forecast is already clamped to `inverter_max_kw` at solve
 * time, so any gap there is always a planned choice, never a hardware ceiling to distinguish
 * separately.
 */
function classifyPvPoint(values: Record<string, number> | null | undefined): CurtailmentKind | null {
  const powerKw = values?.["power_kw"];
  if (powerKw == null) return null;

  const pvForecastKw = values?.["pv_forecast_kw"];
  if (pvForecastKw != null) {
    // Future (plan) point.
    return pvForecastKw - -powerKw > CURTAILMENT_EPS_KW ? "planned" : null;
  }

  // Past (history) point.
  const generationLimitKw = values?.["generation_limit_kw"];
  const inverterMaxKw = values?.["inverter_max_kw"];
  if (
    generationLimitKw != null &&
    Math.abs(powerKw - generationLimitKw) < CURTAILMENT_EPS_KW &&
    (inverterMaxKw == null || Math.abs(generationLimitKw) < inverterMaxKw - CURTAILMENT_EPS_KW)
  ) {
    const source = values?.["curtailment_source"];
    return source === 2 || source === 3 || source === 4 ? "unplanned" : "planned";
  }
  if (inverterMaxKw != null && Math.abs(-powerKw - inverterMaxKw) < CURTAILMENT_EPS_KW) {
    return "hardware";
  }
  return null;
}

const CURTAILMENT_COLORS: Record<CurtailmentKind, string> = {
  hardware: "rgba(120,120,120,0.15)",
  planned: "rgba(230,160,20,0.18)",
  unplanned: "rgba(210,30,30,0.22)",
};

/** Derive contiguous shaded bands from per-point curtailment classification. Each band spans
 * from its first point's ts to the next point's ts (or, for the final run, one extra point-gap
 * beyond the last ts so the band remains visible). */
function buildCurtailmentZones(data: AssetTimelinePoint[]): CurtailmentZone[] {
  const zones: CurtailmentZone[] = [];
  let runStart: number | null = null;
  let runKind: CurtailmentKind | null = null;

  const closeRun = (endTs: number) => {
    if (runStart != null && runKind != null) {
      zones.push({ x1: runStart, x2: endTs, kind: runKind });
    }
    runStart = null;
    runKind = null;
  };

  for (let i = 0; i < data.length; i++) {
    const kind = classifyPvPoint(data[i].values);
    if (kind !== runKind) {
      closeRun(data[i].ts);
      if (kind) {
        runStart = data[i].ts;
        runKind = kind;
      }
    }
  }
  if (runStart != null && runKind != null) {
    const lastTs = data[data.length - 1]?.ts ?? runStart;
    const prevTs = data.length > 1 ? data[data.length - 2].ts : runStart;
    const step = Math.max(lastTs - prevTs, 1);
    closeRun(lastTs + step);
  }
  return zones;
}

export function AssetTimelineChart({
  data,
  color,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  stateKey,
  zones,
  pvCurtailment,
  minPowerSpanKw = MIN_POWER_SPAN_KW,
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

  const costDomain = minSpanDomain(
    chartData.map((p) => p.values?.["cost_rate_eur_h"] ?? null),
    MIN_COST_RATE_SPAN_EUR_H
  );
  const co2Domain = minSpanDomain(
    chartData.map((p) => p.values?.["co2_rate_g_h"] ?? null),
    MIN_CO2_RATE_SPAN_G_H
  );

  const curtailmentZones = pvCurtailment ? buildCurtailmentZones(chartData) : [];

  const powerDomain = minSpanDomain(
    chartData.map((p) => p.values?.["power_kw"] ?? null),
    minPowerSpanKw
  );

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
        <YAxis
          yAxisId="power"
          tick={{ fontSize: 10 }}
          width={46}
          tickFormatter={formatPowerTick}
          domain={powerDomain}
        />
        <YAxis
          yAxisId="cost"
          orientation="right"
          tick={{ fontSize: 10 }}
          width={44}
          unit=" €"
          domain={costDomain}
        />
        <YAxis
          yAxisId="co2"
          orientation="right"
          tick={{ fontSize: 10 }}
          width={44}
          unit=" g"
          domain={co2Domain}
        />
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

        {/* Zone background shading — rendered before data lines so they sit behind */}
        {zones?.map((z, i) => (
          <ReferenceArea
            key={z.from}
            yAxisId="power"
            x1={new Date(z.from).getTime()}
            x2={new Date(z.to).getTime()}
            fill={`rgba(0,0,0,${0.04 * (i + 1)})`}
            ifOverflow="hidden"
          />
        ))}

        {/* PV curtailment shading: hardware-capped (neutral), planned (amber), unplanned (red) */}
        {curtailmentZones.map((z, i) => (
          <ReferenceArea
            key={`curtail-${i}-${z.x1}`}
            yAxisId="power"
            x1={z.x1}
            x2={z.x2}
            fill={CURTAILMENT_COLORS[z.kind]}
            ifOverflow="hidden"
          />
        ))}

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
