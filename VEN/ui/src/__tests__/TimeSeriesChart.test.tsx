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

const { lines, legends, tooltips, areas, xAxes, yAxes } = vi.hoisted(() => ({
  lines: [] as Array<Record<string, unknown>>,
  yAxes: [] as Array<Record<string, unknown>>,
  legends: [] as Array<Record<string, unknown>>,
  tooltips: [] as Array<Record<string, unknown>>,
  areas: [] as Array<Record<string, unknown>>,
  xAxes: [] as Array<Record<string, unknown>>,
}));

vi.mock("recharts", () => ({
  ComposedChart: ({ children }: { children: ReactNode }) => children,
  ResponsiveContainer: ({ children }: { children: ReactNode }) => children,
  CartesianGrid: () => null,
  XAxis: (props: Record<string, unknown>) => {
    xAxes.push(props);
    return null;
  },
  YAxis: (props: Record<string, unknown>) => {
    yAxes.push(props);
    return null;
  },
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
  Area: (props: Record<string, unknown>) => {
    areas.push(props);
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

describe("TimeSeriesChart — bands (BL-43)", () => {
  beforeEach(() => {
    lines.length = 0;
    areas.length = 0;
  });

  it("renders one Area per band, with a [lower, upper] tuple dataKey accessor", () => {
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        bands={[
          {
            key: "headroom",
            axisId: "power",
            lower: (r) => (r.values?.power ?? 0) - 1,
            upper: (r) => (r.values?.power ?? 0) + 1,
            color: "#8BC34A",
          },
        ]}
      />
    );
    expect(areas).toHaveLength(1);
    const band = areas[0];
    expect(band.name).toBe("headroom");
    expect(band.fill).toBe("#8BC34A");
    expect(band.stroke).toBe("none");
    const dataKey = band.dataKey as (row: TimestampedRow) => [number | null, number | null];
    expect(dataKey(data[0])).toEqual([0, 2]); // power=1 -> [1-1, 1+1]
    expect(dataKey(data[1])).toEqual([1, 3]); // power=2 -> [2-1, 2+1]
  });

  it("a band with a null bound at a row yields a [null, null] pair, not a partial range", () => {
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        bands={[
          {
            key: "headroom",
            axisId: "power",
            lower: () => null,
            upper: (r) => (r.values?.power ?? 0) + 1,
            color: "#8BC34A",
          },
        ]}
      />
    );
    const dataKey = areas[0].dataKey as (row: TimestampedRow) => [number | null, number | null];
    expect(dataKey(data[0])).toEqual([null, null]);
  });

  it("no bands prop renders no Area elements", () => {
    render(
      <TimeSeriesChart data={data} xAxisTickFormatter={() => ""} axes={axes} series={series} />
    );
    expect(areas).toHaveLength(0);
  });
});

describe("TimeSeriesChart — time axis never stretches past its window", () => {
  beforeEach(() => {
    xAxes.length = 0;
  });

  // A skewed client clock (or any stale/late data point outside [tMin, tMax]) must
  // never widen the axis and squeeze the intended window into a sliver — regression
  // test for exactly that failure mode, seen live on a VM with a drifted OS clock.
  it("always passes allowDataOverflow so out-of-window data can't stretch the domain", () => {
    render(
      <TimeSeriesChart
        data={data}
        tMin={1000}
        tMax={2000}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
      />
    );
    expect(xAxes[0].allowDataOverflow).toBe(true);
    expect(xAxes[0].domain).toEqual([1000, 2000]);
  });

  it("still sets allowDataOverflow when tMin/tMax are undefined (auto domain)", () => {
    render(
      <TimeSeriesChart data={data} xAxisTickFormatter={() => ""} axes={axes} series={series} />
    );
    expect(xAxes[0].allowDataOverflow).toBe(true);
  });
});

describe("TimeSeriesChart — band tooltip formatter", () => {
  beforeEach(() => {
    lines.length = 0;
    areas.length = 0;
    tooltips.length = 0;
  });

  it("a band with its own formatter uses it for its [lower, upper] tooltip value", () => {
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        bands={[
          {
            key: "headroom",
            axisId: "power",
            lower: (r) => (r.values?.power ?? 0) - 1,
            upper: (r) => (r.values?.power ?? 0) + 1,
            color: "#8BC34A",
            formatter: (lo, hi) => `${lo.toFixed(1)} to ${hi.toFixed(1)} kW`,
          },
        ]}
      />
    );
    const formatter = tooltips[0].formatter as (v: unknown, n: string) => [string, string];
    expect(formatter([0, 2], "headroom")).toEqual(["0.0 to 2.0 kW", "headroom"]);
  });

  it("a band without its own formatter falls back to a plain dash-joined pair", () => {
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={series}
        bands={[
          {
            key: "headroom",
            axisId: "power",
            lower: (r) => (r.values?.power ?? 0) - 1,
            upper: (r) => (r.values?.power ?? 0) + 1,
            color: "#8BC34A",
          },
        ]}
      />
    );
    const formatter = tooltips[0].formatter as (v: unknown, n: string) => [string, string];
    expect(formatter([0, 2], "headroom")).toEqual(["0 – 2", "headroom"]);
  });

  it("a series tooltip formatter is unaffected by band handling", () => {
    const seriesWithFormatter: TimeSeriesSeriesSpec[] = [
      { ...series[0], formatter: (v) => `${v.toFixed(1)} kW` },
    ];
    render(
      <TimeSeriesChart
        data={data}
        xAxisTickFormatter={() => ""}
        axes={axes}
        series={seriesWithFormatter}
        bands={[
          {
            key: "headroom",
            axisId: "power",
            lower: (r) => (r.values?.power ?? 0) - 1,
            upper: (r) => (r.values?.power ?? 0) + 1,
            color: "#8BC34A",
          },
        ]}
      />
    );
    const formatter = tooltips[0].formatter as (v: unknown, n: string) => [string, string];
    expect(formatter(1, "power")).toEqual(["1.0 kW", "power"]);
  });
});

