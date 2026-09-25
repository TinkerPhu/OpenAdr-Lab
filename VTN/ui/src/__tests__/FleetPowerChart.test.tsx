import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { FleetPowerChart, FLEET_KEY } from "../components/FleetPowerChart";
import { venColor, FLEET_SUM_COLOR } from "../utils/venColor";
import type { FleetHistory } from "../api/types";

// The chart itself is recharts; what matters here is what gets handed to it.
const chartProps: Record<string, unknown>[] = [];
vi.mock("@lab/charts/TimeSeriesChart", () => ({
  TimeSeriesChart: (props: Record<string, unknown>) => {
    chartProps.push(props);
    return null;
  },
}));

type Series = { key: string; label?: string; color: string; dataKey: (row: unknown) => unknown };
type Row = { ts: number; values: Record<string, number | null> | null };

function history(over: Partial<FleetHistory> = {}): FleetHistory {
  return {
    source: "raw",
    from: "2026-09-22T09:00:00Z",
    to: "2026-09-22T10:00:00Z",
    stepSeconds: 60,
    vens: [
      {
        venName: "ven-2",
        samples: [
          { ts: "2026-09-22T09:00:00Z", netPowerW: -1500 },
          { ts: "2026-09-22T09:01:00Z", netPowerW: -1000 },
        ],
      },
      {
        venName: "ven-1",
        samples: [
          { ts: "2026-09-22T09:00:00Z", netPowerW: 4000 },
          { ts: "2026-09-22T09:01:00Z", netPowerW: 3000 },
        ],
      },
    ],
    fleet: [
      { ts: "2026-09-22T09:00:00Z", netPowerW: 2500, contributingVens: 2 },
      { ts: "2026-09-22T09:01:00Z", netPowerW: 2000, contributingVens: 2 },
    ],
    ...over,
  };
}

function renderChart(h: FleetHistory = history()) {
  chartProps.length = 0;
  render(<FleetPowerChart history={h} windowMinutes={60} tickMinutes={10} nowMs={Date.parse("2026-09-22T09:01:30Z")} />);
  return chartProps[0];
}

describe("FleetPowerChart", () => {
  it("draws a line per VEN plus the fleet total", () => {
    const props = renderChart();
    const series = props.series as Series[];
    expect(series.map((s) => s.key)).toEqual(["ven-1", "ven-2", FLEET_KEY]);
  });

  /* recharts resolves a hovered tooltip by array index, so every series has to
   * read from the one merged row array — the reason mergeSeries exists. */
  it("folds every series into a single timestamp-keyed row array", () => {
    const props = renderChart();
    const rows = props.data as Row[];
    expect(rows).toHaveLength(2);
    expect(Object.keys(rows[0].values ?? {}).sort()).toEqual([FLEET_KEY, "ven-1", "ven-2"]);
  });

  it("converts the API's watts to kW", () => {
    const props = renderChart();
    const rows = props.data as Row[];
    expect(rows[0].values?.["ven-1"]).toBe(4);
    expect(rows[0].values?.[FLEET_KEY]).toBe(2.5);
  });

  /* Export is negative and stays negative: a chart that clamped it would hide
   * exactly the sites doing something interesting. */
  it("keeps an exporting VEN below the line", () => {
    const props = renderChart();
    const rows = props.data as Row[];
    expect(rows[0].values?.["ven-2"]).toBe(-1.5);
  });

  /* Import above and export below is the shape of a fleet's day; a domain that
   * omitted zero would hide the crossing. */
  it("always spans zero", () => {
    const props = renderChart();
    const [lo, hi] = (props.axes as { domain: [number, number] }[])[0].domain;
    expect(lo).toBeLessThanOrEqual(0);
    expect(hi).toBeGreaterThanOrEqual(0);
  });

  it("names the fleet total in the legend rather than showing its reserved key", () => {
    const series = renderChart().series as Series[];
    expect(series.find((s) => s.key === FLEET_KEY)?.label).toBe("fleet total");
  });

  /* A colour tied to list position would shuffle every time a VEN dropped out
   * of the window. */
  it("gives each VEN a colour derived from its name, and the total its own", () => {
    const series = renderChart().series as Series[];
    expect(series.find((s) => s.key === "ven-1")?.color).toBe(venColor("ven-1"));
    expect(series.find((s) => s.key === FLEET_KEY)?.color).toBe(FLEET_SUM_COLOR);
  });

  it("says which resolution it drew", () => {
    renderChart();
    expect(screen.getByTestId("fleet-chart-source")).toHaveTextContent("raw samples");
    expect(screen.getByTestId("fleet-chart-source")).toHaveTextContent("60s");
  });

  it("orders the legend the way a person counts, not the way bytes sort", () => {
    chartProps.length = 0;
    const many = history({
      vens: ["ven-10", "ven-2", "ven-1", "ven-20", "ven-3"].map((venName) => ({
        venName,
        samples: [{ ts: "2026-09-22T09:00:00Z", netPowerW: 1000 }],
      })),
    });
    render(<FleetPowerChart history={many} windowMinutes={60} tickMinutes={10} nowMs={0} />);

    // The legend renders in series order, so this is the legend's order.
    const keys = (chartProps[0].series as Series[]).map((x) => x.key);
    expect(keys).toEqual(["ven-1", "ven-2", "ven-3", "ven-10", "ven-20", FLEET_KEY]);
  });

  it("gives the curves real vertical room, not a dashboard cell's", () => {
    chartProps.length = 0;
    render(<FleetPowerChart history={history()} windowMinutes={60} tickMinutes={10} nowMs={0} />);
    // Twenty overlapping curves at cell height are a band, not a comparison.
    expect(chartProps[0].height as number).toBeGreaterThanOrEqual(400);
  });

  it("lets the kW axis follow whichever curves are still shown", () => {
    chartProps.length = 0;
    render(<FleetPowerChart history={history()} windowMinutes={60} tickMinutes={10} nowMs={0} />);
    const axis = (chartProps[0].axes as Array<Record<string, unknown>>)[0];
    // The fleet total is the sum of every VEN, so its range dwarfs any single
    // site's. A fixed shared domain flattens the sites into a few pixels, and
    // hiding the total would not give them the axis back.
    expect(axis.autoScale).toBe(true);
    expect(axis.autoScaleMinSpan).toBe(1);
  });

  it("explains an empty window instead of drawing an empty chart", () => {
    chartProps.length = 0;
    render(
      <FleetPowerChart history={history({ vens: [], fleet: [] })} windowMinutes={60} tickMinutes={10} nowMs={0} />,
    );
    expect(screen.getByTestId("fleet-chart-empty")).toBeVisible();
    expect(chartProps).toHaveLength(0);
  });
});

describe("venColor", () => {
  it("is stable for a name and distinct between neighbours", () => {
    expect(venColor("ven-1")).toBe(venColor("ven-1"));
    expect(venColor("ven-1")).not.toBe(venColor("ven-2"));
  });
});
