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
import { formatTariffEurKwh, formatCo2IntensityGKwh } from "./unitFormat";
import { niceAxis, tickFormatterForStep } from "./axisDomain";

type CurvePoint = { fillPct: number; bidEurKwh: number; co2GKwh: number };

interface CurveSeriesConfig {
  dataKey: "bidEurKwh" | "co2GKwh";
  label: string;
  color: string;
  yAxisId: "price" | "co2";
  formatValue: (v: number) => string;
}

/** BL-17: price and CO2 bids are unrelated units/magnitudes, so each gets its own
 * Y-axis (price left, CO2 right) rather than sharing one scale. */
const DEFAULT_SERIES: CurveSeriesConfig[] = [
  {
    dataKey: "bidEurKwh",
    label: "Max bid",
    color: "#2196F3",
    yAxisId: "price",
    formatValue: formatTariffEurKwh,
  },
  {
    dataKey: "co2GKwh",
    label: "Max CO2 bid",
    color: "#4CAF50",
    yAxisId: "co2",
    formatValue: formatCo2IntensityGKwh,
  },
];

interface CurveChartProps {
  rows: ComfortRate[];
  /** Declares which curve axes to render (data-driven per `declare-dont-branch`) —
   * defaults to both price and CO2. */
  series?: CurveSeriesConfig[];
}

/**
 * Non-temporal-X-axis composition (design.md's 3rd taxonomy member, alongside
 * TimeSeriesChart and StackedTimeSeriesChart) — shares only sizing/empty-state/
 * unit-formatting primitives with the other two, since a fill%-vs-bid curve has no
 * time domain, NOW line, or zone shading to share. One real consumer (the
 * comfort-curve editor preview); generalized to a configurable list of Y-series
 * (BL-17 added the CO2 bid alongside the original price bid) rather than adding a
 * second bespoke chart component for the same (fill %, bid) shape.
 *
 * Live preview of the comfort curve being edited in `ComfortCurveCard` — plotted in
 * fill order so the shape of the curve (typically: pay more to reach a low fill
 * fast, less once "enough" is already banked) is visible at a glance, not just as a
 * row of numbers.
 */
export function CurveChart({ rows, series = DEFAULT_SERIES }: CurveChartProps) {
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
    .map((r) => ({
      fillPct: Math.round(r.fill * 100),
      bidEurKwh: r.max_marginal_price,
      co2GKwh: r.max_marginal_co2,
    }))
    .sort((a, b) => a.fillPct - b.fillPct);

  // Same Y-tick rule as the time-series compositions (niceAxis) — this chart owns its own
  // <YAxis> pair. Both bids are strictly-positive prices anchored at 0, which `[0, "auto"]`
  // expressed before but left recharts to tick over an unrounded auto-max.
  const priceAxis = niceAxis([0, Math.max(...data.map((d) => d.bidEurKwh), 0)]);
  const co2Axis = niceAxis([0, Math.max(...data.map((d) => d.co2GKwh), 0)]);

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
            yAxisId="price"
            domain={priceAxis.domain}
            ticks={priceAxis.ticks}
            tickFormatter={tickFormatterForStep(priceAxis.step)}
            tick={{ fontSize: 11 }}
            width={48}
          />
          <YAxis
            yAxisId="co2"
            orientation="right"
            domain={co2Axis.domain}
            ticks={co2Axis.ticks}
            tickFormatter={tickFormatterForStep(co2Axis.step)}
            tick={{ fontSize: 11 }}
            width={48}
          />
          <Tooltip
            formatter={(value: number, name: string) => {
              const s = series.find((s) => s.dataKey === name);
              return s ? [s.formatValue(value), s.label] : [value, name];
            }}
            labelFormatter={(fillPct: number) => `Fill: ${fillPct}%`}
          />
          {series.map((s) => (
            <Line
              key={s.dataKey}
              yAxisId={s.yAxisId}
              type="linear"
              dataKey={s.dataKey}
              name={s.dataKey}
              stroke={s.color}
              strokeWidth={2}
              dot={{ r: 4 }}
              isAnimationActive={false}
            />
          ))}
        </LineChart>
      </ResponsiveContainer>
    </Box>
  );
}
