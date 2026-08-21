import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from "recharts";
import type { SimSnapshot } from "../../api/types";
import { SERIES_COLORS } from "../controller/types";
import { minSpanDomain, MIN_POWER_SPAN_KW, formatPowerTick, niceAxis } from "../charts/axisDomain";
import { formatPowerValue } from "../charts/unitFormat";
import { DIAGNOSTIC_CHART_HEIGHT } from "../charts/chartLayout";

interface SimProfileChartProps {
  data: SimSnapshot;
}

export function SimProfileChart({ data }: SimProfileChartProps) {
  const points: { name: string; power_kw: number }[] = [
    { name: "grid", power_kw: data.grid.net_power_w / 1000 },
    ...Object.entries(data.assets).map(([id, snap]) => ({
      name: id,
      power_kw: snap.power_kw,
    })),
  ];
  const powerDomain = minSpanDomain(
    points.map((p) => p.power_kw),
    MIN_POWER_SPAN_KW
  );
  const powerAxis = niceAxis(powerDomain);

  return (
    <div data-testid="sim-profile-chart">
    <ResponsiveContainer width="100%" height={DIAGNOSTIC_CHART_HEIGHT}>
      <LineChart data={points} margin={{ top: 4, right: 16, left: 0, bottom: 4 }}>
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis dataKey="name" />
        <YAxis tickFormatter={formatPowerTick} domain={powerAxis.domain} ticks={powerAxis.ticks} />
        <Tooltip formatter={(v: number) => formatPowerValue(v)} />
        <Line
          type="monotone"
          dataKey="power_kw"
          stroke={SERIES_COLORS.power}
          dot={true}
          connectNulls={false}
          name="power_kw"
          isAnimationActive={false}
        />
      </LineChart>
    </ResponsiveContainer>
    </div>
  );
}
