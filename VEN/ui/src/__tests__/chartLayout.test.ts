import { describe, it, expect } from "vitest";
import {
  CELL_CHART_HEIGHT,
  DIAGNOSTIC_CHART_HEIGHT,
} from "../components/charts/chartLayout";
import { SERIES_COLORS, ASSET_COLORS } from "../components/controller/types";

describe("chart sizing contract", () => {
  it("dashboard-cell height and diagnostic height are distinct named constants", () => {
    expect(DIAGNOSTIC_CHART_HEIGHT).not.toBe(CELL_CHART_HEIGHT);
    expect(DIAGNOSTIC_CHART_HEIGHT).toBe(260);
    expect(CELL_CHART_HEIGHT).toBe(140);
  });
});

describe("shared series color registry", () => {
  it("has exactly one color per known non-asset series key", () => {
    expect(SERIES_COLORS.import_tariff).toBeTruthy();
    expect(SERIES_COLORS.export_tariff).toBeTruthy();
    expect(SERIES_COLORS.cost_rate).toBeTruthy();
    expect(SERIES_COLORS.co2_rate).toBeTruthy();
    expect(SERIES_COLORS.grid_line).toBeTruthy();
    expect(SERIES_COLORS.power).toBeTruthy();
  });

  it("does not collide with asset colors for shared concepts", () => {
    // import_tariff and export_tariff are distinct from every asset color
    const assetHexes = new Set(Object.values(ASSET_COLORS));
    expect(assetHexes.has(SERIES_COLORS.import_tariff)).toBe(false);
    expect(assetHexes.has(SERIES_COLORS.export_tariff)).toBe(false);
  });
});
