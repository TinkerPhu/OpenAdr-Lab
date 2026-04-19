/**
 * StackedAreaTooltip — unit tests
 * Verifies that the custom tooltip merges _pos/_neg series into one row per asset.
 */
import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { StackedAreaTooltip } from "../components/controller/charts/StackedAreaChart";
import { ASSET_COLORS } from "../components/controller/types";

const colorMap = ASSET_COLORS;

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
