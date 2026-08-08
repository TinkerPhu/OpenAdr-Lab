import { ReferenceArea } from "recharts";
import type { AssetTimelinePoint } from "../types";
import type { ZoneDef, ForecastAccuracySample } from "../../../api/types";
import {
  minSpanDomain,
  MIN_COST_RATE_SPAN_EUR_H,
  MIN_CO2_RATE_SPAN_G_H,
  MIN_POWER_SPAN_KW,
  formatPowerTick,
  roundedTimeTicks,
  zeroAnchoredTicks,
} from "../../charts/axisDomain";
import {
  formatPowerValue,
  formatCostRateEurH,
  formatCo2RateGH,
  formatSocPct,
  formatTemperatureC,
} from "../../charts/unitFormat";
import { mergeTimestampedSeries, locfFillKeys, type TimestampedRow } from "../../charts/mergeSeries";
import { CELL_CHART_HEIGHT } from "../../charts/chartLayout";
import { TimeSeriesChart, type TimeSeriesSeriesSpec, type TimeSeriesAxisSpec } from "../../charts/TimeSeriesChart";

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
  /** forecast-accuracy-tracking: the plan's near-lead (`slots[1]`) forecast sample for this
   * asset from each plan cycle, overlaid on the power axis alongside the actual line. History
   * page only — pass for the PV, base_load, and site-residual cells. */
  nearForecast?: ForecastAccuracySample[];
  /** Same as `nearForecast`, but the far-lead (`slots.last()`) sample. */
  farForecast?: ForecastAccuracySample[];
  /** X-axis ticks every N minutes, snapped to the wall-clock (10:00, 10:30, ...) instead of
   * recharts' default "nice" ticks. History page only — Controller's real-time cells keep the
   * default behavior. */
  xAxisTickIntervalMinutes?: number;
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
function classifyPvPoint(values: Record<string, number | null> | null | undefined): CurtailmentKind | null {
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
function buildCurtailmentZones(data: TimestampedRow[]): CurtailmentZone[] {
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
  nearForecast,
  farForecast,
  xAxisTickIntervalMinutes,
}: AssetTimelineChartProps) {
  // Domain driven by hoursBack/hoursForward keeps the X-axis stable across refreshes.
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;
  const xAxisTicks = xAxisTickIntervalMinutes
    ? roundedTimeTicks(tMin, tMax, xAxisTickIntervalMinutes)
    : undefined;

  // Ensure at least a 2-point range so recharts can compute the X scale and render the
  // NOW reference line even when there are no data points yet.
  const rawData: AssetTimelinePoint[] =
    data.length > 0 ? data : [{ ts: tMin, values: {} }, { ts: tMax, values: {} }];

  // forecast-accuracy-tracking: folded into the SAME per-ts array as the actual line
  // (rather than passed to their own `<Line data={...}>` override) so every series shares
  // one index space — see mergeSeries.ts's doc comment for the bug this structurally
  // prevents (recharts resolves tooltip hover by array index, not by re-matching
  // timestamps across a series' own overridden `data`).
  const foldSamples = (samples: ForecastAccuracySample[] | undefined, key: string) =>
    (samples ?? []).map((s) => ({ ts: s.target_ts, key, value: s.predicted_kw }));
  const merged: TimestampedRow[] = mergeTimestampedSeries(rawData, [
    ...foldSamples(nearForecast, "predicted_kw_near"),
    ...foldSamples(farForecast, "predicted_kw_far"),
  ]);

  // LOCF: carry the last known value forward into slots where a key has no sample —
  // state (soc / temp_c) needs this for the tooltip to always show the current state;
  // the near/far forecast samples need it so their step-function line (rendered
  // `type="stepAfter"`, same as the actual Power line) has a value at every one-minute
  // slot between two ~5-minute-apart samples, not just the sample points themselves —
  // otherwise `connectNulls` would draw the step but hovering the plateau in between
  // would show no forecast value, disagreeing with what's drawn.
  const locfKeys = [...(stateKey ? [stateKey] : []), "predicted_kw_near", "predicted_kw_far"];
  const chartData: TimestampedRow[] = locfFillKeys(merged, locfKeys);

  const costDomain = minSpanDomain(
    chartData.map((p) => p.values?.["cost_rate_eur_h"] ?? null),
    MIN_COST_RATE_SPAN_EUR_H
  );
  const co2Domain = minSpanDomain(
    chartData.map((p) => p.values?.["co2_rate_g_h"] ?? null),
    MIN_CO2_RATE_SPAN_G_H
  );

  const curtailmentZones = pvCurtailment ? buildCurtailmentZones(chartData) : [];

  const hasNearForecast = (nearForecast ?? []).length > 0;
  const hasFarForecast = (farForecast ?? []).length > 0;

  const powerDomain = minSpanDomain(
    chartData.flatMap((p) => [
      p.values?.["power_kw"] ?? null,
      p.values?.["predicted_kw_near"] ?? null,
      p.values?.["predicted_kw_far"] ?? null,
    ]),
    minPowerSpanKw
  );

  const axes: TimeSeriesAxisSpec[] = [
    { id: "power", width: 46, domain: powerDomain, tickFormatter: formatPowerTick, ticks: zeroAnchoredTicks(powerDomain) },
    { id: "cost", orientation: "right", width: 44, unit: " €/h", domain: costDomain, ticks: zeroAnchoredTicks(costDomain) },
    { id: "co2", orientation: "right", width: 44, unit: " g/h", domain: co2Domain, ticks: zeroAnchoredTicks(co2Domain) },
    ...(stateKey ? [{ id: "state", hidden: true, domain: (stateKey === "soc" ? [0, 1] : [0, 100]) as [number, number] }] : []),
  ];

  const series: TimeSeriesSeriesSpec[] = [
    {
      key: "Power [kW]",
      axisId: "power",
      dataKey: (row) => row.values?.["power_kw"] ?? null,
      color,
      strokeWidth: 2,
    },
    // forecast-accuracy-tracking: near/far forecast overlay — visually distinct from the
    // actual Power line above (thin, dotted, muted) and from each other (dash pattern).
    // Reads the same merged `chartData` as every other line via `dataKey`, so hover/tooltip
    // stays aligned with the actual line. `stepAfter` (not a smooth curve) — each sample is
    // the planner's prediction for one discrete plan slot, holding until the next sample
    // supersedes it, same interpretation as the actual Power line's own `stepAfter`.
    // `connectNulls` stays as a backstop; the LOCF fill above already removes in-range
    // nulls between samples.
    ...(hasNearForecast
      ? [
          {
            key: "Forecast (near) [kW]",
            axisId: "power",
            dataKey: (row: TimestampedRow) => row.values?.["predicted_kw_near"] ?? null,
            color,
            strokeWidth: 1,
            strokeOpacity: 0.6,
            strokeDasharray: "2 3",
            connectNulls: true,
          },
        ]
      : []),
    ...(hasFarForecast
      ? [
          {
            key: "Forecast (far) [kW]",
            axisId: "power",
            dataKey: (row: TimestampedRow) => row.values?.["predicted_kw_far"] ?? null,
            color,
            strokeWidth: 1,
            strokeOpacity: 0.6,
            strokeDasharray: "6 3",
            connectNulls: true,
          },
        ]
      : []),
    {
      key: "Cost rate [€/h]",
      axisId: "cost",
      dataKey: (row) => row.values?.["cost_rate_eur_h"] ?? null,
      color,
      strokeWidth: 1.5,
      strokeDasharray: "5 5",
    },
    {
      key: "CO₂eq rate [g/h]",
      axisId: "co2",
      dataKey: (row) => row.values?.["co2_rate_g_h"] ?? null,
      color,
      strokeWidth: 1.5,
      strokeDasharray: "2 2",
    },
    ...(stateKey
      ? [
          {
            key: stateKey === "soc" ? "SoC [%]" : "T_tank [°C]",
            axisId: "state",
            dataKey: (row: TimestampedRow) => row.values?.[stateKey] ?? null,
            color,
            strokeWidth: 1.5,
            strokeDasharray: "4 2",
            type: "monotone" as const,
          },
        ]
      : []),
  ];

  // PV curtailment shading: hardware-capped (neutral), planned (amber), unplanned (red) —
  // asset-specific overlay, not the generic zone-shading primitive.
  const curtailmentAreas = curtailmentZones.map((z, i) => (
    <ReferenceArea
      key={`curtail-${i}-${z.x1}`}
      yAxisId="power"
      x1={z.x1}
      x2={z.x2}
      fill={CURTAILMENT_COLORS[z.kind]}
      ifOverflow="hidden"
    />
  ));

  return (
    <TimeSeriesChart
      data={chartData}
      tMin={tMin}
      tMax={tMax}
      xAxisTickFormatter={formatTs}
      xAxisTicks={xAxisTicks}
      axes={axes}
      series={series}
      nowMs={nowMs}
      referenceAxisId="power"
      zones={zones}
      extraReferenceAreas={curtailmentAreas}
      tooltipFormatter={(value, name) => {
        if (name === "CO₂eq rate [g/h]") return [formatCo2RateGH(value), name];
        if (name === "Cost rate [€/h]") return [formatCostRateEurH(value), name];
        if (name === "SoC [%]") return [formatSocPct(value), name];
        if (name === "T_tank [°C]") return [formatTemperatureC(value), name];
        return [formatPowerValue(value), name];
      }}
      height={CELL_CHART_HEIGHT}
      margin={{ top: 4, right: 4, left: 0, bottom: 0 }}
    />
  );
}
