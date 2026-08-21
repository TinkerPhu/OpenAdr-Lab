/**
 * StackedAreaTooltip — unit tests
 * Verifies that the custom tooltip merges _pos/_neg series into one row per asset.
 */
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import type { ReactNode } from "react";
import { StackedAreaTooltip } from "../components/charts/StackedTimeSeriesChart";
import { ASSET_COLORS } from "../components/controller/types";

const colorMap = ASSET_COLORS;

const { xAxes } = vi.hoisted(() => ({ xAxes: [] as Array<Record<string, unknown>> }));

vi.mock("recharts", () => ({
  ComposedChart: ({ children }: { children: ReactNode }) => children,
  ResponsiveContainer: ({ children }: { children: ReactNode }) => children,
  CartesianGrid: () => null,
  XAxis: (props: Record<string, unknown>) => {
    xAxes.push(props);
    return null;
  },
  YAxis: () => null,
  Tooltip: () => null,
  ReferenceLine: () => null,
  Area: () => null,
  Line: () => null,
  Legend: () => null,
}));

function makePayload(entries: { name: string; value: number }[]) {
  return entries.map((e) => ({ name: e.name, value: e.value }));
}

describe("StackedAreaTooltip", () => {
  it("renders nothing when not active", () => {
    const { container } = render(
      <StackedAreaTooltip active={false} payload={[]} label={1000} colorMap={colorMap} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when payload is empty", () => {
    const { container } = render(
      <StackedAreaTooltip active={true} payload={[]} label={1000} colorMap={colorMap} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows one row per asset by merging pos and neg series", () => {
    const payload = makePayload([
      { name: "ev +", value: 3.5 },
      { name: "ev -", value: 0 },
      { name: "battery +", value: 0 },
      { name: "battery -", value: -2.0 },
      { name: "base_load +", value: 1.0 },
      { name: "base_load -", value: 0 },
    ]);
    render(
      <StackedAreaTooltip
        active={true}
        payload={payload as never}
        label={new Date("2026-01-01T10:00:00Z").getTime()}
        colorMap={colorMap}
      />
    );

    // One row per asset, not two
    expect(screen.getByText(/^EV \(planned\):/)).toBeInTheDocument();
    expect(screen.getByText(/^Battery \(planned\):/)).toBeInTheDocument();
    expect(screen.getByText(/^Base Load \(forecast\):/)).toBeInTheDocument();
    expect(screen.queryAllByText(/EV \(planned\)/).length).toBe(1);
    expect(screen.queryAllByText(/Battery \(planned\)/).length).toBe(1);
  });

  it("shows net kW for unidirectional import asset", () => {
    const payload = makePayload([
      { name: "ev +", value: 3.5 },
      { name: "ev -", value: 0 },
    ]);
    render(
      <StackedAreaTooltip
        active={true}
        payload={payload as never}
        label={1000}
        colorMap={colorMap}
      />
    );
    expect(screen.getByText(/\+3\.50 kW/)).toBeInTheDocument();
  });

  it("shows net kW for unidirectional export asset (negative)", () => {
    const payload = makePayload([
      { name: "pv +", value: 0 },
      { name: "pv -", value: -4.2 },
    ]);
    render(
      <StackedAreaTooltip
        active={true}
        payload={payload as never}
        label={1000}
        colorMap={colorMap}
      />
    );
    expect(screen.getByText(/-4\.20 kW/)).toBeInTheDocument();
  });

  it("shows net kW for bidirectional asset (battery charging and discharging)", () => {
    // battery_pos=1.5 (charging) and battery_neg=-0.5 net = +1.0
    const payload = makePayload([
      { name: "battery +", value: 1.5 },
      { name: "battery -", value: -0.5 },
    ]);
    render(
      <StackedAreaTooltip
        active={true}
        payload={payload as never}
        label={1000}
        colorMap={colorMap}
      />
    );
    expect(screen.getByText(/\+1\.00 kW/)).toBeInTheDocument();
  });

  it("keeps a visible sign for a sub-watt negative residual that rounds to 0 W", () => {
    // -0.0002 kW = -0.2 W, rounds to 0 W in formatPowerValue's Watts branch. A naive
    // `kw >= 0 ? "+" : ""` + rounded-string approach silently drops the sign here
    // (JS stringifies Math.round(-0.2) as "0", not "-0").
    const payload = makePayload([
      { name: "ev +", value: 0 },
      { name: "ev -", value: -0.0002 },
    ]);
    render(
      <StackedAreaTooltip active={true} payload={payload as never} label={1000} colorMap={colorMap} />
    );
    expect(screen.getByText(/-0 W/)).toBeInTheDocument();
  });

  it("shows grid line separately below a divider", () => {
    const payload = makePayload([
      { name: "ev +", value: 3.0 },
      { name: "ev -", value: 0 },
      { name: "Grid [kW]", value: 3.0 },
    ]);
    render(
      <StackedAreaTooltip
        active={true}
        payload={payload as never}
        label={1000}
        colorMap={colorMap}
      />
    );
    expect(screen.getByText(/Grid:/)).toBeInTheDocument();
    // Grid should NOT appear in asset rows
    expect(screen.queryAllByText(/^EV \(planned\):/).length).toBe(1);
    expect(screen.queryAllByText(/Grid/).length).toBe(1);
  });
});

describe("StackedTimeSeriesChart — time axis never stretches past its window", () => {
  function stackedPoint(ts: number) {
    return {
      ts,
      ev_pos: 0, ev_neg: 0,
      heater_pos: 0, heater_neg: 0,
      pv_pos: 0, pv_neg: 0,
      battery_pos: 0, battery_neg: 0,
      base_load_pos: 0, base_load_neg: 0,
      gridPowerKw: null,
    };
  }

  it("always passes allowDataOverflow so out-of-window data can't stretch the domain", async () => {
    xAxes.length = 0;
    const { StackedTimeSeriesChart } = await import("../components/charts/StackedTimeSeriesChart");
    render(
      <StackedTimeSeriesChart
        data={[stackedPoint(1000), stackedPoint(2000)]}
        assetIds={["ev"]}
        colorMap={colorMap}
        nowMs={1500}
        hoursBack={0.5}
        hoursForward={0.5}
      />
    );
    expect(xAxes[0].allowDataOverflow).toBe(true);
  });
});
