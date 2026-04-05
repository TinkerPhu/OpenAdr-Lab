import { describe, it, expect } from "vitest";
import {
  buildTariffPricePoints,
  buildPowerPoints,
  fillCostRateFromTariffs,
  enrichAllAssetTimelines,
} from "../components/controller/tariffBuilders";
import type { TariffSnapshot as ApiTariffSnapshot } from "../api/types";
import type { AssetTimelinePoint } from "../components/controller/types";

// ─── buildTariffPricePoints ───────────────────────────────────────────────────

describe("buildTariffPricePoints", () => {
  function makeTariff(overrides: Partial<ApiTariffSnapshot> = {}): ApiTariffSnapshot {
    return {
      interval_start: "2026-01-01T10:00:00Z",
      import_tariff_eur_kwh: 0.25,
      export_tariff_eur_kwh: 0.10,
      co2_g_kwh: 350,
      ...overrides,
    };
  }

  it("converts ISO interval_start to epoch ms", () => {
    const result = buildTariffPricePoints([makeTariff()]);
    expect(result[0].ts).toBe(new Date("2026-01-01T10:00:00Z").getTime());
  });

  it("maps co2_g_kwh to co2GKwh (regression guard for old co2_rate_g_h bug)", () => {
    const result = buildTariffPricePoints([makeTariff({ co2_g_kwh: 420 })]);
    expect(result[0].co2GKwh).toBe(420);
  });

  it("preserves null import_tariff_eur_kwh as null", () => {
    const result = buildTariffPricePoints([makeTariff({ import_tariff_eur_kwh: null })]);
    expect(result[0].importPriceEurKwh).toBeNull();
  });

  it("preserves null export_tariff_eur_kwh as null", () => {
    const result = buildTariffPricePoints([makeTariff({ export_tariff_eur_kwh: null })]);
    expect(result[0].exportPriceEurKwh).toBeNull();
  });

  it("sets totalCostRateEurH, totalCo2RateGH and gridPowerKw to null (no power contamination)", () => {
    const result = buildTariffPricePoints([makeTariff()]);
    expect(result[0].totalCostRateEurH).toBeNull();
    expect(result[0].totalCo2RateGH).toBeNull();
    expect(result[0].gridPowerKw).toBeNull();
  });

  it("maps multiple inputs to one point each in order", () => {
    const t1 = makeTariff({ interval_start: "2026-01-01T10:00:00Z" });
    const t2 = makeTariff({ interval_start: "2026-01-01T11:00:00Z" });
    const result = buildTariffPricePoints([t1, t2]);
    expect(result).toHaveLength(2);
    expect(result[0].ts).toBe(new Date("2026-01-01T10:00:00Z").getTime());
    expect(result[1].ts).toBe(new Date("2026-01-01T11:00:00Z").getTime());
  });
});

// ─── buildPowerPoints ─────────────────────────────────────────────────────────

describe("buildPowerPoints", () => {
  function makePoint(overrides: Partial<AssetTimelinePoint> = {}): AssetTimelinePoint {
    return {
      ts: 1000,
      values: { cost_rate_eur_h: 0.5, power_kw: 3.2 },
      ...overrides,
    };
  }

  it("maps cost_rate_eur_h to totalCostRateEurH", () => {
    const result = buildPowerPoints([makePoint()]);
    expect(result[0].totalCostRateEurH).toBe(0.5);
  });

  it("maps power_kw to gridPowerKw", () => {
    const result = buildPowerPoints([makePoint()]);
    expect(result[0].gridPowerKw).toBe(3.2);
  });

  it("sets totalCostRateEurH to null when cost_rate_eur_h is missing", () => {
    const result = buildPowerPoints([makePoint({ values: { power_kw: 1.0 } })]);
    expect(result[0].totalCostRateEurH).toBeNull();
  });

  it("maps co2_rate_g_h to totalCo2RateGH", () => {
    const result = buildPowerPoints([makePoint({ values: { co2_rate_g_h: 900, power_kw: 3.0 } })]);
    expect(result[0].totalCo2RateGH).toBe(900);
  });

  it("sets totalCo2RateGH to null when co2_rate_g_h is missing", () => {
    const result = buildPowerPoints([makePoint({ values: { power_kw: 1.0 } })]);
    expect(result[0].totalCo2RateGH).toBeNull();
  });

  it("sets importPriceEurKwh, exportPriceEurKwh, co2GKwh to null (no price contamination)", () => {
    const result = buildPowerPoints([makePoint()]);
    expect(result[0].importPriceEurKwh).toBeNull();
    expect(result[0].exportPriceEurKwh).toBeNull();
    expect(result[0].co2GKwh).toBeNull();
  });

  it("passes ts through unchanged", () => {
    const result = buildPowerPoints([makePoint({ ts: 99999 })]);
    expect(result[0].ts).toBe(99999);
  });
});

