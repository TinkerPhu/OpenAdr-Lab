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
  formatPowerTick,
} from "../../charts/axisDomain";
import { formatSignedPowerValue } from "../../charts/unitFormat";
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
 * BL-43 / `unified-capacity-envelope-engine` (Spec E): live site-level flexibility
 * plotted as a band alongside the grid-power line. `up_kw`/`down_kw` are now ABSOLUTE
 * achievable power (each asset's own `max_effort_setpoint`, summed) rather than a delta
 * from the currently-planned dispatch, so the band is anchored to those absolute limits
 * directly — `-up_kw` (max achievable Export, negative-signed) to `down_kw` (max
 * achievable Import, positive-signed) — not to the grid-power line the way it was before
 * this change. The grid-power line itself is unchanged, still shown for reference.
 * Distinct from `TariffEnvelopeChart`'s Dynamic Operating Envelope
 * (`IMPORT/EXPORT_CAPACITY_LIMIT`), which is a VTN-announced forward *schedule*, not a
 * live/forecast headroom value.
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
  // upKw/downKw hold the ABSOLUTE max-export/max-import limits directly now
  // (design.md D5/D6) -- no longer combined with gridPowerKw to form the band.
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
  // LOCF bridges minor timestamp misalignment between the headroom samples
  // (history/forecast) and the coarser-resolution grid timeline. gridPowerKw
  // is filled too so the line renders without gaps at the merged timestamps
  // the headroom samples introduce -- the band itself no longer depends on
  // gridPowerKw being present on the same row (it reads upKw/downKw alone).
  const filled = locfFillKeys(merged, ["upKw", "downKw", "gridPowerKw"]);
  const clipped = clipRowsToWindow(filled, tMin, tMax);
  const chartData = ensureNonEmptyRows(clipped, tMin, tMax);

  const domain = minSpanDomain(
    chartData.flatMap((row) => [
      row.values?.gridPowerKw,
      row.values?.upKw != null ? -row.values.upKw : null,
      row.values?.downKw ?? null,
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
        { id: "power", width: 46, domain, tickFormatter: formatPowerTick },
      ]}
      series={series}
      bands={[
        {
          key: "Achievable range [kW]",
          axisId: "power",
          lower: (row) => (row.values?.["upKw"] != null ? -row.values["upKw"] : null),
          upper: (row) => row.values?.["downKw"] ?? null,
          color: "#4CAF50",
          fillOpacity: 0.35,
          formatter: (lower, upper) =>
            `${formatSignedPowerValue(lower)} – ${formatSignedPowerValue(upper)}`,
        },
      ]}
      nowMs={nowMs}
      referenceAxisId="power"
      height={height ?? CELL_CHART_HEIGHT}
      // No right-side axis here (single "power" axis, left only), but this chart is
      // stacked in the same column as TariffEnvelopeChart/GridRatesChart/AssetTimelineChart,
      // which all reserve ~88-92px on the right for a second axis. Matching that reserve via
      // margin.right (instead of the generic default of 40) keeps every stacked chart's plot
      // area — and therefore its X-axis ticks/gridlines — the same width and aligned.
      margin={{ top: 4, right: 88, left: 0, bottom: 0 }}
    />
  );
}
