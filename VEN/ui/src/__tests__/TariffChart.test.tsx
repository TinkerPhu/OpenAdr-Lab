/**
 * TariffChart — triple Y-axis structure test
 *
 * Verifies that import/export tariff lines use their own left "tariff" axis (€/kWh),
 * cost rate uses an independent right "cost" axis (€/h), and CO₂ rate uses an
 * independent right "co2" axis (g/h) — three different physical dimensions must
 * never share a scale, since sharing one previously let a larger-magnitude series
 * flatten a smaller one into invisibility.
 */
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { TariffTimePoint } from "../components/controller/types";

// ─── Capture recharts structural props ───────────────────────────────────────
// vi.hoisted ensures these arrays exist before the vi.mock factory runs.

const { axes, lines, referenceAreas, xAxes } = vi.hoisted(() => ({
  axes: [] as Array<Record<string, unknown>>,
  lines: [] as Array<Record<string, unknown>>,
  referenceAreas: [] as Array<Record<string, unknown>>,
  xAxes: [] as Array<Record<string, unknown>>,
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
  XAxis: (props: Record<string, unknown>) => {
    xAxes.push(props);
    return null;
  },
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
    xAxes.length = 0;
  });

  it("renders the chart wrapper", () => {
    render(<TariffChart data={data} nowMs={now} />);
    expect(screen.getByTestId("tariff-chart")).toBeInTheDocument();
  });

  it("renders exactly three Y-axes: left for tariff, two right for cost and CO₂", () => {
    render(<TariffChart data={data} nowMs={now} />);
    expect(axes).toHaveLength(3);

    const left = axes.find((a) => a.yAxisId === "tariff" && a.orientation !== "right");
    const cost = axes.find((a) => a.yAxisId === "cost" && a.orientation === "right");
    const co2 = axes.find((a) => a.yAxisId === "co2" && a.orientation === "right");

    expect(left).toBeDefined();
    expect(cost).toBeDefined();
    expect(co2).toBeDefined();
  });

  it("CO₂ rate line is bound to the right co2 axis — not tariff or cost", () => {
    render(<TariffChart data={data} nowMs={now} />);
    const co2Line = lines.find((l) => l.dataKey === "totalCo2RateGH");
    expect(co2Line?.yAxisId).toBe("co2");
  });

  it("cost rate line is bound to its own cost axis, not the tariff axis", () => {
    render(<TariffChart data={data} nowMs={now} />);
    const costLine = lines.find((l) => l.dataKey === "totalCostRateEurH");
    expect(costLine?.yAxisId).toBe("cost");
  });

  it("import/export tariff lines are on the left tariff axis only", () => {
    render(<TariffChart data={data} nowMs={now} />);
    const tariffLines = lines.filter((l) => l.yAxisId === "tariff");
    const dataKeys = tariffLines.map((l) => l.dataKey as string);
    expect(dataKeys).toContain("importPriceEurKwh");
    expect(dataKeys).toContain("exportPriceEurKwh");
    expect(dataKeys).not.toContain("totalCostRateEurH");
  });

  it("each axis carries its own physically-correct unit label, never a bare €", () => {
    render(<TariffChart data={data} nowMs={now} />);
    const tariff = axes.find((a) => a.yAxisId === "tariff");
    const cost = axes.find((a) => a.yAxisId === "cost");
    const co2 = axes.find((a) => a.yAxisId === "co2");
    expect(tariff?.unit).toBe(" €/kWh");
    expect(cost?.unit).toBe(" €/h");
    expect(co2?.unit).toBe(" g/h");
  });

  it("tariff's rendered domain is independent of cost rate's magnitude", () => {
    // A fixture where cost rate's range would previously have flattened tariff
    // when both shared one axis: tariff stays within [0.15, 0.35], cost swings
    // far wider (±5 €/h during a high-power event).
    const wideCostData: TariffTimePoint[] = [
      { ts: now - 1_800_000, importPriceEurKwh: 0.20, exportPriceEurKwh: 0.15, co2GKwh: 300, totalCostRateEurH: 5.0, totalCo2RateGH: 750, gridPowerKw: 25 },
      { ts: now + 1_800_000, importPriceEurKwh: 0.35, exportPriceEurKwh: 0.26, co2GKwh: 420, totalCostRateEurH: -4.5, totalCo2RateGH: -840, gridPowerKw: null },
    ];
    render(<TariffChart data={wideCostData} nowMs={now} />);
    const tariff = axes.find((a) => a.yAxisId === "tariff");
    const cost = axes.find((a) => a.yAxisId === "cost");
    const [tMin, tMax] = tariff?.domain as [number, number];
    const [cMin, cMax] = cost?.domain as [number, number];
    // Tariff's own domain stays tight around its data (not stretched to ±5 by cost's range).
    expect(tMax - tMin).toBeLessThan(1);
    // Cost's domain, unconstrained by tariff, spans its own much wider real range.
    expect(cMax - cMin).toBeGreaterThan(5);
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

  it("passes rounded-clock X-axis ticks only when xAxisTickIntervalMinutes is set", () => {
    render(<TariffChart data={data} nowMs={now} />);
    expect(xAxes[0].ticks).toBeUndefined();

    xAxes.length = 0;
    render(<TariffChart data={data} nowMs={now} xAxisTickIntervalMinutes={30} />);
    expect(Array.isArray(xAxes[0].ticks)).toBe(true);
    expect((xAxes[0].ticks as number[]).length).toBeGreaterThan(0);
  });
});
