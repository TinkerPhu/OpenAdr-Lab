import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import { Box, Typography } from "@mui/material";
import type { ComfortRate } from "../../api/types";
import { CELL_CHART_HEIGHT } from "../controller/chartLayout";

interface ComfortCurveChartProps {
  rows: ComfortRate[];
  color?: string;
}

type CurvePoint = { fillPct: number; bidEurKwh: number };

/** Live preview of the (fill %, bid €/kWh) willingness-to-pay curve being
 *  edited in `ComfortCurveCard` — plotted in fill order so the shape of the
 *  curve (typically: pay more to reach a low fill fast, less once "enough"
 *  is already banked) is visible at a glance, not just as a row of numbers. */
export function ComfortCurveChart({ rows, color = "#2196F3" }: ComfortCurveChartProps) {
  if (rows.length === 0) {
    return (
      <Box
        data-testid="comfort-curve-chart-empty"
        sx={{
          height: CELL_CHART_HEIGHT,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <Typography color="text.secondary" variant="body2">
          Add points to preview the curve
        </Typography>
      </Box>
    );
  }

  const data: CurvePoint[] = rows
    .map((r) => ({ fillPct: Math.round(r.fill * 100), bidEurKwh: r.max_marginal_price }))
    .sort((a, b) => a.fillPct - b.fillPct);

  return (
    <Box data-testid="comfort-curve-chart">
      <ResponsiveContainer width="100%" height={CELL_CHART_HEIGHT}>
        <LineChart data={data} margin={{ top: 4, right: 12, left: 0, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
          <XAxis
            dataKey="fillPct"
            type="number"
            domain={[0, 100]}
            unit="%"
            tick={{ fontSize: 11 }}
          />
          <YAxis
            dataKey="bidEurKwh"
            domain={[0, "auto"]}
            tick={{ fontSize: 11 }}
            width={48}
          />
          <Tooltip
            formatter={(value: number, name: string) =>
              name === "bidEurKwh" ? [`€${value.toFixed(3)}/kWh`, "Max bid"] : [value, name]
            }
            labelFormatter={(fillPct: number) => `Fill: ${fillPct}%`}
          />
          <Line
            type="linear"
            dataKey="bidEurKwh"
            stroke={color}
            strokeWidth={2}
            dot={{ r: 4 }}
            isAnimationActive={false}
          />
        </LineChart>
      </ResponsiveContainer>
    </Box>
  );
}
