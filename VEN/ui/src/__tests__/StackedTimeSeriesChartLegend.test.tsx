/**
 * StackedTimeSeriesChart — legend grouping + interactiveLegend
 *
 * Covers: one legend entry per asset (not one per internal pos/neg series) regardless of
 * interactiveLegend, and the opt-in checkbox toggling both an asset's pos/neg Areas
 * together. Same recharts-mocking approach as TimeSeriesChart.test.tsx — the mocked
 * <Legend> renders its `content` prop so ChartLegend's real checkboxes are clickable.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactNode } from "react";
import type { AssetId, StackedAreaPoint } from "../components/controller/types";

const { areas, lines } = vi.hoisted(() => ({
  areas: [] as Array<Record<string, unknown>>,
  lines: [] as Array<Record<string, unknown>>,
}));

vi.mock("recharts", () => ({
  ComposedChart: ({ children }: { children: ReactNode }) => children,
  ResponsiveContainer: ({ children }: { children: ReactNode }) => children,
  CartesianGrid: () => null,
  XAxis: () => null,
  YAxis: () => null,
  Tooltip: () => null,
  ReferenceArea: () => null,
  ReferenceLine: () => null,
  Area: (props: Record<string, unknown>) => {
    areas.push(props);
    return null;
  },
  Line: (props: Record<string, unknown>) => {
    lines.push(props);
    return null;
  },
  Legend: (props: Record<string, unknown> & { content?: ReactNode }) => (props.content as ReactNode) ?? null,
}));

import { StackedTimeSeriesChart } from "../components/charts/StackedTimeSeriesChart";

const now = new Date("2026-01-01T12:00:00Z").getTime();
const assetIds: AssetId[] = ["ev", "pv"];
const colorMap = { ev: "#2196F3", pv: "#FFC107" };
const data: StackedAreaPoint[] = [
  { ts: now, ev_pos: 3.0, ev_neg: 0, pv_pos: 0, pv_neg: -2.0, base_load_pos: 0, base_load_neg: 0, battery_pos: 0, battery_neg: 0, heater_pos: 0, heater_neg: 0, gridPowerKw: 1.0 },
];

describe("StackedTimeSeriesChart — legend", () => {
  beforeEach(() => {
    areas.length = 0;
    lines.length = 0;
  });

  it("shows exactly one legend entry per asset, not one per pos/neg series", () => {
    render(<StackedTimeSeriesChart data={data} assetIds={assetIds} colorMap={colorMap} nowMs={now} />);
    // Non-interactive: no checkboxes, but labels should appear exactly once per asset.
    expect(screen.getAllByText("EV (planned)")).toHaveLength(1);
    expect(screen.getAllByText("PV (forecast)")).toHaveLength(1);
    expect(screen.getAllByText("Grid")).toHaveLength(1);
  });

  it("applies the one-entry-per-asset grouping even without interactiveLegend", () => {
    render(<StackedTimeSeriesChart data={data} assetIds={assetIds} colorMap={colorMap} nowMs={now} />);
    expect(screen.queryByTestId("legend-toggle-ev")).not.toBeInTheDocument();
    expect(screen.getByText("EV (planned)")).toBeInTheDocument();
  });

  it("with interactiveLegend, unchecking an asset hides both its positive and negative Area", async () => {
    render(
      <StackedTimeSeriesChart
        data={data}
        assetIds={assetIds}
        colorMap={colorMap}
        nowMs={now}
        interactiveLegend
      />
    );
    await userEvent.click(screen.getByTestId("legend-toggle-ev"));

    const evPos = [...areas].reverse().find((a) => a.name === "ev +");
    const evNeg = [...areas].reverse().find((a) => a.name === "ev -");
    const pvPos = [...areas].reverse().find((a) => a.name === "pv +");
    expect(evPos?.hide).toBe(true);
    expect(evNeg?.hide).toBe(true);
    expect(pvPos?.hide).toBe(false);
  });

  it("the grid entry toggles independently of any asset", async () => {
    render(
      <StackedTimeSeriesChart
        data={data}
        assetIds={assetIds}
        colorMap={colorMap}
        nowMs={now}
        interactiveLegend
      />
    );
    await userEvent.click(screen.getByTestId("legend-toggle-grid"));

    const gridLine = [...lines].reverse().find((l) => l.name === "Grid [kW]");
    const evPos = [...areas].reverse().find((a) => a.name === "ev +");
    expect(gridLine?.hide).toBe(true);
    expect(evPos?.hide).toBe(false);
  });
});
