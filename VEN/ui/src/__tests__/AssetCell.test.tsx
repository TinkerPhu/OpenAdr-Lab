/**
 * AssetCell — tariff enrichment tests
 *
 * Verifies that history/now-point timeline data lacking cost_rate_eur_h and
 * co2_rate_g_h gets enriched via fillAssetRatesFromTariffs before reaching
 * the AssetTimelineChart. The enrichment is driven by useTariffs() in AssetCell.
 */
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, afterEach } from "vitest";
import { AssetCell } from "../components/controller-v2/AssetCell";
import type { AssetSummary, AssetTimelinePoint } from "../components/controller-v2/types";
import type { SimSnapshot, TariffSnapshot as ApiTariffSnapshot } from "../api/types";

// ─── Mock AssetTimelineChart — expose the data it receives via DOM attrs ──────

vi.mock("../components/controller-v2/charts/AssetTimelineChart", () => ({
  AssetTimelineChart: ({ data }: { data: AssetTimelinePoint[] }) => (
    <div
      data-testid="asset-timeline-chart"
      data-point-count={String(data.length)}
      data-cost-rate-0={String(data[0]?.values?.["cost_rate_eur_h"] ?? "null")}
      data-co2-rate-0={String(data[0]?.values?.["co2_rate_g_h"] ?? "null")}
    />
  ),
}));

// ─── Mutable: changed between tests to control what useTariffs returns ────────

let tariffsData: ApiTariffSnapshot[] = [];

vi.mock("../api/hooks", () => ({
  useTariffs: () => ({ data: tariffsData }),
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

function makeTariff(intervalStartMs: number, importEurKwh: number, co2GKwh: number): ApiTariffSnapshot {
  return {
    interval_start: new Date(intervalStartMs).toISOString(),
    import_tariff_eur_kwh: importEurKwh,
    export_tariff_eur_kwh: null,
    co2_g_kwh: co2GKwh,
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

afterEach(() => {
  tariffsData = [];
});

describe("AssetCell — tariff rate enrichment", () => {
  it("renders the asset cell with correct data-testid", () => {
    renderCell([]);
    expect(screen.getByTestId("asset-cell-ev")).toBeTruthy();
  });

  it("fills cost_rate_eur_h on history point when tariff is available", () => {
    const t0 = new Date("2026-01-01T09:00:00Z").getTime(); // interval starts 1h back
    const t1 = new Date("2026-01-01T09:30:00Z").getTime(); // history point
    tariffsData = [makeTariff(t0, 0.20, 300)];

    const historyPoint: AssetTimelinePoint = { ts: t1, values: { power_kw: 5.0 } };
    renderCell([historyPoint]);

    const costRate = parseFloat(screen.getByTestId("asset-timeline-chart").getAttribute("data-cost-rate-0")!);
    expect(costRate).toBeCloseTo(1.0); // 5.0 kW × 0.20 €/kWh = 1.0 €/h
  });

  it("fills co2_rate_g_h on history point when tariff is available", () => {
    const t0 = new Date("2026-01-01T09:00:00Z").getTime();
    const t1 = new Date("2026-01-01T09:30:00Z").getTime();
    tariffsData = [makeTariff(t0, 0.20, 300)];

    const historyPoint: AssetTimelinePoint = { ts: t1, values: { power_kw: 2.0 } };
    renderCell([historyPoint]);

    const co2Rate = parseFloat(screen.getByTestId("asset-timeline-chart").getAttribute("data-co2-rate-0")!);
    expect(co2Rate).toBeCloseTo(600); // 2.0 kW × 300 g/kWh = 600 g/h
  });

  it("does not overwrite cost_rate_eur_h already set by backend (future plan slot)", () => {
    const t0 = new Date("2026-01-01T09:00:00Z").getTime();
    const t1 = new Date("2026-01-01T09:30:00Z").getTime();
    tariffsData = [makeTariff(t0, 0.20, 300)];

    const planPoint: AssetTimelinePoint = { ts: t1, values: { power_kw: 5.0, cost_rate_eur_h: 0.77 } };
    renderCell([planPoint]);

    const costRate = parseFloat(screen.getByTestId("asset-timeline-chart").getAttribute("data-cost-rate-0")!);
    expect(costRate).toBeCloseTo(0.77);
  });

  it("leaves cost_rate_eur_h as null when no applicable tariff exists", () => {
    tariffsData = []; // no tariffs available
    const historyPoint: AssetTimelinePoint = { ts: Date.now() - 60_000, values: { power_kw: 3.0 } };
    renderCell([historyPoint]);

    const attr = screen.getByTestId("asset-timeline-chart").getAttribute("data-cost-rate-0");
    expect(attr).toBe("null");
  });
});
