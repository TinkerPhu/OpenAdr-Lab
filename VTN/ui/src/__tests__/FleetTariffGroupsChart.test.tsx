import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { FleetTariffGroupsChart, MAX_TARIFF_GROUPS } from "../components/FleetTariffGroupsChart";
import type { FleetSignals, SignalBand } from "../api/types";

const chartProps: Record<string, unknown>[] = [];
vi.mock("@lab/ui/TimeSeriesChart", () => ({
  TimeSeriesChart: (props: Record<string, unknown>) => {
    chartProps.push(props);
    return null;
  },
}));

type Series = { key: string; color: string };

const T0 = Date.parse("2026-09-23T09:00:00Z");
const HOUR = 3_600_000;
const NOW = T0 + HOUR / 2;

function tariffOf(name: string, price: number): SignalBand[] {
  const base = {
    from: new Date(T0).toISOString(),
    to: new Date(T0 + 2 * HOUR).toISOString(),
    eventID: name,
    eventName: name,
  };
  return [
    { ...base, payloadType: "PRICE", value: price },
    { ...base, payloadType: "EXPORT_PRICE", value: price * 0.75 },
  ];
}

/** `sizes[i]` VENs on a tariff named `exp-flat-PRICE-g<i>` priced 0.10 + 0.01 i. */
function fleetOf(sizes: number[]): FleetSignals {
  let n = 0;
  const vens = sizes.flatMap((size, g) =>
    Array.from({ length: size }, () => ({
      venName: `ven-${++n}`,
      bands: tariffOf(`exp-flat-PRICE-g${g}`, 0.1 + 0.01 * g),
    })),
  );
  return {
    from: new Date(T0).toISOString(),
    to: new Date(T0 + 2 * HOUR).toISOString(),
    vens,
    rejectedEvents: 0,
  };
}

const renderChart = (s?: FleetSignals) =>
  render(<FleetTariffGroupsChart signals={s} windowMinutes={120} tickMinutes={10} nowMs={NOW} />);

const chartOf = (testId: string) => chartProps.find((p) => p.testId === testId)!;

describe("FleetTariffGroupsChart", () => {
  beforeEach(() => {
    chartProps.length = 0;
  });

  it("draws five import lines and five export lines for five tariffs, a pair in one colour", () => {
    renderChart(fleetOf([1, 4, 5, 6, 4]));
    const imp = chartOf("fleet-tariff-import-chart").series as Series[];
    const exp = chartOf("fleet-tariff-export-chart").series as Series[];
    expect(imp).toHaveLength(5);
    expect(exp).toHaveLength(5);
    expect(exp.map((s) => [s.key, s.color])).toEqual(imp.map((s) => [s.key, s.color]));
    expect(new Set(imp.map((s) => s.color)).size).toBe(5);
  });

  it("labels the lines with the short event name and the VEN count", () => {
    renderChart(fleetOf([1, 4]));
    const keys = (chartOf("fleet-tariff-import-chart").series as Series[]).map((s) => s.key);
    expect(keys).toEqual(["g1 · 4 VENs", "g0 · 1 VEN"]);
  });

  it("lists the VENs of a tariff on hover", () => {
    renderChart(fleetOf([1, 4]));
    const labels = screen.getAllByTestId("fleet-tariff-group");
    expect(labels[0]).toHaveAttribute("title", "exp-flat-PRICE-g1: ven-2, ven-3, ven-4, ven-5");
  });

  it("shows one tariff for everybody as one pair labelled with all VENs", () => {
    renderChart(fleetOf([20]));
    const imp = chartOf("fleet-tariff-import-chart").series as Series[];
    expect(imp.map((s) => s.key)).toEqual(["exp-flat-PRICE-g0 · 20 VENs"]);
  });

  it("does not describe a split as disagreement", () => {
    renderChart(fleetOf([10, 10]));
    expect(screen.queryByText(/different tariff/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("fleet-tariff-source")).toHaveTextContent("20 of 20 VENs priced");
  });

  it("states that the curves are the VTN's events as of now", () => {
    renderChart(fleetOf([2]));
    expect(screen.getByTestId("fleet-tariff-source")).toHaveTextContent("VTN events as of now");
  });

  it("draws the largest tariffs and names the rest past the cap", () => {
    const sizes = Array.from({ length: MAX_TARIFF_GROUPS + 2 }, (_, i) => 20 - i);
    renderChart(fleetOf(sizes));
    expect(chartOf("fleet-tariff-import-chart").series as Series[]).toHaveLength(MAX_TARIFF_GROUPS);
    expect(screen.getByTestId("fleet-tariff-more")).toHaveTextContent("2 more tariff(s)");
  });

  it("counts a VEN without a price as not priced", () => {
    const fleet = fleetOf([2]);
    fleet.vens.push({ venName: "ven-9", bands: [] });
    renderChart(fleet);
    expect(screen.getByTestId("fleet-tariff-source")).toHaveTextContent("2 of 3 VENs priced");
  });

  it("explains an unpriced window instead of drawing an empty chart", () => {
    renderChart({ ...fleetOf([1]), vens: [{ venName: "ven-1", bands: [] }] });
    expect(screen.getByTestId("fleet-tariff-empty")).toBeVisible();
    expect(chartProps).toHaveLength(0);
  });
});
