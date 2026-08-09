import type { PlannedRates } from "../../api/types";
import { SERIES_COLORS } from "../controller/types";
import { DIAGNOSTIC_CHART_HEIGHT } from "../charts/chartLayout";
import { formatTariffEurKwh, formatCo2IntensityGKwh } from "../charts/unitFormat";
import { EmptyState } from "../charts/EmptyState";
import { tightSpanDomain, MIN_TARIFF_SPAN_EUR_KWH, MIN_CO2_INTENSITY_SPAN_G_KWH } from "../charts/axisDomain";
import { TimeSeriesChart, type TimeSeriesSeriesSpec } from "../charts/TimeSeriesChart";
import type { TimestampedRow } from "../charts/mergeSeries";

interface TariffsLineChartProps {
  data: PlannedRates;
}

export function TariffsLineChart({ data }: TariffsLineChartProps) {
  if (data.length === 0) {
    return (
      <EmptyState
        testId="tariffs-line-chart"
        message="No tariff data"
        height={DIAGNOSTIC_CHART_HEIGHT}
      />
    );
  }

  const points: TimestampedRow[] = data.map((snap) => ({
    ts: new Date(snap.interval_start).getTime(),
    values: {
      import_tariff_eur_kwh: snap.import_tariff_eur_kwh,
      export_tariff_eur_kwh: snap.export_tariff_eur_kwh,
      co2_g_kwh: snap.co2_g_kwh,
    },
  }));

  // Tariff (€/kWh) and CO2 intensity (g/kWh) are different physical dimensions — must not
  // share a Y-axis, same reasoning as TariffChart's tariff/cost split.
  const tariffDomain = tightSpanDomain(
    points.flatMap((p) => [p.values?.import_tariff_eur_kwh, p.values?.export_tariff_eur_kwh]),
    MIN_TARIFF_SPAN_EUR_KWH
  );
  const co2Domain = tightSpanDomain(
    points.map((p) => p.values?.co2_g_kwh),
    MIN_CO2_INTENSITY_SPAN_G_KWH
  );

  const series: TimeSeriesSeriesSpec[] = [
    {
      key: "import €/kWh",
      axisId: "tariff",
      dataKey: (row) => row.values?.import_tariff_eur_kwh ?? null,
      color: SERIES_COLORS.import_tariff,
      formatter: formatTariffEurKwh,
    },
    {
      key: "export €/kWh",
      axisId: "tariff",
      dataKey: (row) => row.values?.export_tariff_eur_kwh ?? null,
      color: SERIES_COLORS.export_tariff,
      formatter: formatTariffEurKwh,
    },
    {
      key: "CO₂ g/kWh",
      axisId: "co2",
      dataKey: (row) => row.values?.co2_g_kwh ?? null,
      color: SERIES_COLORS.co2_rate,
      formatter: formatCo2IntensityGKwh,
    },
  ];

  return (
    <TimeSeriesChart
      testId="tariffs-line-chart"
      data={points}
      xAxisTickFormatter={(v) => new Date(v).toLocaleTimeString()}
      axes={[
        { id: "tariff", unit: " €/kWh", domain: tariffDomain },
        { id: "co2", orientation: "right", unit: " g/kWh", domain: co2Domain },
      ]}
      series={series}
      height={DIAGNOSTIC_CHART_HEIGHT}
      margin={{ top: 4, right: 16, left: 0, bottom: 4 }}
    />
  );
}
