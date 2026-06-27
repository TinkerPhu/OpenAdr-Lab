/**
 * TariffChart — dual Y-axis structure test
 *
 * Verifies that import/export/cost lines use the left "tariff" axis (€/kWh)
 * and the CO₂ rate line uses an independent right "co2" axis (g/h).
 * Without this separation the CO₂ values compress the tariff curves into invisibility.
 */
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { TariffTimePoint } from "../components/controller/types";

// ─── Capture recharts structural props ───────────────────────────────────────
// vi.hoisted ensures these arrays exist before the vi.mock factory runs.

const { axes, lines, referenceAreas } = vi.hoisted(() => ({
  axes: [] as Array<Record<string, unknown>>,
  lines: [] as Array<Record<string, unknown>>,
  referenceAreas: [] as Array<Record<string, unknown>>,
}));

vi.mock("recharts", () => ({
  ComposedChart: ({ children }: { children: unknown }) => children,
  ResponsiveContainer: ({ children }: { children: unknown }) => children,
  YAxis: (props: Record<string, unknown>) => {
    axes.push(props);
    return null;
  },
  Line: (props: Record<string, unknown>) => {
    lines.push(props);
    return null;
  },
  ReferenceArea: (props: Record<string, unknown>) => {
    referenceAreas.push(props);
    return null;
  },
  XAxis: () => null,
  CartesianGrid: () => null,
  Tooltip: () => null,
  Legend: () => null,
  ReferenceLine: () => null,
}));

import { TariffChart } from "../components/controller/charts/TariffChart";

// ─── Fixtures ────────────────────────────────────────────────────────────────

const now = new Date("2026-01-01T12:00:00Z").getTime();

const data: TariffTimePoint[] = [
  {
    ts: now - 1_800_000,
    importPriceEurKwh: 0.20,
    exportPriceEurKwh: 0.15,
    co2GKwh: 300,
    totalCostRateEurH: 0.05,
    totalCo2RateGH: 750,
    gridPowerKw: 2.5,
  },
  {
    ts: now + 1_800_000,
    importPriceEurKwh: 0.35,
    exportPriceEurKwh: 0.26,
    co2GKwh: 420,
    totalCostRateEurH: -0.02,
    totalCo2RateGH: -840,
    gridPowerKw: null,
  },
];

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("TariffChart — dual Y-axis", () => {
  beforeEach(() => {
    axes.length = 0;
    lines.length = 0;
    referenceAreas.length = 0;
  });

  it("renders the chart wrapper", () => {
    render(<TariffChart data={data} nowMs={now} />);
    expect(screen.getByTestId("tariff-chart")).toBeInTheDocument();
  });

  it("renders exactly two Y-axes: left for tariff, right for CO₂", () => {
    render(<TariffChart data={data} nowMs={now} />);
    expect(axes).toHaveLength(2);

    const left = axes.find((a) => a.yAxisId === "tariff" && a.orientation !== "right");
    const right = axes.find((a) => a.yAxisId === "co2" && a.orientation === "right");

    expect(left).toBeDefined();
    expect(right).toBeDefined();
  });

  it("CO₂ rate line is bound to the right co2 axis — not the tariff axis", () => {
    render(<TariffChart data={data} nowMs={now} />);
    const co2Line = lines.find((l) => l.dataKey === "totalCo2RateGH");
    expect(co2Line?.yAxisId).toBe("co2");
  });

  it("tariff lines (import, export, cost rate) are all on the left tariff axis", () => {
    render(<TariffChart data={data} nowMs={now} />);
    const tariffLines = lines.filter((l) => l.yAxisId === "tariff");
    const dataKeys = tariffLines.map((l) => l.dataKey as string);
    expect(dataKeys).toContain("importPriceEurKwh");
    expect(dataKeys).toContain("exportPriceEurKwh");
    expect(dataKeys).toContain("totalCostRateEurH");
  });

  it("left axis has € unit and right axis has g/h unit", () => {
    render(<TariffChart data={data} nowMs={now} />);
    const left = axes.find((a) => a.yAxisId === "tariff");
    const right = axes.find((a) => a.yAxisId === "co2");
    expect(left?.unit).toBe(" €");
    expect(right?.unit).toBe(" g/h");
  });

  it("renders one ReferenceArea per zone when zones prop is provided", () => {
    const zones = [
      { from: new Date(now).toISOString(), to: new Date(now + 8 * 3_600_000).toISOString(), step_s: 300 },
      { from: new Date(now + 8 * 3_600_000).toISOString(), to: new Date(now + 24 * 3_600_000).toISOString(), step_s: 600 },
    ];
    render(<TariffChart data={data} nowMs={now} zones={zones} />);
    expect(referenceAreas).toHaveLength(zones.length);
    expect(referenceAreas[0].x1).toBe(new Date(zones[0].from).getTime());
    expect(referenceAreas[0].x2).toBe(new Date(zones[0].to).getTime());
    expect(referenceAreas[1].x1).toBe(new Date(zones[1].from).getTime());
  });

  it("renders no ReferenceArea when zones prop is omitted", () => {
    render(<TariffChart data={data} nowMs={now} />);
    expect(referenceAreas).toHaveLength(0);
  });
});
