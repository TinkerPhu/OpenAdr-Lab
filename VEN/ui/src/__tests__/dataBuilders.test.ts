/**
 * dataBuilders — computeForecastEnergy edge cases
 *
 * computeForecastEnergy is private; exercised via the exported deriveAssetSummaries.
 * All tests check result[0].forecastEnergyKwh (the "ev" asset summary).
 */
import { describe, it, expect } from "vitest";
import { deriveAssetSummaries, computeCostRateEurH } from "../components/controller-v2/dataBuilders";
import type { SimSnapshot, TariffSnapshot as ApiTariffSnapshot } from "../api/types";
import type { AssetTimelinePoint } from "../components/controller-v2/types";

// ─── Fixtures ─────────────────────────────────────────────────────────────────

const NOW = 1_000_000_000_000; // arbitrary epoch ms anchor
const H = 3_600_000;           // one hour in ms

const TARIFF: ApiTariffSnapshot = {
  interval_start: new Date(NOW - H).toISOString(),
  import_tariff_eur_kwh: 0.25,
  export_tariff_eur_kwh: 0.05,
  co2_g_kwh: 300,
};

/** Minimal SimSnapshot with just an ev asset */
const sim: SimSnapshot = {
  ts: "2026-01-01T10:00:00Z",
  grid: { net_power_w: 0, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
  assets: { ev: { power_kw: 0, soc: 0.5 } },
};

function makePoint(ts: number, power_kw?: number): AssetTimelinePoint {
  return { ts, values: power_kw !== undefined ? { power_kw } : {} };
}

/** Run deriveAssetSummaries with an ev-only timeline and return ev's forecastEnergyKwh */
function forecastFor(evPoints: AssetTimelinePoint[]): number | null {
  const summaries = deriveAssetSummaries(sim, [], [], { ev: evPoints }, NOW);
  const ev = summaries.find((s) => s.assetId === "ev");
  return ev?.forecastEnergyKwh ?? null;
}

// ─── Tests ────────────────────────────────────────────────────────────────────

// ─── computeCostRateEurH ──────────────────────────────────────────────────────

describe("computeCostRateEurH", () => {
  it("import: applies import tariff to positive power", () => {
    expect(computeCostRateEurH(4.0, TARIFF)).toBeCloseTo(4.0 * 0.25);
  });

  it("export: applies export tariff to negative power (negative = revenue)", () => {
    expect(computeCostRateEurH(-3.0, TARIFF)).toBeCloseTo(-3.0 * 0.05);
  });

  it("zero power gives zero", () => {
    expect(computeCostRateEurH(0.0, TARIFF)).toBe(0.0);
  });

  it("null tariff gives zero regardless of power", () => {
    expect(computeCostRateEurH(4.0, null)).toBe(0.0);
    expect(computeCostRateEurH(-3.0, null)).toBe(0.0);
  });

  it("null import field falls back to zero", () => {
    const t: ApiTariffSnapshot = { ...TARIFF, import_tariff_eur_kwh: null };
    expect(computeCostRateEurH(4.0, t)).toBe(0.0);
  });

  it("null export field falls back to zero", () => {
    const t: ApiTariffSnapshot = { ...TARIFF, export_tariff_eur_kwh: null };
    expect(computeCostRateEurH(-3.0, t)).toBe(0.0);
  });
});

// ─── deriveAssetSummaries — gridFraction cost allocation ──────────────────────

describe("deriveAssetSummaries — gridFraction", () => {
  it("battery charging fully from PV shows zero cost rate", () => {
    // PV 4 kW, battery charging 3 kW, base_load 1 kW → grid net = 0
    const pvSim: SimSnapshot = {
      ts: new Date(NOW).toISOString(),
      grid: { net_power_w: 0, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
      assets: {
        pv:        { power_kw: -4.0, soc: null },
        battery:   { power_kw: 3.0,  soc: 0.5  },
        base_load: { power_kw: 1.0,  soc: null },
      },
    };
    const summaries = deriveAssetSummaries(pvSim, [TARIFF], [], {}, NOW);
    const battery = summaries.find((s) => s.assetId === "battery")!;
    const baseLoad = summaries.find((s) => s.assetId === "base_load")!;
    expect(battery.costRateEurH).toBeCloseTo(0.0);
    expect(baseLoad.costRateEurH).toBeCloseTo(0.0);
  });

  it("partial PV coverage scales cost proportionally", () => {
    // PV 2 kW, battery charging 3 kW, base_load 1 kW → grid import 2 kW
    // gridFraction = 2 / (3+1) = 0.5
    const pvSim: SimSnapshot = {
      ts: new Date(NOW).toISOString(),
      grid: { net_power_w: 2000, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
      assets: {
        pv:        { power_kw: -2.0, soc: null },
        battery:   { power_kw: 3.0,  soc: 0.5  },
        base_load: { power_kw: 1.0,  soc: null },
      },
    };
    const summaries = deriveAssetSummaries(pvSim, [TARIFF], [], {}, NOW);
    const battery = summaries.find((s) => s.assetId === "battery")!;
    // 3 kW × 0.5 × 0.25 = 0.375 EUR/h
    expect(battery.costRateEurH).toBeCloseTo(3.0 * 0.5 * 0.25);
  });

  it("no PV: full grid import charges at import tariff", () => {
    const gridSim: SimSnapshot = {
      ts: new Date(NOW).toISOString(),
      grid: { net_power_w: 4000, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
      assets: {
        battery:   { power_kw: 3.0, soc: 0.5  },
        base_load: { power_kw: 1.0, soc: null },
      },
    };
    const summaries = deriveAssetSummaries(gridSim, [TARIFF], [], {}, NOW);
    const battery = summaries.find((s) => s.assetId === "battery")!;
    // gridFraction = 4/4 = 1 → 3 kW × 0.25 = 0.75 EUR/h
    expect(battery.costRateEurH).toBeCloseTo(3.0 * 0.25);
  });

  it("asset cost rates sum to grid total cost rate", () => {
    // Any scenario: sum of consuming asset cost rates must equal grid cost rate
    const pvSim: SimSnapshot = {
      ts: new Date(NOW).toISOString(),
      grid: { net_power_w: 1500, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
      assets: {
        pv:        { power_kw: -2.5, soc: null },
        battery:   { power_kw: 2.0,  soc: 0.5  },
        base_load: { power_kw: 2.0,  soc: null },
      },
    };
    const summaries = deriveAssetSummaries(pvSim, [TARIFF], [], {}, NOW);
    const consuming = summaries.filter((s) => s.costRateEurH >= 0 && s.assetId !== "pv");
    const total = consuming.reduce((s, a) => s + a.costRateEurH, 0);
    const gridCostRate = 1.5 * 0.25; // 1.5 kW import × 0.25
    expect(total).toBeCloseTo(gridCostRate);
  });
});

// ─── computeForecastEnergy via deriveAssetSummaries ───────────────────────────

describe("computeForecastEnergy via deriveAssetSummaries", () => {
  it("returns null when allTimelines is empty (no ev key)", () => {
    const summaries = deriveAssetSummaries(sim, [], [], {}, NOW);
    const ev = summaries.find((s) => s.assetId === "ev");
    expect(ev?.forecastEnergyKwh).toBeNull();
  });

  it("returns null when all points are in the past", () => {
    const points = [
      makePoint(NOW - 2 * H, 10),
      makePoint(NOW - 1 * H, 10),
    ];
    expect(forecastFor(points)).toBeNull();
  });

  it("returns null when future points have no power_kw key", () => {
    const points = [
      makePoint(NOW + 1 * H),  // empty values: {}
      makePoint(NOW + 2 * H),
    ];
    expect(forecastFor(points)).toBeNull();
  });

  it("returns 0 for a single future point (prevGap=0 for last point)", () => {
    const points = [makePoint(NOW + 1 * H, 10)];
    expect(forecastFor(points)).toBe(0);
  });

  it("returns 20 kWh for two future points each 1h apart at 10 kW", () => {
    // i=0: duration = (+2h)-(+1h) = 1h → 10 kWh
    // i=1 (last): prevGap = 1h → 10 kWh
    // total = 20 kWh
    const points = [
      makePoint(NOW + 1 * H, 10),
      makePoint(NOW + 2 * H, 10),
    ];
    expect(forecastFor(points)).toBeCloseTo(20, 6);
  });

  it("returns 12 kWh for three future points each 1h apart at 4 kW (prevGap reuse)", () => {
    // i=0: duration=1h → 4 kWh; i=1: duration=1h → 4 kWh; i=2 (last): prevGap=1h → 4 kWh
    const points = [
      makePoint(NOW + 1 * H, 4),
      makePoint(NOW + 2 * H, 4),
      makePoint(NOW + 3 * H, 4),
    ];
    expect(forecastFor(points)).toBeCloseTo(12, 6);
  });

  it("only counts future points when mixed past+future", () => {
    // Past point at -1h (ignored), two future points 1h apart at 10 kW → 20 kWh
    const points = [
      makePoint(NOW - 1 * H, 10),
      makePoint(NOW + 1 * H, 10),
      makePoint(NOW + 2 * H, 10),
    ];
    expect(forecastFor(points)).toBeCloseTo(20, 6);
  });

  it("uses Math.abs for negative power (export scenario)", () => {
    // Two future points at -5 kW, 1h apart → abs(-5)*1 + abs(-5)*1 = 10 kWh
    const points = [
      makePoint(NOW + 1 * H, -5),
      makePoint(NOW + 2 * H, -5),
    ];
    expect(forecastFor(points)).toBeCloseTo(10, 6);
  });

  it("skips points with missing power_kw and counts remaining", () => {
    // [+1h power=4, +2h no key, +3h power=4]
    // i=0: power=4, duration=1h → 4 kWh
    // i=1: no power_kw → skipped
    // i=2 (last): power=4, prevGap=future[2].ts-future[1].ts=1h → 4 kWh
    // total = 8 kWh
    const points = [
      makePoint(NOW + 1 * H, 4),
      makePoint(NOW + 2 * H),       // missing power_kw
      makePoint(NOW + 3 * H, 4),
    ];
    expect(forecastFor(points)).toBeCloseTo(8, 6);
  });

  it("skips points with values: null (empty grid buckets)", () => {
    // [+1h power=4, +2h null, +3h power=4]
    // i=0: power=4, duration=1h → 4 kWh
    // i=1: values null → skipped
    // i=2 (last): power=4, prevGap=1h → 4 kWh
    // total = 8 kWh
    const points: AssetTimelinePoint[] = [
      makePoint(NOW + 1 * H, 4),
      { ts: NOW + 2 * H, values: null },
      makePoint(NOW + 3 * H, 4),
    ];
    expect(forecastFor(points)).toBeCloseTo(8, 6);
  });
});
