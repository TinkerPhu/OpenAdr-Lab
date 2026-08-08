import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import { Box } from "@mui/material";
import type { ComfortRate } from "../../api/types";
import { CELL_CHART_HEIGHT } from "./chartLayout";
import { EmptyState } from "./EmptyState";
import { formatTariffEurKwh } from "./unitFormat";

interface CurveChartProps {
  rows: ComfortRate[];
  color?: string;
}

type CurvePoint = { fillPct: number; bidEurKwh: number };

/**
 * Non-temporal-X-axis composition (design.md's 3rd taxonomy member, alongside
 * TimeSeriesChart and StackedTimeSeriesChart) — shares only sizing/empty-state/
 * unit-formatting primitives with the other two, since a fill%-vs-price curve has no
 * time domain, NOW line, or zone shading to share. Currently has one real consumer
 * (the comfort-curve editor preview); kept scoped to that exact (fill%, €/kWh) shape
 * rather than generalized further, since there's no second shape yet to generalize for.
 *
 * Live preview of the (fill %, bid €/kWh) willingness-to-pay curve being edited in
 * `ComfortCurveCard` — plotted in fill order so the shape of the curve (typically: pay
 * more to reach a low fill fast, less once "enough" is already banked) is visible at a
 * glance, not just as a row of numbers.
 */
export function CurveChart({ rows, color = "#2196F3" }: CurveChartProps) {
  if (rows.length === 0) {
    return (
      <EmptyState
        testId="comfort-curve-chart-empty"
        message="Add points to preview the curve"
        height={CELL_CHART_HEIGHT}
      />
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
              name === "bidEurKwh" ? [formatTariffEurKwh(value), "Max bid"] : [value, name]
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
