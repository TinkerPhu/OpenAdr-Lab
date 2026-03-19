import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer } from "recharts";
import type { PlannedRates } from "../../api/types";
import { CHART_COLORS } from "./colors";

interface TariffsLineChartProps {
  data: PlannedRates;
}

export function TariffsLineChart({ data }: TariffsLineChartProps) {
  if (data.length === 0) {
    return <div data-testid="tariffs-line-chart">No tariff data</div>;
  }

  const points = data.map((snap) => ({
    ts: new Date(snap.interval_start).getTime(),
    import_price_eur_kwh: snap.import_price_eur_kwh,
    export_price_eur_kwh: snap.export_price_eur_kwh,
    co2_g_kwh: snap.co2_g_kwh,
  }));

  return (
    <div data-testid="tariffs-line-chart">
    <ResponsiveContainer width="100%" height={260}>
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
          formatter={(v: number | null, name: string) =>
            v !== null ? [`${v.toFixed(4)}`, name] : ["—", name]
          }
        />
        <Legend />
        <Line
          type="stepAfter"
          dataKey="import_price_eur_kwh"
          stroke={CHART_COLORS[0]}
          dot={false}
          connectNulls={false}
          name="import €/kWh"
        />
        <Line
          type="stepAfter"
          dataKey="export_price_eur_kwh"
          stroke={CHART_COLORS[1]}
          dot={false}
          connectNulls={false}
          name="export €/kWh"
        />
        <Line
          type="stepAfter"
          dataKey="co2_g_kwh"
          stroke={CHART_COLORS[2]}
          dot={false}
          connectNulls={false}
          name="CO₂ g/kWh"
        />
      </LineChart>
    </ResponsiveContainer>
    </div>
  );
}
