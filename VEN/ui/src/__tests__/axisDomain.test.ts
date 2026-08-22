import { describe, it, expect } from "vitest";
import {
  minSpanDomain,
  tightSpanDomain,
  MIN_POWER_SPAN_KW,
  MIN_TARIFF_SPAN_EUR_KWH,
  formatPowerTick,
  roundedTimeTicks,
  niceAxis,
  tickFormatterForStep,
} from "../components/charts/axisDomain";

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

describe("roundedTimeTicks", () => {
  it("snaps ticks to the wall-clock, not to the domain's own (arbitrary) start offset", () => {
    // Domain starts 9 minutes past the hour — ticks must still land on :00/:30, not :09/:39.
    const from = Date.UTC(2026, 0, 1, 10, 9, 0);
    const to = Date.UTC(2026, 0, 1, 12, 9, 0);
    const ticks = roundedTimeTicks(from, to, 30);
    const labels = ticks.map((t) => new Date(t).toISOString().slice(11, 16));
    expect(labels).toEqual(["10:30", "11:00", "11:30", "12:00"]);
  });

  it("defaults to a 30-minute interval", () => {
    const from = Date.UTC(2026, 0, 1, 10, 0, 0);
    const to = Date.UTC(2026, 0, 1, 11, 0, 0);
    expect(roundedTimeTicks(from, to)).toEqual([
      Date.UTC(2026, 0, 1, 10, 0, 0),
      Date.UTC(2026, 0, 1, 10, 30, 0),
      Date.UTC(2026, 0, 1, 11, 0, 0),
    ]);
  });

  it("honors a custom interval", () => {
    const from = Date.UTC(2026, 0, 1, 10, 0, 0);
    const to = Date.UTC(2026, 0, 1, 11, 0, 0);
    const ticks = roundedTimeTicks(from, to, 15);
    expect(ticks).toHaveLength(5);
  });

  it("returns an empty array when the window is narrower than the interval", () => {
    const from = Date.UTC(2026, 0, 1, 10, 5, 0);
    const to = Date.UTC(2026, 0, 1, 10, 10, 0);
    expect(roundedTimeTicks(from, to, 30)).toEqual([]);
  });

  it("falls back to hourly, full-hour-anchored ticks when a 30-min window is too dense (e.g. 24h)", () => {
    // A History-page-sized window starting at an arbitrary offset (10:07, not on any
    // 30-min mark) — 30-min spacing here would produce 48 ticks, which is exactly the
    // case that used to leave only :30 marks after the chart thinned the array itself.
    const from = Date.UTC(2026, 0, 1, 10, 7, 0);
    const to = Date.UTC(2026, 0, 2, 10, 7, 0);
    const ticks = roundedTimeTicks(from, to, 30);
    expect(ticks.length).toBeLessThanOrEqual(25);
    for (const t of ticks) {
      expect(new Date(t).getUTCMinutes()).toBe(0);
    }
  });

  it("keeps 30-min spacing (with half-hour marks) when the window is short enough to fit", () => {
    const from = Date.UTC(2026, 0, 1, 10, 9, 0);
    const to = Date.UTC(2026, 0, 1, 12, 9, 0);
    const ticks = roundedTimeTicks(from, to, 30);
    expect(ticks.some((t) => new Date(t).getUTCMinutes() === 30)).toBe(true);
  });
});

describe("MIN_TARIFF_SPAN_EUR_KWH", () => {
  it("never lets a near-flat tariff series auto-zoom narrower than the floor", () => {
    const values = [0.21, 0.2101, 0.2099];
    const [min, max] = tightSpanDomain(values, MIN_TARIFF_SPAN_EUR_KWH);
    expect(max - min).toBeGreaterThanOrEqual(MIN_TARIFF_SPAN_EUR_KWH - 1e-12);
  });
});

describe("tightSpanDomain", () => {
  it("does NOT anchor at 0 for an always-positive series — the bug minSpanDomain would have", () => {
    // A realistic tariff band that never approaches 0. minSpanDomain would return
    // [0, 0.32], compressing this real ~0.04 range into the top ~12% of the axis —
    // exactly the "squeezed" defect this function exists to avoid reintroducing.
    const values = [0.28, 0.30, 0.32, 0.29];
    const [min, max] = tightSpanDomain(values, 0.02);
    expect(min).toBeCloseTo(0.28, 9);
    expect(max).toBeCloseTo(0.32, 9);
  });

  it("still expands a near-flat series to the minimum span, centered on the real data", () => {
    const values = [0.30, 0.3001, 0.2999];
    const [min, max] = tightSpanDomain(values, 0.02);
    expect(max - min).toBeCloseTo(0.02, 9);
    expect((min + max) / 2).toBeCloseTo(0.3, 9);
  });

  it("handles a negative-only series without forcing 0 into the domain", () => {
    const values = [-0.5, -0.3, -0.4];
    const [min, max] = tightSpanDomain(values, 0.02);
    expect(min).toBeCloseTo(-0.5, 9);
    expect(max).toBeCloseTo(-0.3, 9);
  });

  it("falls back to a span centered on 0 when there is no data at all", () => {
    const [min, max] = tightSpanDomain([], 0.02);
    expect(min).toBeCloseTo(-0.01, 9);
    expect(max).toBeCloseTo(0.01, 9);
  });

  it("ignores null and undefined entries", () => {
    const values = [null, 0.28, undefined, 0.32, null];
    const [min, max] = tightSpanDomain(values, 0.02);
    expect(min).toBeCloseTo(0.28, 9);
    expect(max).toBeCloseTo(0.32, 9);
  });
});

