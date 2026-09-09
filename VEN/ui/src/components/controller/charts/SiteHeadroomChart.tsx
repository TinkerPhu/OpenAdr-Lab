import type { AssetTimelinePoint } from "../types";
import type {
  CapacityCurvesResponse,
  SiteFlexibilitySample,
  SiteFlexibilityForecastSlot,
} from "../../../api/types";
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
  /** `GET /flexibility/capacity` — sustained-commitment curves, `null` before the first
   * dispatcher tick or wherever unavailable; optional so this component still works
   * without it. See this component's own doc comment for how these curves relate to
   * the achievable-range band. */
  capacity?: CapacityCurvesResponse | null;
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
 *
 * Also overlays the sustained-commitment capacity curves (`GET /flexibility/capacity`,
 * `controller::capacity_headroom::compute_site_capacity_curve`) as dashed step-lines,
 * starting exactly at `now` with no backward extension (that endpoint's `t1` is always
 * "now" — there is no meaningful past value for it, unlike the band's own history).
 * These curves answer a genuinely different question than the band: the band is a
 * per-instant snapshot ("if the plan's own trajectory holds to this future moment, what
 * could each asset do right then"), while the capacity curve is a single continuous
 * full-effort commitment starting now (e.g. a battery discharging non-stop). Because of
 * that, **the capacity curve legitimately sitting inside (narrower than) the band, or an
 * Export curve swinging positive past the band's usual scale (a sustained Export
 * commitment can be pushed net-importing by base load — see `capacity_headroom.rs`'s own
 * `merge_events` doc), is normal, not a bug** — hence the distinct dashed styling here
 * rather than drawing them with the same visual weight as the band.
 */
export function SiteHeadroomChart({
  gridTimeline,
  history,
  forecast = [],
  capacity = null,
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
  // Sustained-commitment capacity curves -- same step-curve shape CapacityForecastChart
  // itself builds, starting exactly at `now` (the endpoint's own `t1`), no samples before
  // it, so LOCF naturally leaves every pre-`now` row without a value (see this
  // component's own doc comment for why that's correct, not a gap to fill).
  const capacityStartMs = capacity ? new Date(capacity.import.start).getTime() : null;
  const importCapSamples: NamedSample[] = capacity
    ? capacity.import.steps.map((s) => ({
        ts: capacityStartMs! + s.elapsed_s * 1000,
        key: "importCapKw",
        value: s.power_kw,
      }))
    : [];
  const exportCapSamples: NamedSample[] = capacity
    ? capacity.export.steps.map((s) => ({
        ts: capacityStartMs! + s.elapsed_s * 1000,
        key: "exportCapKw",
        value: s.power_kw,
      }))
    : [];

  const merged = mergeTimestampedSeries(gridRows, [
    ...upSamples,
    ...downSamples,
    ...forecastUpSamples,
    ...forecastDownSamples,
    ...importCapSamples,
    ...exportCapSamples,
  ]);
  // LOCF bridges minor timestamp misalignment between the headroom samples
  // (history/forecast) and the coarser-resolution grid timeline. gridPowerKw
  // is filled too so the line renders without gaps at the merged timestamps
  // the headroom samples introduce -- the band itself no longer depends on
  // gridPowerKw being present on the same row (it reads upKw/downKw alone).
  const filled = locfFillKeys(merged, [
    "upKw",
    "downKw",
    "gridPowerKw",
    "importCapKw",
    "exportCapKw",
  ]);
  const clipped = clipRowsToWindow(filled, tMin, tMax);
  const chartData = ensureNonEmptyRows(clipped, tMin, tMax);

  const domain = minSpanDomain(
    chartData.flatMap((row) => [
      row.values?.gridPowerKw,
      row.values?.upKw != null ? -row.values.upKw : null,
      row.values?.downKw ?? null,
      row.values?.importCapKw,
      row.values?.exportCapKw,
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
    {
      key: "Import commitment [kW]",
      axisId: "power",
      dataKey: (row) => row.values?.["importCapKw"] ?? null,
      color: "#D32F2F",
      type: "stepAfter",
      strokeDasharray: "4 3",
      connectNulls: true,
      formatter: formatPowerValue,
    },
    {
      key: "Export commitment [kW]",
      axisId: "power",
      dataKey: (row) => row.values?.["exportCapKw"] ?? null,
      color: "#2E7D32",
      type: "stepAfter",
      strokeDasharray: "4 3",
      connectNulls: true,
      formatter: formatPowerValue,
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
