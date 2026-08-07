/**
 * AssetTimelineChart — PV curtailment shading
 *
 * Verifies the three curtailment states (hardware-capped, planned, unplanned) render
 * distinct ReferenceArea bands, and that uncurtailed data renders none. See
 * openspec/changes/pv-curtailment-history/.
 */
import { render } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { AssetTimelinePoint } from "../components/controller/types";

const { referenceAreas, lines, xAxes } = vi.hoisted(() => ({
  referenceAreas: [] as Array<Record<string, unknown>>,
  lines: [] as Array<Record<string, unknown>>,
  xAxes: [] as Array<Record<string, unknown>>,
}));

vi.mock("recharts", () => ({
  ComposedChart: ({ children }: { children: unknown }) => children,
  ResponsiveContainer: ({ children }: { children: unknown }) => children,
  YAxis: () => null,
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

import { AssetTimelineChart } from "../components/controller/charts/AssetTimelineChart";
import type { ForecastAccuracySample } from "../api/types";

const now = new Date("2026-01-01T12:00:00Z").getTime();
const minute = 60_000;

function point(offsetMs: number, values: Record<string, number>): AssetTimelinePoint {
  return { ts: now + offsetMs, values };
}

function forecastSample(leadKind: "near" | "far", targetTs: number): ForecastAccuracySample {
  return {
    asset_id: "pv",
    lead_kind: leadKind,
    target_ts: targetTs,
    predicted_kw: -3.0,
    predicted_at: now,
    actual_kw: null,
    actual_at: null,
  };
}

describe("AssetTimelineChart — PV curtailment shading", () => {
  beforeEach(() => {
    referenceAreas.length = 0;
    lines.length = 0;
  });

  it("renders no curtailment band for uncurtailed data", () => {
    const data = [
      point(-2 * minute, { power_kw: -3.0 }),
      point(-1 * minute, { power_kw: -3.0 }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);
    expect(referenceAreas).toHaveLength(0);
  });

  it("renders a hardware-capped band when output is pinned at inverter_max_kw", () => {
    const data = [
      point(-2 * minute, { power_kw: -5.0, inverter_max_kw: 5.0 }),
      point(-1 * minute, { power_kw: -5.0, inverter_max_kw: 5.0 }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);
    expect(referenceAreas).toHaveLength(1);
    expect(referenceAreas[0].fill).toContain("120,120,120");
  });

  it("a commanded limit at or above inverter_max_kw is treated as hardware-capped, not imposed", () => {
    const data = [
      point(-2 * minute, {
        power_kw: -5.0,
        generation_limit_kw: -8.0, // looser than inverter_max_kw — output pinned by hardware instead
        curtailment_source: 2,
        inverter_max_kw: 5.0,
      }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);
    expect(referenceAreas).toHaveLength(1);
    expect(referenceAreas[0].fill).toContain("120,120,120");
  });

  it("renders a planned (amber) band for plan-sourced imposed curtailment", () => {
    const data = [
      point(-2 * minute, {
        power_kw: -2.0,
        generation_limit_kw: -2.0,
        curtailment_source: 1,
        inverter_max_kw: 5.0,
      }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);
    expect(referenceAreas).toHaveLength(1);
    expect(referenceAreas[0].fill).toContain("230,160,20");
  });

  it("renders an unplanned (red) band for capacity-sourced imposed curtailment", () => {
    const data = [
      point(-2 * minute, {
        power_kw: -2.0,
        generation_limit_kw: -2.0,
        curtailment_source: 2,
        inverter_max_kw: 5.0,
      }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);
    expect(referenceAreas).toHaveLength(1);
    expect(referenceAreas[0].fill).toContain("210,30,30");
  });

  it("renders an unplanned (red) band for arbiter-sourced imposed curtailment", () => {
    const data = [
      point(-2 * minute, {
        power_kw: -2.0,
        generation_limit_kw: -2.0,
        curtailment_source: 3,
        inverter_max_kw: 5.0,
      }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);
    expect(referenceAreas).toHaveLength(1);
    expect(referenceAreas[0].fill).toContain("210,30,30");
  });

  it("renders an unplanned (red) band for manual-sourced imposed curtailment", () => {
    const data = [
      point(-2 * minute, {
        power_kw: -2.0,
        generation_limit_kw: -2.0,
        curtailment_source: 4,
        inverter_max_kw: 5.0,
      }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);
    expect(referenceAreas).toHaveLength(1);
    expect(referenceAreas[0].fill).toContain("210,30,30");
  });

  it("renders a planned band for a future slot where pv_used_kw is below pv_forecast_kw", () => {
    const data = [point(2 * minute, { power_kw: -3.0, pv_forecast_kw: 5.0 })];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);
    expect(referenceAreas).toHaveLength(1);
    expect(referenceAreas[0].fill).toContain("230,160,20");
  });

  it("renders no band for a future slot where pv_used_kw equals pv_forecast_kw", () => {
    const data = [point(2 * minute, { power_kw: -5.0, pv_forecast_kw: 5.0 })];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);
    expect(referenceAreas).toHaveLength(0);
  });

  it("does not shade curtailment for non-PV charts (pvCurtailment omitted)", () => {
    const data = [
      point(-2 * minute, {
        power_kw: -2.0,
        generation_limit_kw: -2.0,
        curtailment_source: 2,
        inverter_max_kw: 5.0,
      }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} />);
    expect(referenceAreas).toHaveLength(0);
  });
});

// forecast-accuracy-tracking (task 6.3)
describe("AssetTimelineChart — near/far forecast overlay", () => {
  beforeEach(() => {
    lines.length = 0;
  });

  it("renders both the near and far forecast series when data is present", () => {
    const data = [point(-1 * minute, { power_kw: -3.0 })];
    render(
      <AssetTimelineChart
        data={data}
        color="#000"
        nowMs={now}
        nearForecast={[forecastSample("near", now + minute)]}
        farForecast={[forecastSample("far", now + 5 * minute)]}
      />
    );
    const names = lines.map((l) => l.name);
    expect(names).toContain("Forecast (near) [kW]");
    expect(names).toContain("Forecast (far) [kW]");
  });

  it("renders cleanly with no overlay lines when neither prop is passed", () => {
    const data = [point(-1 * minute, { power_kw: -3.0 })];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} />);
    const names = lines.map((l) => l.name);
    expect(names).not.toContain("Forecast (near) [kW]");
    expect(names).not.toContain("Forecast (far) [kW]");
  });

  it("renders cleanly with no overlay lines when the query returned empty arrays", () => {
    const data = [point(-1 * minute, { power_kw: -3.0 })];
    render(
      <AssetTimelineChart data={data} color="#000" nowMs={now} nearForecast={[]} farForecast={[]} />
    );
    const names = lines.map((l) => l.name);
    expect(names).not.toContain("Forecast (near) [kW]");
    expect(names).not.toContain("Forecast (far) [kW]");
  });
});

describe("AssetTimelineChart — X-axis tick rounding", () => {
  beforeEach(() => {
    xAxes.length = 0;
  });

  it("leaves ticks undefined (recharts default) when xAxisTickIntervalMinutes is omitted", () => {
    const data = [point(-1 * minute, { power_kw: -3.0 })];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} />);
    expect(xAxes[0].ticks).toBeUndefined();
  });

  it("passes rounded-clock ticks when xAxisTickIntervalMinutes is set", () => {
    const data = [point(-1 * minute, { power_kw: -3.0 })];
    render(
      <AssetTimelineChart data={data} color="#000" nowMs={now} xAxisTickIntervalMinutes={30} />
    );
    expect(Array.isArray(xAxes[0].ticks)).toBe(true);
    for (const t of xAxes[0].ticks as number[]) {
      expect(new Date(t).getUTCMinutes() % 30).toBe(0);
    }
  });
});
