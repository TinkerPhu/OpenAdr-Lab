import { describe, it, expect } from "vitest";
import { SIGNAL_OPERATORS, applyOperator } from "../utils/signalOperators";
import { draftFromPattern, type SignalDraft } from "../utils/signalDraft";
import { buildSignalRegistry } from "../utils/signalRegistry";

const REGISTRY = buildSignalRegistry([
  { payloadType: "PRICE", units: "KWH", currency: "EUR" },
  { payloadType: "EXPORT_PRICE", units: "KWH", currency: "EUR" },
]);
const MIDNIGHT = Date.parse("2026-10-18T00:00:00Z");
const HOUR = 3_600_000;

/** 24 hourly rows from UTC midnight; PRICE = hour/100 + 0.1, so every row is distinguishable. */
function day(duration: string | null = "P9999Y"): SignalDraft {
  return draftFromPattern(
    { programID: "p", eventName: "tou", duration },
    Array.from({ length: 24 }, (_, h) => ({
      start: new Date(MIDNIGHT + h * HOUR).toISOString(),
      end: new Date(MIDNIGHT + (h + 1) * HOUR).toISOString(),
      payloads: [{ type: "PRICE", value: Number((0.1 + h / 100).toFixed(2)) }],
    })),
    REGISTRY,
  );
}
const prices = (d: SignalDraft) => d.intervals.map((r) => r.values.PRICE);

describe("scale", () => {
  it("multiplies every value and rounds to the registry step", () => {
    const out = applyOperator("scale", day(), REGISTRY, "PRICE", { factor: 1.2 });
    expect(out.intervals[0].values.PRICE).toBe(0.12); // 0.1 * 1.2
    expect(out.intervals[10].values.PRICE).toBe(0.24); // 0.2 * 1.2
  });

  it("leaves the recurrence and the other columns alone", () => {
    const d = day();
    const out = applyOperator("scale", d, REGISTRY, "PRICE", { factor: 2 });
    expect(out.recurrence).toEqual(d.recurrence);
    expect(out.intervals.map((r) => [r.start, r.end])).toEqual(d.intervals.map((r) => [r.start, r.end]));
  });

  it("does not mutate the draft it was given", () => {
    const d = day();
    applyOperator("scale", d, REGISTRY, "PRICE", { factor: 3 });
    expect(d.intervals[0].values.PRICE).toBe(0.1);
  });
});

describe("shift", () => {
  it("moves the values later by whole rows, wrapping inside a repeating pattern", () => {
    const out = applyOperator("shift", day(), REGISTRY, "PRICE", { hours: 2 });
    expect(out.intervals[2].values.PRICE).toBe(0.1); // what was at 00:00 is now at 02:00
    expect(out.intervals[0].values.PRICE).toBe(0.32); // 22:00's value wrapped to 00:00
    expect(out.intervals).toHaveLength(24);
  });

  it("shifts earlier with a negative amount", () => {
    const out = applyOperator("shift", day(), REGISTRY, "PRICE", { hours: -1 });
    expect(out.intervals[0].values.PRICE).toBe(0.11);
    expect(out.intervals[23].values.PRICE).toBe(0.1); // 00:00 wrapped to 23:00
  });

  it("does not wrap a series that does not repeat: the edge row is held", () => {
    const out = applyOperator("shift", day(null), REGISTRY, "PRICE", { hours: 2 });
    expect(out.intervals[0].values.PRICE).toBe(0.1);
    expect(out.intervals[1].values.PRICE).toBe(0.1);
    expect(out.intervals[2].values.PRICE).toBe(0.1);
    expect(out.intervals[3].values.PRICE).toBe(0.11);
  });

  it("is a no-op for a shift that is a whole number of periods", () => {
    expect(prices(applyOperator("shift", day(), REGISTRY, "PRICE", { hours: 24 }))).toEqual(
      prices(day()),
    );
  });
});

describe("cap", () => {
  it("limits values to a maximum", () => {
    const out = applyOperator("cap", day(), REGISTRY, "PRICE", { max: 0.2 });
    expect(Math.max(...(prices(out) as number[]))).toBe(0.2);
    expect(out.intervals[5].values.PRICE).toBe(0.15);
  });
});

describe("addRange", () => {
  it("adds an amount to the rows starting inside [from, to) UTC hours", () => {
    const out = applyOperator("addRange", day(), REGISTRY, "PRICE", { from: 17, to: 20, delta: 0.2 });
    expect(out.intervals[16].values.PRICE).toBe(0.26);
    expect(out.intervals[17].values.PRICE).toBe(0.47);
    expect(out.intervals[19].values.PRICE).toBe(0.49);
    expect(out.intervals[20].values.PRICE).toBe(0.3);
  });
});

describe("templates", () => {
  it("flat sets every row to one value", () => {
    const out = applyOperator("flat", day(), REGISTRY, "PRICE", { value: 0.25 });
    expect(new Set(prices(out))).toEqual(new Set([0.25]));
  });

  it("two-tier sets peak hours and off-peak hours", () => {
    const out = applyOperator("twoTier", day(), REGISTRY, "PRICE", {
      offPeak: 0.1,
      peak: 0.4,
      peakFrom: 17,
      peakTo: 20,
    });
    expect(prices(out).filter((p) => p === 0.4)).toHaveLength(3);
    expect(out.intervals[18].values.PRICE).toBe(0.4);
    expect(out.intervals[3].values.PRICE).toBe(0.1);
  });

  it("three-tier adds a shoulder around the peak", () => {
    const out = applyOperator("threeTier", day(), REGISTRY, "PRICE", {
      offPeak: 0.1,
      shoulder: 0.2,
      peak: 0.4,
      peakFrom: 17,
      peakTo: 20,
      shoulderFrom: 7,
      shoulderTo: 22,
    });
    expect(out.intervals[3].values.PRICE).toBe(0.1);
    expect(out.intervals[10].values.PRICE).toBe(0.2);
    expect(out.intervals[18].values.PRICE).toBe(0.4);
    expect(out.intervals[23].values.PRICE).toBe(0.1);
  });

  it("a template does not change the recurrence or the times", () => {
    const d = day();
    const out = applyOperator("flat", d, REGISTRY, "PRICE", { value: 0.3 });
    expect(out.recurrence).toEqual(d.recurrence);
    expect(out.intervals.map((r) => r.start)).toEqual(d.intervals.map((r) => r.start));
  });
});

describe("the operator table", () => {
  it("declares every operator with a label and typed parameters", () => {
    for (const [id, op] of Object.entries(SIGNAL_OPERATORS)) {
      expect(op.label, id).toBeTruthy();
      for (const p of op.params) {
        expect(p.key, id).toBeTruthy();
        expect(Number.isFinite(p.default), `${id}.${p.key}`).toBe(true);
      }
    }
    expect(Object.keys(SIGNAL_OPERATORS).sort()).toEqual(
      ["addRange", "cap", "flat", "scale", "shift", "threeTier", "twoTier"].sort(),
    );
  });

  it("refuses an operator it does not know", () => {
    expect(() => applyOperator("nope", day(), REGISTRY, "PRICE", {})).toThrow(/nope/);
  });
});
