/**
 * CurveChart — BL-17 multi-series generalization (price + CO2 bid, dual Y-axis)
 *
 * Mocks recharts' structural components (same pattern as TimeSeriesChart.test.tsx)
 * so we can capture props passed to <Line>/<YAxis>/<Tooltip> without a full recharts
 * render in jsdom.
 */
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactNode } from "react";

const { lines, yAxes, tooltips } = vi.hoisted(() => ({
  lines: [] as Array<Record<string, unknown>>,
  yAxes: [] as Array<Record<string, unknown>>,
  tooltips: [] as Array<Record<string, unknown>>,
}));

vi.mock("recharts", () => ({
  LineChart: ({ children }: { children: ReactNode }) => children,
  ResponsiveContainer: ({ children }: { children: ReactNode }) => children,
  CartesianGrid: () => null,
  XAxis: () => null,
  YAxis: (props: Record<string, unknown>) => {
    yAxes.push(props);
    return null;
  },
  Tooltip: (props: Record<string, unknown>) => {
    tooltips.push(props);
    return null;
  },
  Line: (props: Record<string, unknown>) => {
    lines.push(props);
    return null;
  },
}));

import { CurveChart } from "../components/charts/CurveChart";
import type { ComfortRate } from "../api/types";

const rows: ComfortRate[] = [
  { fill: 0.5, max_marginal_price: 0.4, max_marginal_co2: 300 },
  { fill: 1.0, max_marginal_price: 0.1, max_marginal_co2: 100 },
];

describe("CurveChart", () => {
  beforeEach(() => {
    lines.length = 0;
    yAxes.length = 0;
    tooltips.length = 0;
  });

  it("shows an empty state and no chart when there are no points", () => {
    render(<CurveChart rows={[]} />);
    expect(screen.getByTestId("comfort-curve-chart-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("comfort-curve-chart")).not.toBeInTheDocument();
  });

  it("renders one Line per series (price + CO2) on separate Y-axes", () => {
    render(<CurveChart rows={rows} />);
    expect(screen.getByTestId("comfort-curve-chart")).toBeInTheDocument();
    expect(lines).toHaveLength(2);
    const price = lines.find((l) => l.dataKey === "bidEurKwh");
    const co2 = lines.find((l) => l.dataKey === "co2GKwh");
    expect(price?.yAxisId).toBe("price");
    expect(co2?.yAxisId).toBe("co2");
    expect(yAxes.map((a) => a.yAxisId)).toEqual(["price", "co2"]);
  });

  it("formats each series' tooltip value in its own unit, keyed by series name", () => {
    render(<CurveChart rows={rows} />);
    const formatter = tooltips[0].formatter as (
      value: number,
      name: string,
    ) => [string, string];
    expect(formatter(0.4, "bidEurKwh")).toEqual(["0.4000 €/kWh", "Max bid"]);
    expect(formatter(300, "co2GKwh")).toEqual(["300.000 g/kWh", "Max CO2 bid"]);
  });

  it("a caller-supplied series list overrides the default price+CO2 pair", () => {
    render(
      <CurveChart
        rows={rows}
        series={[
          {
            dataKey: "bidEurKwh",
            label: "Price only",
            color: "#000",
            yAxisId: "price",
            formatValue: (v) => `${v}`,
          },
        ]}
      />,
    );
    expect(lines).toHaveLength(1);
    expect(lines[0].dataKey).toBe("bidEurKwh");
  });
});