// ─── fillCostRateFromTariffs ──────────────────────────────────────────────────

function makeTariffSnapshot(
  intervalStartMs: number,
  importEurKwh: number | null,
  co2GKwh: number | null = null,
  exportEurKwh: number | null = null
): ApiTariffSnapshot {
  return {
    interval_start: new Date(intervalStartMs).toISOString(),
    import_tariff_eur_kwh: importEurKwh,
    export_tariff_eur_kwh: exportEurKwh,
    co2_g_kwh: co2GKwh,
  };
}

function makePoint(ts: number, overrides: Partial<ReturnType<typeof buildTariffPricePoints>[0]> = {}) {
  return {
    ts,
    importPriceEurKwh: null,
    exportPriceEurKwh: null,
    co2GKwh: null,
    totalCostRateEurH: null,
    totalCo2RateGH: null,
    gridPowerKw: null,
    ...overrides,
  };
}

describe("fillCostRateFromTariffs", () => {
  it("fills totalCostRateEurH from applicable import tariff × power_kw (import)", () => {
    const tariffs = [makeTariffSnapshot(100, 0.20)];
    const merged = [
      makePoint(100, { importPriceEurKwh: 0.20 }),
      makePoint(200, { gridPowerKw: 3.0 }),
    ];
    const result = fillCostRateFromTariffs(merged, tariffs);
    expect(result[1].totalCostRateEurH).toBeCloseTo(0.60);
  });

  it("fills totalCostRateEurH with export tariff × power_kw (negative = revenue)", () => {
    const tariffs = [makeTariffSnapshot(100, 0.20, null, 0.08)];
    const merged = [
      makePoint(100, { importPriceEurKwh: 0.20 }),
      makePoint(200, { gridPowerKw: -2.0 }),
    ];
    const result = fillCostRateFromTariffs(merged, tariffs);
    expect(result[1].totalCostRateEurH).toBeCloseTo(-2.0 * 0.08); // −0.16
  });

  it("fills totalCo2RateGH as power × co2 intensity (positive when importing)", () => {
    const tariffs = [makeTariffSnapshot(100, 0.20, 300)];
    const merged = [
      makePoint(100, { importPriceEurKwh: 0.20 }),
      makePoint(200, { gridPowerKw: 4.0 }),
    ];
    const result = fillCostRateFromTariffs(merged, tariffs);
    expect(result[1].totalCo2RateGH).toBeCloseTo(1200); // 4 × 300
  });

  it("fills totalCo2RateGH as negative when exporting (displaced emissions)", () => {
    const tariffs = [makeTariffSnapshot(100, 0.20, 300, 0.08)];
    const merged = [
      makePoint(100, { importPriceEurKwh: 0.20 }),
      makePoint(200, { gridPowerKw: -3.0 }),
    ];
    const result = fillCostRateFromTariffs(merged, tariffs);
    expect(result[1].totalCo2RateGH).toBeCloseTo(-900); // −3 × 300
  });

  it("leaves cost rate null when no export tariff and power is negative", () => {
    const tariffs = [makeTariffSnapshot(100, 0.20)]; // no export tariff
    const merged = [
      makePoint(100, { importPriceEurKwh: 0.20 }),
      makePoint(200, { gridPowerKw: -2.0 }),
    ];
    const result = fillCostRateFromTariffs(merged, tariffs);
    expect(result[1].totalCostRateEurH).toBeNull();
  });

  it("does not overwrite an already-set totalCostRateEurH", () => {
    const tariffs = [makeTariffSnapshot(100, 0.20)];
    const merged = [
      makePoint(200, { totalCostRateEurH: 0.99, totalCo2RateGH: 99, gridPowerKw: 3.0 }),
    ];
    const result = fillCostRateFromTariffs(merged, tariffs);
    expect(result[0].totalCostRateEurH).toBe(0.99);
    expect(result[0].totalCo2RateGH).toBe(99);
  });

  it("leaves totalCostRateEurH null when no applicable tariff exists", () => {
    const tariffs: ApiTariffSnapshot[] = [];
    const merged = [
      makePoint(200, { gridPowerKw: 3.0 }),
    ];
    const result = fillCostRateFromTariffs(merged, tariffs);
    expect(result[0].totalCostRateEurH).toBeNull();
    expect(result[0].totalCo2RateGH).toBeNull();
  });
});

