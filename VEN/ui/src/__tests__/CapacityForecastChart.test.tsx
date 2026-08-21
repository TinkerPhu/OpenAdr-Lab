/**
 * CapacityForecastChart — renders both sustained-commitment direction
 * curves (import, export) as step lines with their step points preserved,
 * and shows nothing but an empty-state message before the first tick.
 */
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactNode } from "react";
import type { TimestampedRow } from "../components/charts/mergeSeries";

const { propsCalls } = vi.hoisted(() => ({
  propsCalls: [] as Array<Record<string, unknown>>,
}));

vi.mock("../components/charts/TimeSeriesChart", () => ({
  TimeSeriesChart: (props: Record<string, unknown>) => {
    propsCalls.push(props);
    return null as ReactNode;
  },
}));

import { CapacityForecastChart } from "../components/controller/charts/CapacityForecastChart";
import type { CapacityCurvesResponse } from "../api/types";

describe("CapacityForecastChart — empty state before first tick", () => {
  it("shows a waiting message and renders no chart when curves is null", () => {
    render(<CapacityForecastChart curves={null} />);
    expect(screen.getByTestId("capacity-forecast-empty")).toBeInTheDocument();
    expect(propsCalls).toHaveLength(0);
  });
});

describe("CapacityForecastChart — both directions rendered as step lines", () => {
  beforeEach(() => {
    propsCalls.length = 0;
  });

  it("forwards import and export series with stepAfter type, and the curve's own step points", () => {
    const start = "2026-08-20T12:00:00.000Z";
    const startMs = new Date(start).getTime();
    const curves: CapacityCurvesResponse = {
      import: {
        direction: "import",
        start,
        steps: [
          { elapsed_s: 0, power_kw: 5.0 },
          { elapsed_s: 3600, power_kw: 0.0 },
        ],
      },
      export: {
        direction: "export",
        start,
        steps: [
          { elapsed_s: 0, power_kw: 4.5 },
          { elapsed_s: 1800, power_kw: 0.0 },
        ],
      },
    };

    render(<CapacityForecastChart curves={curves} />);

    expect(propsCalls).toHaveLength(1);
    const { data, series } = propsCalls[0] as {
      data: TimestampedRow[];
      series: Array<{
        key: string;
        type?: string;
        dataKey: (row: TimestampedRow) => number | null | undefined;
      }>;
    };

    expect(series).toHaveLength(2);
    for (const s of series) {
      expect(s.type).toBe("stepAfter");
    }

    const importSeries = series.find((s) => s.key.startsWith("Import"))!;
    const exportSeries = series.find((s) => s.key.startsWith("Export"))!;

    const rowAt0 = data.find((row) => row.ts === startMs)!;
    expect(importSeries.dataKey(rowAt0)).toBe(5.0);
    expect(exportSeries.dataKey(rowAt0)).toBe(4.5);

    // Both curves' own step points (3600s import, 1800s export) survive the merge —
    // the step function, not a smoothed/interpolated shape.
    const rowAt1800 = data.find((row) => row.ts === startMs + 1_800_000)!;
    expect(exportSeries.dataKey(rowAt1800)).toBe(0.0);
    expect(importSeries.dataKey(rowAt1800)).toBe(5.0); // still holding until 3600s

    const rowAt3600 = data.find((row) => row.ts === startMs + 3_600_000)!;
    expect(importSeries.dataKey(rowAt3600)).toBe(0.0);
  });

  it("shows each direction's cumulative energy total", () => {
    const start = "2026-08-20T12:00:00.000Z";
    const curves: CapacityCurvesResponse = {
      import: {
        direction: "import",
        start,
        steps: [
          { elapsed_s: 0, power_kw: 5.0 },
          { elapsed_s: 3600, power_kw: 0.0 },
        ],
      },
      export: {
        direction: "export",
        start,
        steps: [{ elapsed_s: 0, power_kw: 4.5 }],
      },
    };

    render(<CapacityForecastChart curves={curves} />);

    // 5.0 kW held for exactly 1h = 5.0 kWh.
    expect(screen.getByTestId("capacity-forecast-import-energy").textContent).toContain("5");
    // Single-step export curve has no defined end — 0 kWh, per
    // CapacityCurve::energy_kwh_total's own "no next elapsed time" rule.
    expect(screen.getByTestId("capacity-forecast-export-energy").textContent).toContain("0");
  });
});
