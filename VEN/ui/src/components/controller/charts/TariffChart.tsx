import type { TariffTimePoint } from "../types";
import { SERIES_COLORS } from "../types";
import type { ZoneDef } from "../../../api/types";
import {
  minSpanDomain,
  tightSpanDomain,
  MIN_CO2_RATE_SPAN_G_H,
  MIN_COST_RATE_SPAN_EUR_H,
  MIN_TARIFF_SPAN_EUR_KWH,
  roundedTimeTicks,
  zeroAnchoredTicks,
} from "../../charts/axisDomain";
import { formatCo2RateGH, formatCostRateEurH, formatTariffEurKwh } from "../../charts/unitFormat";
import { CELL_CHART_HEIGHT } from "../../charts/chartLayout";
import { TimeSeriesChart, type TimeSeriesSeriesSpec } from "../../charts/TimeSeriesChart";
import type { TimestampedRow } from "../../charts/mergeSeries";

interface TariffChartProps {
  data: TariffTimePoint[];
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  height?: number;
  zones?: ZoneDef[];
  /** X-axis ticks every N minutes, snapped to the wall-clock (10:00, 10:30, ...) instead of
   * recharts' default "nice" ticks. History page only — Controller's real-time cells keep the
   * default behavior. */
  xAxisTickIntervalMinutes?: number;
}

function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function toRow(p: TariffTimePoint): TimestampedRow {
  return {
    ts: p.ts,
    values: {
      importPriceEurKwh: p.importPriceEurKwh,
      exportPriceEurKwh: p.exportPriceEurKwh,
      co2GKwh: p.co2GKwh,
      totalCostRateEurH: p.totalCostRateEurH,
      totalCo2RateGH: p.totalCo2RateGH,
      gridPowerKw: p.gridPowerKw,
    },
  };
}

