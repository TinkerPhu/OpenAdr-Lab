import { describe, it, expect } from "vitest";
import { minSpanDomain, MIN_POWER_SPAN_KW } from "../components/controller/charts/axisDomain";

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
  it("is 1 W expressed in kW — power values elsewhere are always in kW", () => {
    expect(MIN_POWER_SPAN_KW).toBeCloseTo(0.001, 9);
  });

  it("never lets a sub-watt power series auto-zoom narrower than 1 W", () => {
    const values = [0, 0.0002, -0.0001, 0.00015];
    const [min, max] = minSpanDomain(values, MIN_POWER_SPAN_KW);
    expect(max - min).toBeGreaterThanOrEqual(MIN_POWER_SPAN_KW - 1e-12);
  });
});
