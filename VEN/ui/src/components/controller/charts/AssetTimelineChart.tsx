import type { AssetTimelinePoint } from "../types";
import type { ZoneDef, ForecastAccuracySample } from "../../../api/types";
import {
  minSpanDomain,
  MIN_COST_RATE_SPAN_EUR_H,
  MIN_CO2_RATE_SPAN_G_H,
  MIN_POWER_SPAN_KW,
  formatPowerTick,
  roundedTimeTicks,
  POWER_AXIS_WIDTH_PX,
} from "@lab/charts/axisDomain";
import {
  formatPowerValue,
  formatCostRateEurH,
  formatCo2RateGH,
  formatSocPct,
  formatTemperatureC,
} from "@lab/charts/unitFormat";
import { mergeTimestampedSeries, locfFillKeys, clipRowsToWindow, type TimestampedRow } from "@lab/charts/mergeSeries";
import { CELL_CHART_HEIGHT } from "@lab/charts/chartLayout";
import { TimeSeriesChart, type TimeSeriesSeriesSpec, type TimeSeriesAxisSpec } from "@lab/charts/TimeSeriesChart";
import { renderStateShading, type StateShadingSpec } from "@lab/charts/StateShading";

interface AssetTimelineChartProps {
  data: AssetTimelinePoint[];
  color: string;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  stateKey?: "soc" | "temp_c";
  zones?: ZoneDef[];
  /** Time ranges to shade, classified from each point's `values` — the asset's own
   * declaration from `assetChartSpecs.ts` (PV curtailment, EV unplugged). */
  shadings?: StateShadingSpec[];
  /** Two consecutive points further apart than this are not joined by a shading. Pass it
   * where the data has a known cadence and may have holes (the History page's one-minute
   * rows across a VEN restart); omit where spacing is irregular by design (plan slots). */
  shadingMaxGapMs?: number;
  /** Minimum power-axis span [kW]. The power Y-axis never auto-zooms narrower than this, even
   * when every visible point is near zero — see `MIN_POWER_SPAN_KW` in `axisDomain.ts`. Defaults
   * to that 1 W floor for every caller (Controller and History tabs both render through this one
   * component); override only for a chart that genuinely needs a different floor. */
  minPowerSpanKw?: number;
  /** forecast-accuracy-tracking: the plan's near-lead (`slots[1]`) forecast sample for this
   * asset from each plan cycle, overlaid on the power axis alongside the actual line. History
   * page only — pass for the PV and base_load cells. */
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

export function AssetTimelineChart({
  data,
  color,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  stateKey,
  zones,
  shadings,
  shadingMaxGapMs,
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

  // Clip to the intended window before anything else — a stale/late point outside
  // [tMin, tMax] must never be allowed to widen this chart's time axis (see
  // tariffChartShared.ts's clipToWindow, which the sibling grid/tariff/headroom charts
  // already apply; this was the one chart still passing `data` through unclipped).
  const clipped: TimestampedRow[] = clipRowsToWindow(data, tMin, tMax);

  // Ensure at least a 2-point range so recharts can compute the X scale and render the
  // NOW reference line even when there are no data points yet.
  const rawData: TimestampedRow[] =
    clipped.length > 0 ? clipped : [{ ts: tMin, values: {} }, { ts: tMax, values: {} }];

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

  const powerDomain = minSpanDomain(
    chartData.flatMap((p) => [
      p.values?.["power_kw"] ?? null,
      p.values?.["predicted_kw_near"] ?? null,
      p.values?.["predicted_kw_far"] ?? null,
    ]),
    minPowerSpanKw
  );

  const axes: TimeSeriesAxisSpec[] = [
    { id: "power", width: POWER_AXIS_WIDTH_PX, domain: powerDomain, tickFormatter: formatPowerTick },
    { id: "cost", orientation: "right", width: 44, unit: " €/h", domain: costDomain },
    { id: "co2", orientation: "right", width: 44, unit: " g/h", domain: co2Domain },
    ...(stateKey ? [{ id: "state", hidden: true, domain: (stateKey === "soc" ? [0, 1] : [0, 100]) as [number, number] }] : []),
  ];

  const series: TimeSeriesSeriesSpec[] = [
    {
      key: "Power [kW]",
      axisId: "power",
      dataKey: (row) => row.values?.["power_kw"] ?? null,
      color,
      strokeWidth: 2,
      formatter: formatPowerValue,
    },
    // forecast-accuracy-tracking: near/far forecast overlay — visually distinct from the
    // actual Power line above (thin, dotted, muted) and from each other (dash pattern).
    // Reads the same merged `chartData` as every other line via `dataKey`, so hover/tooltip
    // stays aligned with the actual line. `stepAfter` (not a smooth curve) — each sample is
    // the planner's prediction for one discrete plan slot, holding until the next sample
    // supersedes it, same interpretation as the actual Power line's own `stepAfter`.
    // `connectNulls` stays as a backstop; the LOCF fill above already removes in-range
    // nulls between samples. Declared unconditionally — `TimeSeriesChart`'s own
    // data-presence filtering hides these automatically when nearForecast/farForecast are
    // empty, same as every other series here.
    {
      key: "Forecast (near) [kW]",
      axisId: "power",
      dataKey: (row: TimestampedRow) => row.values?.["predicted_kw_near"] ?? null,
      color,
      strokeWidth: 1,
      strokeOpacity: 0.6,
      strokeDasharray: "2 3",
      connectNulls: true,
      formatter: formatPowerValue,
    },
    {
      key: "Forecast (far) [kW]",
      axisId: "power",
      dataKey: (row: TimestampedRow) => row.values?.["predicted_kw_far"] ?? null,
      color,
      strokeWidth: 1,
      strokeOpacity: 0.6,
      strokeDasharray: "6 3",
      connectNulls: true,
      formatter: formatPowerValue,
    },
    {
      key: "Cost rate [€/h]",
      axisId: "cost",
      dataKey: (row) => row.values?.["cost_rate_eur_h"] ?? null,
      color,
      strokeWidth: 1.5,
      strokeDasharray: "5 5",
      formatter: formatCostRateEurH,
    },
    {
      key: "CO₂eq rate [g/h]",
      axisId: "co2",
      dataKey: (row) => row.values?.["co2_rate_g_h"] ?? null,
      color,
      strokeWidth: 1.5,
      strokeDasharray: "2 2",
      formatter: formatCo2RateGH,
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
            formatter: stateKey === "soc" ? formatSocPct : formatTemperatureC,
          },
        ]
      : []),
  ];

  // Each declared shading goes to the layer it asks for: `background` under the grid and
  // the lines, `overlay` over them (see `TimeSeriesChart`'s two slots).
  const shadingAreas = (layer: StateShadingSpec["layer"]) =>
    (shadings ?? [])
      .filter((spec) => spec.layer === layer)
      .flatMap((spec) =>
        renderStateShading("power", chartData, spec, { nowMs, maxGapMs: shadingMaxGapMs }),
      );

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
      backgroundAreas={shadingAreas("background")}
      extraReferenceAreas={shadingAreas("overlay")}
      interactiveLegend
      height={CELL_CHART_HEIGHT}
      margin={{ top: 4, right: 4, left: 0, bottom: 0 }}
    />
  );
}
