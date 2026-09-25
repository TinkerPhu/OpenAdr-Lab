/**
 * The wash is checked by reading values, not by rendering a chart — the same
 * approach NowLine.test.tsx takes for the other render-function primitives.
 */
import { describe, it, expect } from "vitest";
import { dayNightBands, nightAlphaAt, renderDayNightShading } from "@lab/charts/DayNightShading";

const LAT = 47.4491;
const LON = 7.8081;
const spec = (tMin: number, tMax: number, steps?: number) => ({
  tMin,
  tMax,
  latitudeDeg: LAT,
  longitudeDeg: LON,
  steps,
});

const alphaAt = (iso: string) => nightAlphaAt(Date.parse(iso), LAT, LON);

describe("nightAlphaAt", () => {
  it("leaves midday clear and midnight dark", () => {
    expect(alphaAt("2026-06-21T11:41:00Z")).toBe(0);
    expect(alphaAt("2026-06-21T23:30:00Z")).toBeGreaterThan(0);
  });

  it("is seasonal — the whole point of using the real sun", () => {
    // 17:00 UTC (19:00 local) is daylight in June and night in December. A
    // clock-based gradient would give these two the same value, so this is the
    // assertion that fails if anyone swaps the solar maths for a cosine.
    expect(alphaAt("2026-06-21T17:00:00Z")).toBe(0);
    expect(alphaAt("2026-12-21T17:00:00Z")).toBeGreaterThan(0);
  });

  it("ramps through twilight instead of switching", () => {
    // Late September, around local dusk: some band between full day and full
    // night must be partially shaded, or dawn and dusk are a hard edge.
    const samples = Array.from({ length: 24 }, (_, i) =>
      alphaAt(new Date(Date.parse("2026-09-24T16:00:00Z") + i * 600_000).toISOString()),
    );
    const partial = samples.filter((a) => a > 0 && a < Math.max(...samples));
    expect(partial.length).toBeGreaterThan(0);
  });

  it("never gets dark enough to hide a curve", () => {
    expect(alphaAt("2026-12-21T23:00:00Z")).toBeLessThanOrEqual(0.2);
  });
});

describe("dayNightBands", () => {
  const day = Date.parse("2026-09-24T00:00:00Z");

  it("costs the same whether the window is 15 minutes or 24 hours", () => {
    const short = dayNightBands(spec(day, day + 15 * 60_000));
    const long = dayNightBands(spec(day, day + 24 * 3_600_000));
    expect(short).toHaveLength(long.length);
  });

  it("tiles the window edge to edge with no gaps", () => {
    const bands = dayNightBands(spec(day, day + 3_600_000, 4));
    expect(bands[0].x1).toBe(day);
    expect(bands[bands.length - 1].x2).toBe(day + 3_600_000);
    for (let i = 1; i < bands.length; i++) {
      expect(bands[i].x1).toBe(bands[i - 1].x2);
    }
  });

  it("is darker at night than at noon across a whole day", () => {
    const bands = dayNightBands(spec(day, day + 24 * 3_600_000));
    const noon = bands.find((b) => new Date(b.x1).getUTCHours() === 11)!;
    const midnight = bands.find((b) => new Date(b.x1).getUTCHours() === 0)!;
    expect(noon.alpha).toBeLessThan(midnight.alpha);
  });

  it("has nothing to draw for an empty or inverted window", () => {
    expect(dayNightBands(spec(day, day))).toEqual([]);
    expect(dayNightBands(spec(day, day - 1000))).toEqual([]);
  });
});

describe("renderDayNightShading", () => {
  const day = Date.parse("2026-09-24T00:00:00Z");
  type AreaProps = { yAxisId: string; ifOverflow: string; fillOpacity: number; fill: string };
  const props = (i: number) =>
    renderDayNightShading("kw", spec(day, day + 24 * 3_600_000, 8))[i].props as AreaProps;

  it("puts every band on the caller's axis and clips it to the plot", () => {
    expect(props(0).yAxisId).toBe("kw");
    expect(props(0).ifOverflow).toBe("hidden");
  });

  it("sets fillOpacity explicitly, since recharts would otherwise halve it", () => {
    // ReferenceArea defaults fillOpacity to 0.5; the alpha lives in the rgba().
    expect(props(0).fillOpacity).toBe(1);
    expect(props(0).fill).toMatch(/^rgba\(/);
  });
});
