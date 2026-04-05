/**
 * AssetCell — rendering tests
 *
 * Rate enrichment (gridFraction-based cost_rate_eur_h / co2_rate_g_h) was moved to
 * ControllerPage via enrichAllAssetTimelines so the full allTimelines context is available.
 * AssetCell is now a pure display component: it renders whatever timePoints it receives.
 * Enrichment logic is tested in tariffBuilders.test.ts.
 */
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { AssetCell } from "../components/controller/AssetCell";
import type { AssetSummary, AssetTimelinePoint } from "../components/controller/types";
import type { SimSnapshot } from "../api/types";

// ─── Mock AssetTimelineChart — expose the data it receives via DOM attrs ──────

vi.mock("../components/controller/charts/AssetTimelineChart", () => ({
  AssetTimelineChart: ({ data }: { data: AssetTimelinePoint[] }) => (
    <div
      data-testid="asset-timeline-chart"
      data-point-count={String(data.length)}
      data-cost-rate-0={String(data[0]?.values?.["cost_rate_eur_h"] ?? "null")}
    />
  ),
}));

vi.mock("../api/hooks", () => ({
  useSimSchema: () => ({ data: {} }),
}));

// ─── Fixtures ─────────────────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-01-01T10:00:00Z",
  grid: { net_power_w: 0, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
  assets: {},
};

function makeAssetSummary(assetId: "ev"): AssetSummary {
  return {
    assetId,
    label: "EV",
    color: "#2196F3",
    powerKw: 3.0,
    costRateEurH: 0.6,
    co2RateGH: 900,
    socPct: 50,
    forecastEnergyKwh: null,
    activeRequest: null,
  };
}

function makeQC() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function renderCell(timePoints: AssetTimelinePoint[]) {
  return render(
    <QueryClientProvider client={makeQC()}>
      <AssetCell
        assetId="ev"
        summary={makeAssetSummary("ev")}
        simSnapshot={baseSim}
        simOverrides={undefined}
        collapsed={{ left: false, right: true }}
        timePoints={timePoints}
        nowMs={Date.now()}
        extended={false}
        pinned={false}
        onTogglePin={vi.fn()}
        onToggleCollapse={vi.fn()}
        onOverrideChange={vi.fn()}
        onResetSoc={vi.fn()}
      />
    </QueryClientProvider>
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("AssetCell — rendering", () => {
  it("renders the asset cell with correct data-testid", () => {
    renderCell([]);
    expect(screen.getByTestId("asset-cell-ev")).toBeTruthy();
  });

  it("passes timePoints through unchanged to AssetTimelineChart", () => {
    const point: AssetTimelinePoint = {
      ts: Date.now(),
      values: { power_kw: 3.0, cost_rate_eur_h: 0.77 },
    };
    renderCell([point]);
    const costRate = parseFloat(
      screen.getByTestId("asset-timeline-chart").getAttribute("data-cost-rate-0")!
    );
    expect(costRate).toBeCloseTo(0.77);
  });

  it("passes pre-enriched null cost_rate through without modification", () => {
    // Enrichment happens upstream in ControllerPage — AssetCell does not fill in rates.
    const point: AssetTimelinePoint = { ts: Date.now(), values: { power_kw: 3.0 } };
    renderCell([point]);
    const attr = screen.getByTestId("asset-timeline-chart").getAttribute("data-cost-rate-0");
    expect(attr).toBe("null");
  });
});
