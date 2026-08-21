import type { TariffTimePoint } from "../types";
import { SERIES_COLORS } from "../types";
import type { ZoneDef } from "../../../api/types";
import {
  minSpanDomain,
  tightSpanDomain,
  MIN_POWER_SPAN_KW,
  MIN_TARIFF_SPAN_EUR_KWH,
  roundedTimeTicks,
} from "../../charts/axisDomain";
import { formatTariffEurKwh, formatPowerValue } from "../../charts/unitFormat";
import { CELL_CHART_HEIGHT } from "../../charts/chartLayout";
import { TimeSeriesChart, type TimeSeriesSeriesSpec } from "../../charts/TimeSeriesChart";
import { clipToWindow, carryForwardLastKnown, ensureNonEmpty, formatTs } from "./tariffChartShared";

interface TariffEnvelopeChartProps {
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
 * Direct VTN signals: tariff (PRICE/EXPORT_PRICE) and the Dynamic Operating Envelope
 * (IMPORT_CAPACITY_LIMIT/EXPORT_CAPACITY_LIMIT, OpenADR 3.1 User Guide §8.10.1) — both
 * announced by the VTN as-is, unlike GridRatesChart's cost/CO2 rate which the VEN derives
 * by multiplying tariff × power.
 */
export function TariffEnvelopeChart({
  data,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  height,
  zones,
  xAxisTickIntervalMinutes,
}: TariffEnvelopeChartProps) {
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;
  const xAxisTicks = xAxisTickIntervalMinutes
    ? roundedTimeTicks(tMin, tMax, xAxisTickIntervalMinutes)
    : undefined;

  const windowed = clipToWindow(data, tMin, tMax);
  const withTariffCarry = carryForwardLastKnown(windowed, tMax, [
    "importPriceEurKwh",
    "exportPriceEurKwh",
  ]);
  const clipped = carryForwardLastKnown(withTariffCarry, tMax, [
    "importLimitKw",
    "exportLimitKw",
  ]);

  // Tariff (€/kWh) and capacity limit (kW) are different physical dimensions and must not
  // share a Y-axis — see TariffChart's original tariff/cost-rate split for the same
  // rationale. tightSpanDomain for tariff (strictly-positive price, no meaningful 0
  // baseline); minSpanDomain for the capacity limit (0 kW — fully curtailed — is a
  // meaningful baseline worth always showing, like a rate axis).
  const tariffDomain = tightSpanDomain(
    clipped.flatMap((p) => [p.values?.importPriceEurKwh, p.values?.exportPriceEurKwh]),
    MIN_TARIFF_SPAN_EUR_KWH
  );
  const capacityDomain = minSpanDomain(
    clipped.flatMap((p) => [p.values?.importLimitKw, p.values?.exportLimitKw]),
    MIN_POWER_SPAN_KW
  );

  const chartData = ensureNonEmpty(clipped, tMin, tMax);

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
      key: "Import limit [kW]",
      axisId: "capacity",
      dataKey: (row) => row.values?.importLimitKw ?? null,
      color: SERIES_COLORS.import_capacity_limit,
      strokeDasharray: "3 3",
      connectNulls: true,
      formatter: formatPowerValue,
    },
    {
      key: "Export limit [kW]",
      axisId: "capacity",
      dataKey: (row) => row.values?.exportLimitKw ?? null,
      color: SERIES_COLORS.export_capacity_limit,
      strokeDasharray: "3 3",
      connectNulls: true,
      formatter: formatPowerValue,
    },
  ];

  return (
    <TimeSeriesChart
      testId="tariff-envelope-chart"
      data={chartData}
      tMin={tMin}
      tMax={tMax}
      xAxisTickFormatter={formatTs}
      xAxisTicks={xAxisTicks}
      axes={[
        { id: "tariff", unit: " €/kWh", width: 48, domain: tariffDomain },
        { id: "capacity", orientation: "right", unit: " kW", width: 48, domain: capacityDomain },
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
