import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from "recharts";
import type { SimSnapshot } from "../../api/types";
import { CHART_COLORS } from "./colors";

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

  return (
    <div data-testid="sim-profile-chart">
    <ResponsiveContainer width="100%" height={260}>
      <LineChart data={points} margin={{ top: 4, right: 16, left: 0, bottom: 4 }}>
        <CartesianGrid strokeDasharray="3 3" />
        <XAxis dataKey="name" />
        <YAxis unit=" kW" />
        <Tooltip formatter={(v: number) => `${v.toFixed(3)} kW`} />
        <Line
          type="monotone"
          dataKey="power_kw"
          stroke={CHART_COLORS[0]}
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
