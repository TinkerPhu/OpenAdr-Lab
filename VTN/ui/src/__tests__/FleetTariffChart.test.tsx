import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { FleetTariffChart } from "../components/FleetTariffChart";
import { FleetPowerChart } from "../components/FleetPowerChart";
import { fleetChartWindow, FLEET_AXIS_WIDTH } from "../components/fleetChartWindow";
import { IMPORT_KEY, EXPORT_KEY } from "../utils/fleetTariff";
import type { FleetSignals, SignalBand, FleetHistory } from "../api/types";

// The chart itself is recharts; what matters here is what gets handed to it.
const chartProps: Record<string, unknown>[] = [];
vi.mock("@lab/charts/TimeSeriesChart", () => ({
  TimeSeriesChart: (props: Record<string, unknown>) => {
    chartProps.push(props);
    return null;
  },
}));

type Series = {
  key: string;
  color: string;
  strokeDasharray?: string;
  connectNulls?: boolean;
  type?: string;
  strokeWidth?: number;
};
type Axis = { unit?: string; width?: number; domain: [number, number] };

const T0 = Date.parse("2026-09-23T09:00:00Z");
const HOUR = 3_600_000;
const NOW = T0 + 2 * HOUR;

function band(over: Partial<SignalBand> = {}): SignalBand {
  return {
    from: new Date(T0).toISOString(),
    to: new Date(T0 + HOUR).toISOString(),
    eventID: "ev-1",
    eventName: "tou-pricing-day-ahead",
    payloadType: "PRICE",
    value: 0.4,
    ...over,
  };
}

const LAB_BANDS = [
  band({ payloadType: "PRICE", value: 0.4 }),
  band({ payloadType: "EXPORT_PRICE", value: 0.15 }),
];

function signals(vens: { venName: string; bands: SignalBand[] }[]): FleetSignals {
  return {
    from: new Date(T0).toISOString(),
    to: new Date(NOW).toISOString(),
    vens,
    rejectedEvents: 0,
  };
}

const fleet = (n: number, bands = LAB_BANDS) =>
  signals(Array.from({ length: n }, (_, i) => ({ venName: `ven-${i + 1}`, bands })));

describe("FleetTariffChart", () => {
  beforeEach(() => {
    chartProps.length = 0;
  });

  const renderChart = (s?: FleetSignals) =>
    render(<FleetTariffChart signals={s} windowMinutes={120} nowMs={NOW} />);

  it("draws one tariff for the whole fleet, not one per VEN", () => {
    renderChart(fleet(20));
    const keys = (chartProps[0].series as Series[]).map((s) => s.key);
    expect(keys).toEqual([IMPORT_KEY, EXPORT_KEY]);
  });

  it("uses the same colours and line style as the VEN Controller tab", () => {
    renderChart(fleet(20));
    const [imp, exp] = chartProps[0].series as Series[];
    expect(imp.color).toBe("#f44336");
    expect(exp.color).toBe("#4caf50");
    for (const s of [imp, exp]) {
      expect(s.strokeDasharray).toBe("5 5");
      expect(s.connectNulls).toBe(true);
      // Unset on purpose: inherited stepAfter/1.5 is what keeps the two tabs
      // identical without restating the defaults here.
      expect(s.type).toBeUndefined();
      expect(s.strokeWidth).toBeUndefined();
    }
  });

  it("is twice the standard chart height", () => {
    renderChart(fleet(20));
    expect(chartProps[0].height).toBe(280);
  });

  it("does not anchor the price axis at zero", () => {
    renderChart(fleet(20));
    const axis = (chartProps[0].axes as Axis[])[0];
    expect(axis.unit).toBe(" €/kWh");
    // A 0.15–0.40 band squashed against the top is exactly what
    // tightSpanDomain (not minSpanDomain) exists to avoid.
    expect(axis.domain[0]).toBeGreaterThan(0);
  });

  it("says so when the fleet was not sent one tariff", () => {
    const odd = LAB_BANDS.map((b) => ({ ...b, value: 0.99 }));
    renderChart(
      signals([
        { venName: "ven-1", bands: LAB_BANDS },
        { venName: "ven-2", bands: LAB_BANDS },
        { venName: "ven-3", bands: odd },
      ]),
    );
    expect(screen.getByTestId("fleet-tariff-disagree")).toHaveTextContent("ven-3");
  });

  it("stays quiet when every VEN got the same tariff", () => {
    renderChart(fleet(20));
    expect(screen.queryByTestId("fleet-tariff-disagree")).not.toBeInTheDocument();
    expect(screen.getByTestId("fleet-tariff-source")).toHaveTextContent("20 of 20");
  });

  it("explains an unpriced window instead of drawing an empty chart", () => {
    renderChart(signals([{ venName: "ven-1", bands: [] }]));
    expect(screen.getByTestId("fleet-tariff-empty")).toBeVisible();
    expect(chartProps).toHaveLength(0);
  });
});

/**
 * The requirement is that the two charts stack and read as one picture. That is
 * a visual claim, and this is the only thing that makes it testable: given the
 * same window, both must receive the same time domain and the same axis width,
 * or a price step will not sit above the power change it caused.
 */
describe("the two Fleet charts line up", () => {
  const history: FleetHistory = {
    source: "raw",
    from: new Date(T0).toISOString(),
    to: new Date(NOW).toISOString(),
    stepSeconds: 60,
    vens: [
      { venName: "ven-1", samples: [{ ts: new Date(T0).toISOString(), netPowerW: 1000 }] },
    ],
    fleet: [{ ts: new Date(T0).toISOString(), netPowerW: 1000, contributingVens: 1 }],
  };

  it("share tMin, tMax and the y-axis width", () => {
    chartProps.length = 0;
    render(<FleetTariffChart signals={fleet(20)} windowMinutes={120} nowMs={NOW} />);
    render(<FleetPowerChart history={history} windowMinutes={120} nowMs={NOW} />);

    const [tariff, power] = chartProps;
    const expected = fleetChartWindow(NOW, 120);

    expect(tariff.tMin).toBe(expected.tMin);
    expect(power.tMin).toBe(expected.tMin);
    expect(tariff.tMax).toBe(expected.tMax);
    expect(power.tMax).toBe(expected.tMax);
    expect((tariff.axes as Axis[])[0].width).toBe(FLEET_AXIS_WIDTH);
    expect((power.axes as Axis[])[0].width).toBe(FLEET_AXIS_WIDTH);
  });
});