/**
 * The Y-axis rounding rule lives in this composition, not in its callers: a caller passes only
 * a raw data domain (there is no `ticks` prop on TimeSeriesAxisSpec any more), and every axis
 * comes out with round tick values. See `niceAxis` in axisDomain.ts.
 */
describe("TimeSeriesChart — rounded Y ticks (applied by the composition, not the caller)", () => {
  beforeEach(() => {
    yAxes.length = 0;
  });

  const renderWithAxes = (axisSpecs: TimeSeriesAxisSpec[]) =>
    render(
      <TimeSeriesChart
        data={data}
        tMin={1000}
        tMax={2000}
        xAxisTickFormatter={(t) => String(t)}
        axes={axisSpecs}
        series={[series[0]]}
      />
    );

  it("rounds an ugly single-sign domain — the real PV export-revenue case", () => {
    // Pre-fix this axis reached recharts as [-0.4172, 0] with no ticks, and rendered
    // -0.4172 / -0.3129 / -0.2086 / ... as labels.
    renderWithAxes([{ id: "cost", domain: [-0.4172, 0], unit: " €/h" }]);
    const axis = yAxes.find((a) => a.yAxisId === "cost")!;
    expect(axis.ticks).toEqual([-0.6, -0.4, -0.2, 0]);
    expect(axis.domain).toEqual([-0.6, 0]);
  });

  it("formats ticks with the step's own precision when the axis declares no formatter", () => {
    renderWithAxes([{ id: "cost", domain: [-0.4172, 0], unit: " €/h" }]);
    const axis = yAxes.find((a) => a.yAxisId === "cost")!;
    const format = axis.tickFormatter as (v: number) => string;
    expect(format(-0.2)).toBe("-0.2");
  });

  it("keeps an axis' own unit-specific tickFormatter when it declares one", () => {
    const formatter = (v: number) => `${v} kW`;
    renderWithAxes([{ id: "power", domain: [-3.2, 1.1], tickFormatter: formatter }]);
    expect(yAxes.find((a) => a.yAxisId === "power")!.tickFormatter).toBe(formatter);
  });

  it("leaves a hidden (tooltip-only) axis' domain untouched", () => {
    renderWithAxes([{ id: "state", hidden: true, domain: [0, 1] }]);
    const axis = yAxes.find((a) => a.yAxisId === "state")!;
    expect(axis.domain).toEqual([0, 1]);
    expect(axis.ticks).toBeUndefined();
  });
});
