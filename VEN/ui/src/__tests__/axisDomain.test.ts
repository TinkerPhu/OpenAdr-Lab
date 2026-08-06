import { describe, it, expect } from "vitest";
import {
  minSpanDomain,
  MIN_POWER_SPAN_KW,
  formatPowerTick,
} from "../components/controller/charts/axisDomain";

describe("minSpanDomain", () => {
  it("expands a near-zero toggling series to the minimum span, centered on the data", () => {
    // The exact bug this guards against: EV cost-rate flickering between 0 and
    // 0.00034 €/h (control-loop residual noise) — found via the phase 3/4 review.
    const values = [0, 0.00034, 0, 0.00035, 0, 0.00043];
    const [min, max] = minSpanDomain(values, 0.05);
    expect(max - min).toBeCloseTo(0.05, 9);
    // Domain must still contain every data point.
    expect(min).toBeLessThanOrEqual(0);
    expect(max).toBeGreaterThanOrEqual(0.00043);
  });

  it("leaves a real, wide swing untouched (does not compress genuine signal)", () => {
    const values = [0, 4.4, 0, 2.1];
    const [min, max] = minSpanDomain(values, 0.05);
    expect(min).toBe(0);
    expect(max).toBe(4.4);
  });

  it("always includes 0 in the domain even when all values share one sign", () => {
    const [min, max] = minSpanDomain([0.02, 0.03], 0.05);
    expect(min).toBeLessThanOrEqual(0);
    expect(max).toBeGreaterThanOrEqual(0.03);
  });

  it("handles negative-only series (export revenue) symmetrically", () => {
    const values = [-0.5, -0.2, 0];
    const [min, max] = minSpanDomain(values, 0.05);
    expect(min).toBe(-0.5);
    expect(max).toBe(0);
  });

  it("returns a span centered on 0 when there is no data at all", () => {
    const [min, max] = minSpanDomain([], 0.05);
    expect(min).toBeCloseTo(-0.025, 9);
    expect(max).toBeCloseTo(0.025, 9);
  });

  it("ignores null and undefined entries", () => {
    const values = [null, 0, undefined, 0.0004, null];
    const [min, max] = minSpanDomain(values, 0.05);
    expect(max - min).toBeCloseTo(0.05, 9);
  });
});

describe("MIN_POWER_SPAN_KW", () => {
  it("is 5 W expressed in kW — power values elsewhere are always in kW", () => {
    expect(MIN_POWER_SPAN_KW).toBeCloseTo(0.005, 9);
  });

  it("never lets a sub-5W power series auto-zoom narrower than the floor", () => {
    const values = [0, 0.0002, -0.0001, 0.00015];
    const [min, max] = minSpanDomain(values, MIN_POWER_SPAN_KW);
    expect(max - min).toBeGreaterThanOrEqual(MIN_POWER_SPAN_KW - 1e-12);
  });

  it("strips float noise (e.g. ~1e-16 grid-residual sums) from the returned domain", () => {
    const values = [0, 1.8e-16, -2.4e-16];
    const [min, max] = minSpanDomain(values, MIN_POWER_SPAN_KW);
    // The center these near-zero values would otherwise produce is itself
    // float noise; it must be snapped away, not surfaced as a tick value.
    expect(Number.isFinite(min)).toBe(true);
    expect(Number.isFinite(max)).toBe(true);
    expect(max - min).toBeCloseTo(MIN_POWER_SPAN_KW, 9);
    expect((min + max) / 2).toBeCloseTo(0, 9);
  });
});

describe("formatPowerTick", () => {
  it("formats sub-1kW magnitudes in whole Watts", () => {
    expect(formatPowerTick(0.045)).toBe("45 W");
    expect(formatPowerTick(-0.0034)).toBe("-3 W");
    expect(formatPowerTick(0)).toBe("0 W");
  });

  it("formats 1kW and above in kW with up to 2 decimals", () => {
    expect(formatPowerTick(2.345)).toBe("2.35 kW");
    expect(formatPowerTick(-4.4)).toBe("-4.40 kW");
    expect(formatPowerTick(1)).toBe("1.00 kW");
  });
});