// ─── enrichAllAssetTimelines ──────────────────────────────────────────────────

function makeAllTimelines(
  evKw: number,
  pvKw: number,
  gridKw: number,
  ts = 200
): Record<string, AssetTimelinePoint[]> {
  return {
    ev: [{ ts, values: { power_kw: evKw } }],
    heater: [{ ts, values: { power_kw: 0 } }],
    pv: [{ ts, values: { power_kw: pvKw } }],
    battery: [{ ts, values: { power_kw: 0 } }],
    base_load: [{ ts, values: { power_kw: 0 } }],
    grid: [{ ts, values: { power_kw: gridKw } }],
  };
}

describe("enrichAllAssetTimelines", () => {
  it("full grid import: EV at 100% gridFraction → cost = power × import_tariff", () => {
    // EV draws 4 kW, all from grid (PV = 0)
    const tariffs = [makeTariffSnapshot(100, 0.20, 300)];
    const timelines = makeAllTimelines(4.0, 0, 4.0);
    const result = enrichAllAssetTimelines(timelines, tariffs);
    expect(result["ev"][0].values?.["cost_rate_eur_h"]).toBeCloseTo(0.80); // 4 × 0.20
    expect(result["ev"][0].values?.["co2_rate_g_h"]).toBeCloseTo(1200);    // 4 × 300
  });

  it("PV covers EV fully: gridFraction = 0 → EV cost = 0", () => {
    // EV draws 3 kW, PV generates 3 kW, grid import = 0
    const tariffs = [makeTariffSnapshot(100, 0.20, 300)];
    const timelines = makeAllTimelines(3.0, -3.0, 0);
    const result = enrichAllAssetTimelines(timelines, tariffs);
    expect(result["ev"][0].values?.["cost_rate_eur_h"]).toBeCloseTo(0);
    expect(result["ev"][0].values?.["co2_rate_g_h"]).toBeCloseTo(0);
  });

  it("PV covers EV partially: gridFraction = 0.5 → EV cost halved", () => {
    // EV draws 4 kW, PV generates 2 kW, grid import = 2 kW
    const tariffs = [makeTariffSnapshot(100, 0.20, 300)];
    const timelines = makeAllTimelines(4.0, -2.0, 2.0);
    const result = enrichAllAssetTimelines(timelines, tariffs);
    // gridFraction = 2/4 = 0.5; effectiveKw = 4 × 0.5 = 2; cost = 2 × 0.20 = 0.40
    expect(result["ev"][0].values?.["cost_rate_eur_h"]).toBeCloseTo(0.40);
  });

  it("PV export: negative cost_rate (revenue) using export_tariff", () => {
    // PV generates 5 kW → power_kw = -5; export tariff = 0.06
    const tariffs = [{
      interval_start: new Date(100).toISOString(),
      import_tariff_eur_kwh: 0.20,
      export_tariff_eur_kwh: 0.06,
      co2_g_kwh: 300,
    }];
    const timelines = makeAllTimelines(0, -5.0, -5.0);
    const result = enrichAllAssetTimelines(timelines, tariffs);
    // pv power_kw = -5; cost = -5 × 0.06 = -0.30
    expect(result["pv"][0].values?.["cost_rate_eur_h"]).toBeCloseTo(-0.30);
    expect(result["pv"][0].values?.["co2_rate_g_h"]).toBeCloseTo(-1500);   // -5 × 300
  });

  it("does not overwrite cost_rate_eur_h already set by backend (plan slot)", () => {
    const tariffs = [makeTariffSnapshot(100, 0.20, 300)];
    const timelines: Record<string, AssetTimelinePoint[]> = {
      ev: [{ ts: 200, values: { power_kw: 4.0, cost_rate_eur_h: 0.55 } }],
      heater: [], pv: [], battery: [], base_load: [], grid: [{ ts: 200, values: { power_kw: 4.0 } }],
    };
    const result = enrichAllAssetTimelines(timelines, tariffs);
    expect(result["ev"][0].values?.["cost_rate_eur_h"]).toBe(0.55);
  });

  it("leaves rates absent when no applicable tariff exists", () => {
    const timelines = makeAllTimelines(4.0, 0, 4.0);
    const result = enrichAllAssetTimelines(timelines, []);
    expect(result["ev"][0].values?.["cost_rate_eur_h"]).toBeUndefined();
    expect(result["ev"][0].values?.["co2_rate_g_h"]).toBeUndefined();
  });

  it("snaps sub-1W effective power to 0 (nearly-full PV coverage)", () => {
    // EV draws 4 kW; PV nearly covers it — only 0.0008 kW imported from grid
    // gridFraction = 0.0008/4 = 0.0002 → effectiveKw = 4 × 0.0002 = 0.0008 kW < NEAR_ZERO_KW
    const tariffs = [makeTariffSnapshot(100, 0.20, 300)];
    const timelines: Record<string, AssetTimelinePoint[]> = {
      ev:        [{ ts: 200, values: { power_kw: 4.0 } }],
      heater:    [{ ts: 200, values: { power_kw: 0.0 } }],
      pv:        [{ ts: 200, values: { power_kw: -4.0 } }],
      battery:   [{ ts: 200, values: { power_kw: 0.0 } }],
      base_load: [{ ts: 200, values: { power_kw: 0.0 } }],
      grid:      [{ ts: 200, values: { power_kw: 0.0008 } }],
    };
    const result = enrichAllAssetTimelines(timelines, tariffs);
    expect(result["ev"][0].values?.["cost_rate_eur_h"]).toBe(0);
    expect(result["ev"][0].values?.["co2_rate_g_h"]).toBe(0);
  });

  it("snaps sub-1W export power to 0 (tiny PV trickle)", () => {
    // PV produces 0.0005 kW (half a watt — below snap threshold)
    const tariffs = [{
      interval_start: new Date(100).toISOString(),
      import_tariff_eur_kwh: 0.20,
      export_tariff_eur_kwh: 0.06,
      co2_g_kwh: 300,
    }];
    const timelines = makeAllTimelines(0, -0.0005, -0.0005);
    const result = enrichAllAssetTimelines(timelines, tariffs);
    expect(result["pv"][0].values?.["cost_rate_eur_h"]).toBe(0);
    expect(result["pv"][0].values?.["co2_rate_g_h"]).toBe(0);
  });

  it("passes grid timeline through unchanged", () => {
    const tariffs = [makeTariffSnapshot(100, 0.20, 300)];
    const timelines = makeAllTimelines(4.0, 0, 4.0);
    const result = enrichAllAssetTimelines(timelines, tariffs);
    expect(result["grid"]).toBe(timelines["grid"]); // same reference
  });
});