/** Mantissa of `v` normalized to [1,10) — 0.05 → 5, 200 → 2 — with float noise scrubbed. */
function mantissa(v: number): number {
  const exponent = Math.floor(Math.log10(Math.abs(v)));
  return Number((Math.abs(v) / Math.pow(10, exponent)).toFixed(6));
}

describe("niceAxis", () => {
  it("returns round ticks for a negative-only domain (the PV export-revenue axis)", () => {
    // The real defect: minSpanDomain([-0.4172 ... 0]) used to reach recharts un-nice-ed,
    // which split the raw span into equal parts (-0.4172, -0.3129, -0.2086, ...).
    const { ticks, domain } = niceAxis([-0.4172, 0]);
    expect(ticks).toContain(0);
    expect(domain[0]).toBeLessThanOrEqual(-0.4172);
    for (const t of ticks) expect(Number(t.toFixed(6))).toBe(Number(t.toFixed(1)));
  });

  it("returns round ticks for a positive-only domain (the tariff axis)", () => {
    const { ticks, step } = niceAxis([0.2899, 0.3101]);
    expect(ticks.length).toBeGreaterThanOrEqual(3);
    for (const t of ticks) expect(Math.abs(t / step - Math.round(t / step))).toBeLessThan(1e-6);
  });

  it("keeps 0 as a tick whenever the domain contains it", () => {
    for (const d of [[-3, 5], [-0.05, 0.05], [0, 940], [-120, 0]] as [number, number][]) {
      expect(niceAxis(d).ticks).toContain(0);
    }
  });

  it("uses a finer step when the range is narrow and offset from zero (1.3 / 1.4 / 1.5)", () => {
    // Finer steps are deliberately NOT ruled out: a narrow band far from zero must still get
    // readable ticks rather than being coarsened into a single label.
    const { ticks } = niceAxis([1.32, 1.48]);
    expect(ticks).toEqual([1.3, 1.4, 1.5]);
  });

  it("prefers the coarsest step that still yields enough ticks (roundest labels win)", () => {
    const { ticks, step } = niceAxis([-0.4172, 0]);
    expect(step).toBe(0.2);
    expect(ticks).toEqual([-0.6, -0.4, -0.2, 0]);
  });

  it("expands the domain outward so the extreme ticks are round too", () => {
    const { domain, ticks } = niceAxis([-0.4172, 0]);
    expect(domain).toEqual([ticks[0], ticks[ticks.length - 1]]);
  });

  it("never emits float-noise ticks (e.g. 0.30000000000000004)", () => {
    for (const d of [[0, 0.5], [-0.3, 0.3], [0.28, 0.32], [-1e-4, 1e-4]] as [number, number][]) {
      for (const t of niceAxis(d).ticks) {
        expect(String(t)).not.toMatch(/\d{8,}/);
      }
    }
  });

  it("PROPERTY: every tick is an exact multiple of a 1/2/5×10^n step, for any domain", () => {
    // The systematic guarantee: it holds by construction for positive-only, negative-only,
    // straddling, tiny and huge domains alike — no per-chart check anywhere.
    const bounds = [-9400, -137.4, -0.4172, -0.0031, 0, 0.0007, 0.29, 3.7, 941, 128000];
    for (const lo of bounds) {
      for (const hi of bounds) {
        if (hi <= lo) continue;
        const { ticks, step } = niceAxis([lo, hi]);
        expect([1, 2, 5]).toContain(mantissa(step));
        expect(ticks.length).toBeGreaterThanOrEqual(3);
        expect(ticks.length).toBeLessThanOrEqual(7);
        for (const t of ticks) {
          expect(Math.abs(t / step - Math.round(t / step))).toBeLessThan(1e-6);
        }
      }
    }
  });

  it("survives a degenerate (zero-span) domain", () => {
    const { ticks, step } = niceAxis([2, 2]);
    expect(step).toBeGreaterThan(0);
    expect(ticks.length).toBeGreaterThanOrEqual(3);
    expect(ticks).toContain(2);
  });

  it("widens a degenerate [0, 0] domain upward only, never into negative labels", () => {
    // Regression: CurveChart's price axis (`niceAxis([0, Math.max(...bids, 0)])`) and
    // PlanHistory's solve-time axis hit exactly this domain whenever every value is 0 — both
    // are non-negative-only quantities (price, milliseconds), so a symmetric widen around 0
    // used to render an impossible negative tick (e.g. "-0.5 ms").
    const { ticks, domain } = niceAxis([0, 0]);
    expect(domain[0]).toBeGreaterThanOrEqual(0);
    for (const t of ticks) expect(t).toBeGreaterThanOrEqual(0);
    expect(ticks).toContain(0);
  });
});

describe("tickFormatterForStep", () => {
  it("prints exactly the decimals the step implies", () => {
    expect(tickFormatterForStep(0.05)(-0.35)).toBe("-0.35");
    expect(tickFormatterForStep(0.2)(-0.4)).toBe("-0.4");
    expect(tickFormatterForStep(200)(600)).toBe("600");
    expect(tickFormatterForStep(1)(3)).toBe("3");
  });

  it("strips float noise instead of stringifying it verbatim", () => {
    expect(tickFormatterForStep(0.1)(0.30000000000000004)).toBe("0.3");
  });
});
