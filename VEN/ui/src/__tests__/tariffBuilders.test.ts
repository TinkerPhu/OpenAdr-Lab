import { describe, it, expect } from "vitest";
import {
  buildTariffPricePoints,
  buildPowerPoints,
  fillCostRateFromTariffs,
} from "../components/controller-v2/tariffBuilders";
import type { TariffSnapshot as ApiTariffSnapshot } from "../api/types";
import type { AssetTimelinePoint } from "../components/controller-v2/types";

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

  it("sets totalCostRateEurH and gridPowerKw to null (no power contamination)", () => {
    const result = buildTariffPricePoints([makeTariff()]);
    expect(result[0].totalCostRateEurH).toBeNull();
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

describe("fillCostRateFromTariffs", () => {
  it("fills totalCostRateEurH from preceding import tariff × power_kw", () => {
    const input = [
      { ts: 100, importPriceEurKwh: 0.20, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, gridPowerKw: null },
      { ts: 200, importPriceEurKwh: null, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, gridPowerKw: 3.0 },
    ];
    const result = fillCostRateFromTariffs(input);
    expect(result[1].totalCostRateEurH).toBeCloseTo(0.60);
  });

  it("clamps negative power to 0 (export case)", () => {
    const input = [
      { ts: 100, importPriceEurKwh: 0.20, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, gridPowerKw: null },
      { ts: 200, importPriceEurKwh: null, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, gridPowerKw: -2.0 },
    ];
    const result = fillCostRateFromTariffs(input);
    expect(result[1].totalCostRateEurH).toBe(0);
  });

  it("does not overwrite an already-set totalCostRateEurH", () => {
    const input = [
      { ts: 100, importPriceEurKwh: 0.20, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, gridPowerKw: null },
      { ts: 200, importPriceEurKwh: null, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: 0.99, gridPowerKw: 3.0 },
    ];
    const result = fillCostRateFromTariffs(input);
    expect(result[1].totalCostRateEurH).toBe(0.99);
  });

  it("leaves totalCostRateEurH null when no preceding tariff exists", () => {
    const input = [
      { ts: 200, importPriceEurKwh: null, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, gridPowerKw: 3.0 },
    ];
    const result = fillCostRateFromTariffs(input);
    expect(result[0].totalCostRateEurH).toBeNull();
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
