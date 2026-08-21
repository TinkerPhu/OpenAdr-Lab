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

const { referenceAreas, lines, xAxes, yAxes, composedChartData } = vi.hoisted(() => ({
  referenceAreas: [] as Array<Record<string, unknown>>,
  yAxes: [] as Array<Record<string, unknown>>,
  lines: [] as Array<Record<string, unknown>>,
  xAxes: [] as Array<Record<string, unknown>>,
  composedChartData: [] as Array<Record<string, unknown>>,
}));

vi.mock("recharts", () => ({
  ComposedChart: ({ children, data }: { children: unknown; data: Record<string, unknown>[] }) => {
    composedChartData.length = 0;
    composedChartData.push(...data);
    return children;
  },
  ResponsiveContainer: ({ children }: { children: unknown }) => children,
  YAxis: (props: Record<string, unknown>) => {
    yAxes.push(props);
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

  it("holds (LOCF) the forecast value across one-minute ticks between two ~5-minute-apart samples, so the step line has no gap and hover anywhere on the plateau reports the held value", () => {
    // One-minute tick grid, no forecast sample at most of these timestamps — only at
    // now+0 and now+5min. Mirrors the real history-tab shape (1-min ticks, 5-min plan cycle).
    const data = Array.from({ length: 6 }, (_, i) => point(i * minute, { power_kw: -1.0 }));
    render(
      <AssetTimelineChart
        data={data}
        color="#000"
        nowMs={now}
        nearForecast={[forecastSample("near", now), { ...forecastSample("near", now + 5 * minute), predicted_kw: -2.5 }]}
      />
    );
    const nearLine = lines.find((l) => l.name === "Forecast (near) [kW]")!;
    const dataKey = nearLine.dataKey as (pt: { values: Record<string, number> }) => number | null;
    // Sample points themselves resolve directly.
    const atStart = composedChartData.find((p) => p.ts === now)!;
    const atEnd = composedChartData.find((p) => p.ts === now + 5 * minute)!;
    expect(dataKey(atStart as { values: Record<string, number> })).toBe(-3.0); // default forecastSample predicted_kw
    expect(dataKey(atEnd as { values: Record<string, number> })).toBe(-2.5);
    // Every in-between minute must carry the LAST known sample forward, not null.
    for (let i = 1; i < 5; i++) {
      const pt = composedChartData.find((p) => p.ts === now + i * minute)!;
      expect(dataKey(pt as { values: Record<string, number> })).toBe(-3.0);
    }
  });
});

describe("AssetTimelineChart — Cost rate/CO2eq rate only render when data is present", () => {
  beforeEach(() => {
    lines.length = 0;
  });

  it("renders no Cost rate/CO2eq rate lines for a fixture with no cost/CO2 data", () => {
    const data = [point(-1 * minute, { power_kw: -3.0 })];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} />);
    const names = lines.map((l) => l.name);
    expect(names).not.toContain("Cost rate [€/h]");
    expect(names).not.toContain("CO₂eq rate [g/h]");
  });

  it("renders Cost rate/CO2eq rate lines when that data is present", () => {
    const data = [point(-1 * minute, { power_kw: -3.0, cost_rate_eur_h: 0.5, co2_rate_g_h: 120 })];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} />);
    const names = lines.map((l) => l.name);
    expect(names).toContain("Cost rate [€/h]");
    expect(names).toContain("CO₂eq rate [g/h]");
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

describe("AssetTimelineChart — clips data to [tMin, tMax]", () => {
  beforeEach(() => {
    composedChartData.length = 0;
  });

  // Regression test: a client with a skewed OS clock (or any stale/late point outside
  // the window) must not have that point reach recharts, where an un-clipped domain
  // would stretch to include it and squeeze the intended window into a sliver.
  it("drops a data point outside the hoursBack/hoursForward window", () => {
    const inWindow = point(-30 * minute, { power_kw: 1.0 });
    const farOutside = point(5 * 60 * minute, { power_kw: 99.0 }); // 5h ahead, hoursForward defaults to 1h
    render(
      <AssetTimelineChart data={[inWindow, farOutside]} color="#000" nowMs={now} />
    );
    expect(composedChartData.some((row) => row.ts === farOutside.ts)).toBe(false);
    expect(composedChartData.some((row) => row.ts === inWindow.ts)).toBe(true);

/**
 * Regression for the Controller PV cell: its two right-hand axes (cost €/h, CO2 g/h) used to
 * render labels like "-0.31275 €/h" because a single-sign domain got no explicit ticks. The
 * chart passes no tick config at all now — rounding comes from TimeSeriesChart.
 */
describe("AssetTimelineChart — rounded Y-axis labels on a PV-shaped cell", () => {
  beforeEach(() => {
    yAxes.length = 0;
  });

  /** Significant digits of a rendered tick label, ignoring sign, decimal point and
   * leading/trailing zeros — "-0.4" → 1, "1.35" → 3, "600" → 1. */
  function significantDigits(label: string): number {
    const digits = label.replace(/[^0-9]/g, "").replace(/^0+/, "").replace(/0+$/, "");
    return digits.length;
  }

  it("labels every axis with round multiples of the axis step", () => {
    // PV: generation is negative power, export revenue is a negative cost rate, avoided
    // emissions a negative CO2 rate — all three axes are single-sign, the case that used to
    // fall through to recharts' raw-domain ticks.
    const data = [
      point(-2 * minute, { power_kw: -3.17, cost_rate_eur_h: -0.4172, co2_rate_g_h: -617.3 }),
      point(-1 * minute, { power_kw: -2.83, cost_rate_eur_h: -0.3311, co2_rate_g_h: -498.1 }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);

    const visible = yAxes.filter((a) => a.ticks !== undefined);
    expect(visible.length).toBe(3);
    for (const axis of visible) {
      const step = (axis.ticks as number[])[1] - (axis.ticks as number[])[0];
      for (const tick of axis.ticks as number[]) {
        expect(Math.abs(tick / step - Math.round(tick / step))).toBeLessThan(1e-6);
      }
    }
  });

  it("keeps the cost/CO2 labels at one or two significant digits for this data", () => {
    const data = [
      point(-2 * minute, { power_kw: -3.17, cost_rate_eur_h: -0.4172, co2_rate_g_h: -617.3 }),
      point(-1 * minute, { power_kw: -2.83, cost_rate_eur_h: -0.3311, co2_rate_g_h: -498.1 }),
    ];
    render(<AssetTimelineChart data={data} color="#000" nowMs={now} pvCurtailment />);

    for (const axis of yAxes.filter((a) => a.ticks !== undefined)) {
      const format = axis.tickFormatter as (v: number) => string;
      for (const tick of axis.ticks as number[]) {
        expect(significantDigits(format(tick))).toBeLessThanOrEqual(2);
      }
    }
  });
});