// ─── merge + sort ─────────────────────────────────────────────────────────────

describe("merge and sort tariff + power points", () => {
  it("sorts combined points by ts", () => {
    const tariffPoints = buildTariffPricePoints([
      {
        interval_start: new Date(100).toISOString(),
        import_tariff_eur_kwh: 0.2,
        export_tariff_eur_kwh: null,
        co2_g_kwh: null,
      },
      {
        interval_start: new Date(300).toISOString(),
        import_tariff_eur_kwh: 0.3,
        export_tariff_eur_kwh: null,
        co2_g_kwh: null,
      },
    ]);
    const powerPoints = buildPowerPoints([{ ts: 200, values: { power_kw: 5.0 } }]);
    const merged = [...tariffPoints, ...powerPoints].sort((a, b) => a.ts - b.ts);

    expect(merged.map((p) => p.ts)).toEqual([100, 200, 300]);
  });

  it("tariff point has price fields set and null power; power point has power fields set and null prices", () => {
    const tariffPoints = buildTariffPricePoints([
      {
        interval_start: new Date(100).toISOString(),
        import_tariff_eur_kwh: 0.2,
        export_tariff_eur_kwh: 0.1,
        co2_g_kwh: 350,
      },
    ]);
    const powerPoints = buildPowerPoints([
      { ts: 200, values: { cost_rate_eur_h: 0.6, power_kw: 4.0 } },
    ]);
    const merged = [...tariffPoints, ...powerPoints].sort((a, b) => a.ts - b.ts);

    // t=100: tariff point
    expect(merged[0].importPriceEurKwh).toBe(0.2);
    expect(merged[0].exportPriceEurKwh).toBe(0.1);
    expect(merged[0].co2GKwh).toBe(350);
    expect(merged[0].totalCostRateEurH).toBeNull();
    expect(merged[0].gridPowerKw).toBeNull();

    // t=200: power point
    expect(merged[1].totalCostRateEurH).toBe(0.6);
    expect(merged[1].gridPowerKw).toBe(4.0);
    expect(merged[1].importPriceEurKwh).toBeNull();
    expect(merged[1].exportPriceEurKwh).toBeNull();
    expect(merged[1].co2GKwh).toBeNull();
  });
});
