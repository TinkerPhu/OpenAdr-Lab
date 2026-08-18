import type { AssetTimelinePoint } from "../types";
import type { SiteFlexibilitySample, SiteFlexibilityForecastSlot } from "../../../api/types";
import type { NamedSample, TimestampedRow } from "../../charts/mergeSeries";
import {
  mergeTimestampedSeries,
  locfFillKeys,
  clipRowsToWindow,
  ensureNonEmptyRows,
} from "../../charts/mergeSeries";
import {
  minSpanDomain,
  MIN_POWER_SPAN_KW,
  roundedTimeTicks,
  zeroAnchoredTicks,
  formatPowerTick,
} from "../../charts/axisDomain";
import { formatSignedPowerValue, formatPowerValue } from "../../charts/unitFormat";
import { CELL_CHART_HEIGHT } from "../../charts/chartLayout";
import { TimeSeriesChart, type TimeSeriesSeriesSpec } from "../../charts/TimeSeriesChart";
import { formatTs } from "./tariffChartShared";

interface SiteHeadroomChartProps {
  /** `allTimelines["grid"]` — already threaded to every other grid cell; its shape is
   * structurally a `TimestampedRow[]`, used directly as the merge base. */
  gridTimeline: AssetTimelinePoint[];
  /** BL-43: the site-headroom ring (`GET /flexibility/history`), oldest first. */
  history: SiteFlexibilitySample[];
  /** Forward-looking per-slot trajectory (`GET /flexibility/forecast`); optional so
   * this component still works wherever only the past ring is available. */
  forecast?: SiteFlexibilityForecastSlot[];
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  height?: number;
  /** X-axis ticks every N minutes, snapped to the wall-clock (10:00, 10:30, ...) instead of
   * recharts' default "nice" ticks — same mechanism as GridRatesChart/TariffEnvelopeChart. */
  xAxisTickIntervalMinutes?: number;
}

/**
 * BL-43: live site-level flexibility (`SiteFlexibilityEnvelope`, VEN-derived, valid only for
 * the current instant) plotted as a shaded band around the grid-power line — distinct from
 * `TariffEnvelopeChart`'s Dynamic Operating Envelope (`IMPORT/EXPORT_CAPACITY_LIMIT`), which
 * is a VTN-announced forward *schedule*, not a live headroom value.
 */
export function SiteHeadroomChart({
  gridTimeline,
  history,
  forecast = [],
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  height,
  xAxisTickIntervalMinutes,
}: SiteHeadroomChartProps) {
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;
  const xAxisTicks = xAxisTickIntervalMinutes
    ? roundedTimeTicks(tMin, tMax, xAxisTickIntervalMinutes)
    : undefined;

  const gridRows: TimestampedRow[] = gridTimeline.map((p) => ({
    ts: p.ts,
    values: { gridPowerKw: p.values?.["power_kw"] ?? null },
  }));
  const upSamples: NamedSample[] = history.map((s) => ({
    ts: new Date(s.ts).getTime(),
    key: "upKw",
    value: s.up_kw,
  }));
  const downSamples: NamedSample[] = history.map((s) => ({
    ts: new Date(s.ts).getTime(),
    key: "downKw",
    value: s.down_kw,
  }));
  // Forecast supplies genuine future per-slot values instead of letting LOCF
  // flat-extend the last historical sample across the whole forward window.
  const forecastUpSamples: NamedSample[] = forecast.map((s) => ({
    ts: new Date(s.ts).getTime(),
    key: "upKw",
    value: s.up_kw,
  }));
  const forecastDownSamples: NamedSample[] = forecast.map((s) => ({
    ts: new Date(s.ts).getTime(),
    key: "downKw",
    value: s.down_kw,
  }));

  const merged = mergeTimestampedSeries(gridRows, [
    ...upSamples,
    ...downSamples,
    ...forecastUpSamples,
    ...forecastDownSamples,
  ]);
  // LOCF now only bridges minor timestamp misalignment between the headroom
  // samples (history/forecast) and the coarser-resolution grid timeline —
  // gridPowerKw itself must be filled too (not just upKw/downKw): the band's
  // lower/upper accessors require gridPowerKw non-null on the SAME row as
  // upKw/downKw, and since real grid rows are sparse relative to the dense
  // history rows, leaving gridPowerKw real-only meant almost no row ever had
  // both — the band rendered nothing. Consistent with the line's own default
  // `type="stepAfter"` rendering: a forward-filled step value is exactly what
  // that shape already implies.
  const filled = locfFillKeys(merged, ["upKw", "downKw", "gridPowerKw"]);
  const clipped = clipRowsToWindow(filled, tMin, tMax);
  const chartData = ensureNonEmptyRows(clipped, tMin, tMax);

  const domain = minSpanDomain(
    chartData.flatMap((row) => [
      row.values?.gridPowerKw,
      row.values?.gridPowerKw != null && row.values?.upKw != null
        ? row.values.gridPowerKw - row.values.upKw
        : null,
      row.values?.gridPowerKw != null && row.values?.downKw != null
        ? row.values.gridPowerKw + row.values.downKw
        : null,
    ]),
    MIN_POWER_SPAN_KW
  );

  const series: TimeSeriesSeriesSpec[] = [
    {
      key: "Grid power [kW]",
      axisId: "power",
      dataKey: (row) => row.values?.["gridPowerKw"] ?? null,
      color: "#212121",
      connectNulls: true,
      formatter: formatSignedPowerValue,
    },
  ];

  return (
    <TimeSeriesChart
      testId="site-headroom-chart"
      data={chartData}
      tMin={tMin}
      tMax={tMax}
      xAxisTickFormatter={formatTs}
      xAxisTicks={xAxisTicks}
      axes={[
        { id: "power", width: 46, domain, tickFormatter: formatPowerTick, ticks: zeroAnchoredTicks(domain) },
      ]}
      series={series}
      bands={[
        {
          key: "Headroom [kW]",
          axisId: "power",
          lower: (row) =>
            row.values?.["gridPowerKw"] != null && row.values?.["upKw"] != null
              ? row.values["gridPowerKw"] - row.values["upKw"]
              : null,
          upper: (row) =>
            row.values?.["gridPowerKw"] != null && row.values?.["downKw"] != null
              ? row.values["gridPowerKw"] + row.values["downKw"]
              : null,
          color: "#8BC34A",
          formatter: (lower, upper) => `${formatPowerValue(lower)} – ${formatPowerValue(upper)}`,
        },
      ]}
      nowMs={nowMs}
      referenceAxisId="power"
      height={height ?? CELL_CHART_HEIGHT}
      margin={{ top: 4, right: 40, left: 0, bottom: 0 }}
    />
  );
}
