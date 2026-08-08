import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer } from "recharts";
import type { PlannedRates } from "../../api/types";
import { SERIES_COLORS } from "../controller/types";
import { DIAGNOSTIC_CHART_HEIGHT } from "../charts/chartLayout";
import { formatTariffEurKwh, formatCo2IntensityGKwh } from "../charts/unitFormat";
import { EmptyState } from "../charts/EmptyState";

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

  const points = data.map((snap) => ({
    ts: new Date(snap.interval_start).getTime(),
    import_tariff_eur_kwh: snap.import_tariff_eur_kwh,
    export_tariff_eur_kwh: snap.export_tariff_eur_kwh,
    co2_g_kwh: snap.co2_g_kwh,
  }));

  return (
    <div data-testid="tariffs-line-chart">
    <ResponsiveContainer width="100%" height={DIAGNOSTIC_CHART_HEIGHT}>
      <LineChart data={points} margin={{ top: 4, right: 16, left: 0, bottom: 4 }}>
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis
          dataKey="ts"
          scale="time"
          type="number"
          domain={["auto", "auto"]}
          tickFormatter={(v: number) => new Date(v).toLocaleTimeString()}
        />
        <YAxis />
        <Tooltip
          labelFormatter={(v: number) => new Date(v).toLocaleString()}
          formatter={(v, name) => {
            if (typeof v !== "number") return ["—", String(name)];
            if (name === "CO₂ g/kWh") return [formatCo2IntensityGKwh(v), String(name)];
            return [formatTariffEurKwh(v), String(name)];
          }}
        />
        <Legend />
        <Line
          type="stepAfter"
          dataKey="import_tariff_eur_kwh"
          stroke={SERIES_COLORS.import_tariff}
          dot={false}
          connectNulls={false}
          name="import €/kWh"
          isAnimationActive={false}
        />
        <Line
          type="stepAfter"
          dataKey="export_tariff_eur_kwh"
          stroke={SERIES_COLORS.export_tariff}
          dot={false}
          connectNulls={false}
          name="export €/kWh"
          isAnimationActive={false}
        />
        <Line
          type="stepAfter"
          dataKey="co2_g_kwh"
          stroke={SERIES_COLORS.co2_rate}
          dot={false}
          connectNulls={false}
          name="CO₂ g/kWh"
          isAnimationActive={false}
        />
      </LineChart>
    </ResponsiveContainer>
    </div>
  );
}
