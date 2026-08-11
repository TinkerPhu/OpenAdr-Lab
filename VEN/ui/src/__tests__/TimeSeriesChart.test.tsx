/**
 * TimeSeriesChart — interactiveLegend behavior
 *
 * Mocks recharts' structural components (same pattern as TariffEnvelopeChart.test.tsx-style mocking) so we can
 * capture props passed to <Line>/<Legend> without needing a full recharts render in
 * jsdom. The mocked <Legend> actually renders its `content` prop (rather than returning
 * null like the other mocks) so ChartLegend's real checkboxes are in the DOM and
 * clickable — this is what lets us test the actual toggle interaction end-to-end.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactNode } from "react";

const { lines, legends, tooltips } = vi.hoisted(() => ({
  lines: [] as Array<Record<string, unknown>>,
  legends: [] as Array<Record<string, unknown>>,
  tooltips: [] as Array<Record<string, unknown>>,
}));

vi.mock("recharts", () => ({
  ComposedChart: ({ children }: { children: ReactNode }) => children,
  ResponsiveContainer: ({ children }: { children: ReactNode }) => children,
  CartesianGrid: () => null,
  XAxis: () => null,
  YAxis: () => null,
  Tooltip: (props: Record<string, unknown>) => {
    tooltips.push(props);
    return null;
  },
  ReferenceArea: () => null,
  ReferenceLine: () => null,
  Line: (props: Record<string, unknown>) => {
    lines.push(props);
    return null;
  },
  Legend: (props: Record<string, unknown> & { content?: ReactNode }) => {
    legends.push(props);
    return (props.content as ReactNode) ?? null;
  },
}));

import { TimeSeriesChart, type TimeSeriesSeriesSpec, type TimeSeriesAxisSpec } from "../components/charts/TimeSeriesChart";
import type { TimestampedRow } from "../components/charts/mergeSeries";

const data: TimestampedRow[] = [
  { ts: 1000, values: { power: 1, cost: 0.1 } },
  { ts: 2000, values: { power: 2, cost: 0.2 } },
];

const axes: TimeSeriesAxisSpec[] = [{ id: "power", domain: [0, 5] }];

const series: TimeSeriesSeriesSpec[] = [
  { key: "power", axisId: "power", dataKey: (r) => r.values?.power ?? null, color: "#2196F3" },
  { key: "cost", axisId: "power", dataKey: (r) => r.values?.cost ?? null, color: "#212121" },
];

describe("TimeSeriesChart — interactiveLegend", () => {
  beforeEach(() => {
    lines.length = 0;
    legends.length = 0;
    tooltips.length = 0;
  });

  it("without interactiveLegend, no series is ever hidden and no checkbox renders", () => {
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        tooltipFormatter={(v, n) => [String(v), n]}
      />
    );
    expect(lines.every((l) => l.hide === false)).toBe(true);
    expect(screen.queryByTestId("legend-toggle-power")).not.toBeInTheDocument();
  });

  it("with interactiveLegend, every series starts visible and checked", () => {
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        tooltipFormatter={(v, n) => [String(v), n]}
        interactiveLegend
      />
    );
    expect(screen.getByTestId("legend-toggle-power")).toBeChecked();
    expect(screen.getByTestId("legend-toggle-cost")).toBeChecked();
    expect(lines.every((l) => l.hide === false)).toBe(true);
  });

  it("unchecking a series' legend entry hides that series' Line", async () => {
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        tooltipFormatter={(v, n) => [String(v), n]}
        interactiveLegend
      />
    );
    await userEvent.click(screen.getByTestId("legend-toggle-cost"));

    const latestCostLine = [...lines].reverse().find((l) => l.name === "cost");
    const latestPowerLine = [...lines].reverse().find((l) => l.name === "power");
    expect(latestCostLine?.hide).toBe(true);
    expect(latestPowerLine?.hide).toBe(false);
  });

  it("re-checking a hidden series shows it again", async () => {
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        tooltipFormatter={(v, n) => [String(v), n]}
        interactiveLegend
      />
    );
    await userEvent.click(screen.getByTestId("legend-toggle-cost"));
    await userEvent.click(screen.getByTestId("legend-toggle-cost"));

    const latestCostLine = [...lines].reverse().find((l) => l.name === "cost");
    expect(latestCostLine?.hide).toBe(false);
  });
});

describe("TimeSeriesChart — data-presence filtering", () => {
  beforeEach(() => {
    lines.length = 0;
    legends.length = 0;
    tooltips.length = 0;
  });

  const allAbsentData: TimestampedRow[] = [
    { ts: 1000, values: { power: 1, cost: null } },
    { ts: 2000, values: { power: 2, cost: null } },
  ];

  it("a series absent at every row has no Line and no legend entry", () => {
    render(
      <TimeSeriesChart
        data={allAbsentData}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        interactiveLegend
      />
    );
    expect(lines.some((l) => l.name === "cost")).toBe(false);
    expect(lines.some((l) => l.name === "power")).toBe(true);
    expect(screen.queryByTestId("legend-toggle-cost")).not.toBeInTheDocument();
    expect(screen.getByTestId("legend-toggle-power")).toBeInTheDocument();
  });

  it("a series absent at every row is excluded from rendering even without interactiveLegend", () => {
    render(
      <TimeSeriesChart
        data={allAbsentData}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
      />
    );
    expect(lines.some((l) => l.name === "cost")).toBe(false);
    expect(lines.some((l) => l.name === "power")).toBe(true);
  });

  it("a series that gains data on a later render appears with no caller-side flag", () => {
    const { rerender } = render(
      <TimeSeriesChart
        data={allAbsentData}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        interactiveLegend
      />
    );
    expect(lines.some((l) => l.name === "cost")).toBe(false);

    rerender(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        interactiveLegend
      />
    );
    expect(lines.some((l) => l.name === "cost")).toBe(true);
    expect(screen.getByTestId("legend-toggle-cost")).toBeInTheDocument();
  });

  it("a series with only zero values at every row still renders and appears in the legend", () => {
    const zeroData: TimestampedRow[] = [
      { ts: 1000, values: { power: 1, cost: 0 } },
      { ts: 2000, values: { power: 2, cost: 0 } },
    ];
    render(
      <TimeSeriesChart
        data={zeroData}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        interactiveLegend
      />
    );
    expect(lines.some((l) => l.name === "cost")).toBe(true);
    expect(screen.getByTestId("legend-toggle-cost")).toBeInTheDocument();
  });
});

describe("TimeSeriesChart — per-series tooltip formatter", () => {
  beforeEach(() => {
    lines.length = 0;
    legends.length = 0;
    tooltips.length = 0;
  });

  it("a series with its own formatter uses it for its tooltip value", () => {
    const seriesWithFormatter: TimeSeriesSeriesSpec[] = [
      series[0],
      { ...series[1], formatter: (v) => `$${v.toFixed(2)}` },
    ];
    render(
      <TimeSeriesChart data={data} xAxisTickFormatter={() => ""} axes={axes} series={seriesWithFormatter} />
    );
    const formatter = tooltips[0].formatter as (v: number, n: string) => [string, string];
    expect(formatter(0.2, "cost")).toEqual(["$0.20", "cost"]);
  });

  it("a series without its own formatter falls back to the chart-level tooltipFormatter", () => {
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        tooltipFormatter={(v, n) => [`fallback:${v}`, n]}
      />
    );
    const formatter = tooltips[0].formatter as (v: number, n: string) => [string, string];
    expect(formatter(2, "power")).toEqual(["fallback:2", "power"]);
  });
});