export function TariffChart({
  data,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  height,
  zones,
  xAxisTickIntervalMinutes,
}: TariffChartProps) {
  // Domain driven by hoursBack/hoursForward keeps the X-axis stable and ensures the
  // NOW reference line is always visible even when past tariff data is absent.
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;
  const xAxisTicks = xAxisTickIntervalMinutes
    ? roundedTimeTicks(tMin, tMax, xAxisTickIntervalMinutes)
    : undefined;

  // Clip data to [tMin, tMax]. recharts does not clip rendered data to the XAxis domain —
  // without this the chart auto-scales to the full data extent (e.g. 6×24h from /tariffs).
  // Keep the last point before tMin as a left anchor so stepAfter lines start at the
  // correct value at the left edge of the window.
  const clipped: TimestampedRow[] = (() => {
    const rows = data.map(toRow);
    const upToEnd = rows.filter((p) => p.ts <= tMax);
    // Pin the left-anchor to tMin so recharts never sees a data point outside
    // [tMin, tMax]. Without this, recharts expands the X-axis domain to fit the
    // anchor's original timestamp, shifting the NOW line rightward relative to
    // all other charts that share the same domain.
    const lastBefore = upToEnd.filter((p) => p.ts < tMin).slice(-1)
      .map((p) => ({ ...p, ts: tMin }));
    const inWindow = upToEnd.filter((p) => p.ts >= tMin);
    const windowed = [...lastBefore, ...inWindow];

    // Carry-forward the last known tariff prices to tMax. The merged dataset contains
    // power points (gridTimeline) with null tariff fields after the last tariff snapshot.
    // connectNulls=false stops the stepAfter line at the last non-null value rather than
    // extending to the right edge — a sentinel at tMax prevents this gap.
    const lastTariff = [...windowed].reverse().find(
      (p) => p.values?.importPriceEurKwh != null || p.values?.exportPriceEurKwh != null || p.values?.co2GKwh != null
    );
    if (lastTariff) {
      windowed.push({
        ts: tMax,
        values: {
          importPriceEurKwh: lastTariff.values?.importPriceEurKwh ?? null,
          exportPriceEurKwh: lastTariff.values?.exportPriceEurKwh ?? null,
          co2GKwh: lastTariff.values?.co2GKwh ?? null,
          totalCostRateEurH: null,
          totalCo2RateGH: null,
          gridPowerKw: null,
        },
      });
    }

    return windowed;
  })();

  const co2Domain = minSpanDomain(
    clipped.map((p) => p.values?.totalCo2RateGH ?? null),
    MIN_CO2_RATE_SPAN_G_H
  );
  // Tariff (€/kWh) and cost rate (€/h) are different physical dimensions and must not
  // share a Y-axis — plotting them together previously let cost rate's range flatten the
  // tariff curves whenever the two magnitudes diverged. tightSpanDomain (not minSpanDomain)
  // — tariff is a strictly-positive price, so anchoring the domain at 0 would still
  // compress a narrow real range (e.g. 0.28-0.32) into a sliver of the axis, undoing the
  // point of splitting the axis out in the first place.
  const tariffDomain = tightSpanDomain(
    clipped.flatMap((p) => [p.values?.importPriceEurKwh, p.values?.exportPriceEurKwh]),
    MIN_TARIFF_SPAN_EUR_KWH
  );
  const costDomain = minSpanDomain(
    clipped.map((p) => p.values?.totalCostRateEurH ?? null),
    MIN_COST_RATE_SPAN_EUR_H
  );

  // Ensure at least a 2-point range so recharts can render the NOW line when data is empty.
  const chartData: TimestampedRow[] =
    clipped.length > 0
      ? clipped
      : [
          { ts: tMin, values: {} },
          { ts: tMax, values: {} },
        ];

  const series: TimeSeriesSeriesSpec[] = [
    {
      key: "Import tariff [€/kWh]",
      axisId: "tariff",
      dataKey: (row) => row.values?.importPriceEurKwh ?? null,
      color: SERIES_COLORS.import_tariff,
      strokeDasharray: "5 5",
      connectNulls: true,
      formatter: formatTariffEurKwh,
    },
    {
      key: "Export tariff [€/kWh]",
      axisId: "tariff",
      dataKey: (row) => row.values?.exportPriceEurKwh ?? null,
      color: SERIES_COLORS.export_tariff,
      strokeDasharray: "5 5",
      connectNulls: true,
      formatter: formatTariffEurKwh,
    },
    {
      key: "Cost rate [€/h]",
      axisId: "cost",
      dataKey: (row) => row.values?.totalCostRateEurH ?? null,
      color: SERIES_COLORS.cost_rate,
      strokeDasharray: "5 5",
      connectNulls: true,
      formatter: formatCostRateEurH,
    },
    {
      key: "CO₂ rate [g/h]",
      axisId: "co2",
      dataKey: (row) => row.values?.totalCo2RateGH ?? null,
      color: SERIES_COLORS.co2_rate,
      strokeDasharray: "2 2",
      connectNulls: true,
      formatter: formatCo2RateGH,
    },
  ];

  return (
    <TimeSeriesChart
      testId="tariff-chart"
      data={chartData}
      tMin={tMin}
      tMax={tMax}
      xAxisTickFormatter={formatTs}
      xAxisTicks={xAxisTicks}
      axes={[
        { id: "tariff", unit: " €/kWh", width: 48, domain: tariffDomain, ticks: zeroAnchoredTicks(tariffDomain) },
        { id: "cost", orientation: "right", unit: " €/h", width: 48, domain: costDomain, ticks: zeroAnchoredTicks(costDomain) },
        { id: "co2", orientation: "right", unit: " g/h", width: 52, domain: co2Domain, ticks: zeroAnchoredTicks(co2Domain) },
      ]}
      series={series}
      nowMs={nowMs}
      referenceAxisId="tariff"
      zones={zones}
      interactiveLegend
      height={height ?? CELL_CHART_HEIGHT}
      margin={{ top: 4, right: 40, left: 0, bottom: 0 }}
    />
  );
}
