import { describe, it, expect } from "vitest";
import { fleetChartWindow } from "../components/fleetChartWindow";

const NOW = Date.parse("2026-10-18T12:00:00Z");

describe("fleetChartWindow", () => {
  it("looks back from now by default, as the Fleet page always has", () => {
    expect(fleetChartWindow(NOW, 60)).toEqual({ tMin: NOW - 3_600_000, tMax: NOW });
  });

  it("can also look ahead, for a tariff that has not happened yet", () => {
    expect(fleetChartWindow(NOW, 360, 1080)).toEqual({
      tMin: NOW - 6 * 3_600_000,
      tMax: NOW + 18 * 3_600_000,
    });
  });
});
