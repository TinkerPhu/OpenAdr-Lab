/**
 * HistoryPage → AssetTimelineChart: what each asset's chart is handed.
 *
 * The chart itself is mocked so the props can be read. That the chart turns an EV's
 * `plugged` values into a band is AssetTimelineChart.test.tsx's business; this pins the
 * other half — that a persisted history row's `plugged` reaches it, under the same key
 * the live timeline uses, and that the asset's declared shading comes along.
 */
import { render } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { assetChartSpec } from "../components/controller/assetChartSpecs";

const chartProps = vi.hoisted(() => [] as Array<Record<string, unknown>>);

vi.mock("../components/controller/charts/AssetTimelineChart", () => ({
  AssetTimelineChart: (props: Record<string, unknown>) => {
    chartProps.push(props);
    return null;
  },
}));

const T0 = Date.UTC(2026, 0, 1, 6);
const minute = 60_000;
const mockTicks = [
  { ts: T0, asset_id: "ev", power_kw: 0, soc_pct: 42, temperature_c: null, plugged: 0 },
  { ts: T0 + minute, asset_id: "ev", power_kw: 0, soc_pct: 42, temperature_c: null, plugged: 0.5 },
  // A row from before the column existed: no value, so nothing to shade.
  { ts: T0 + 2 * minute, asset_id: "ev", power_kw: 0, soc_pct: 42, temperature_c: null, plugged: null },
  { ts: T0, asset_id: "heater", power_kw: 1.2, soc_pct: null, temperature_c: 55, plugged: null },
  { ts: T0, asset_id: "wm", power_kw: 0.4, soc_pct: null, temperature_c: null, plugged: null },
];

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useHistoryTicks: () => ({ data: mockTicks, refetch: vi.fn() }),
  useHistoryGrid: () => ({ data: [], refetch: vi.fn() }),
  useHistoryEvents: () => ({ data: { rows: [], total: 0 }, refetch: vi.fn() }),
  useHistoryReports: () => ({ data: { rows: [], total: 0 }, refetch: vi.fn() }),
  useHistoryForecastAccuracy: () => ({ data: [], refetch: vi.fn() }),
  useHealth: () => ({ data: { server_time: new Date(T0 + 60 * minute).toISOString() } }),
}));

vi.mock("../api/venContext", () => ({
  useVenContext: () => ({ venUrl: "http://localhost:8081", venName: "ven-1", setVenUrl: vi.fn(), api: {} }),
}));

import { HistoryPage } from "../pages/History";

type Point = { ts: number; values: Record<string, number> };

function propsFor(color: string) {
  const found = chartProps.filter((p) => p.color === color);
  return found[found.length - 1];
}

beforeEach(() => {
  chartProps.length = 0;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <HistoryPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
});

describe("HistoryPage — per-asset chart props", () => {
  it("passes a history row's plugged value through under the timeline's own key", () => {
    const ev = propsFor("#2196F3");
    const data = ev.data as Point[];
    expect(data.map((p) => p.values.plugged)).toEqual([0, 0.5, undefined]);
    expect("plugged" in data[2].values).toBe(false);
  });

  it("hands the EV chart its declared shading and a gap limit", () => {
    const ev = propsFor("#2196F3");
    expect(ev.shadings).toBe(assetChartSpec("ev").shadings);
    expect(ev.shadingMaxGapMs).toBe(5 * minute);
  });

  it("takes each asset's state line from its declaration, not from the data", () => {
    expect(propsFor("#2196F3").stateKey).toBe("soc");
    expect(propsFor("#FF5722").stateKey).toBe("temp_c");
    // A shiftable load declares neither a state line nor a shading.
    const wm = propsFor("#FF9800");
    expect(wm.stateKey).toBeUndefined();
    expect(wm.shadings).toBeUndefined();
  });
});
