import type { TariffTimePoint } from "../types";
import { SERIES_COLORS } from "../types";
import type { ZoneDef } from "../../../api/types";
import {
  minSpanDomain,
  MIN_CO2_RATE_SPAN_G_H,
  MIN_COST_RATE_SPAN_EUR_H,
  roundedTimeTicks,
} from "../../charts/axisDomain";
import { formatCo2RateGH, formatCostRateEurH } from "../../charts/unitFormat";
import { CELL_CHART_HEIGHT } from "../../charts/chartLayout";
import { TimeSeriesChart, type TimeSeriesSeriesSpec } from "../../charts/TimeSeriesChart";
import { clipToWindow, ensureNonEmpty, formatTs } from "./tariffChartShared";

interface GridRatesChartProps {
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

/**
 * Derived signals: cost rate and CO₂ rate, both computed by the VEN as
 * (tariff × grid power) — not announced by the VTN directly, unlike
 * TariffEnvelopeChart's tariff/capacity-limit series. Split out from the combined
 * tariff+rates chart so direct VTN signals and VEN-derived ones don't share a diagram.
 */
export function GridRatesChart({
  data,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  height,
  zones,
  xAxisTickIntervalMinutes,
}: GridRatesChartProps) {
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;
  const xAxisTicks = xAxisTickIntervalMinutes
    ? roundedTimeTicks(tMin, tMax, xAxisTickIntervalMinutes)
    : undefined;

  const clipped = clipToWindow(data, tMin, tMax);

  const co2Domain = minSpanDomain(
    clipped.map((p) => p.values?.totalCo2RateGH ?? null),
    MIN_CO2_RATE_SPAN_G_H
  );
  const costDomain = minSpanDomain(
    clipped.map((p) => p.values?.totalCostRateEurH ?? null),
    MIN_COST_RATE_SPAN_EUR_H
  );

  const chartData = ensureNonEmpty(clipped, tMin, tMax);

  const series: TimeSeriesSeriesSpec[] = [
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
      testId="grid-rates-chart"
      data={chartData}
      tMin={tMin}
      tMax={tMax}
      xAxisTickFormatter={formatTs}
      xAxisTicks={xAxisTicks}
      axes={[
        { id: "cost", unit: " €/h", width: 48, domain: costDomain },
        { id: "co2", orientation: "right", unit: " g/h", width: 52, domain: co2Domain },
      ]}
      series={series}
      nowMs={nowMs}
      referenceAxisId="cost"
      zones={zones}
      interactiveLegend
      height={height ?? CELL_CHART_HEIGHT}
      margin={{ top: 4, right: 40, left: 0, bottom: 0 }}
    />
  );
}
