import { describe, it, expect } from "vitest";
import {
  draftFromPattern,
  eventPayloadTypes,
  fillRange,
  pasteRows,
  setValue,
  supersedePriority,
  toEventInput,
  validateDraft,
  type PatternInterval,
  type SignalDraft,
} from "../utils/signalDraft";
import { buildSignalRegistry, UnknownSignalType } from "../utils/signalRegistry";

const REGISTRY = buildSignalRegistry([
  { payloadType: "PRICE", units: "KWH", currency: "EUR" },
  { payloadType: "EXPORT_PRICE", units: "KWH", currency: "EUR" },
]);

const MIDNIGHT = Date.parse("2026-10-18T00:00:00Z");
const HOUR = 3_600_000;
const iso = (ms: number) => new Date(ms).toISOString();

/** The seeded `tou-pricing-day-ahead` as the BFF reports its unexpanded pattern. */
const SEED_PRICES = [
  0.08, 0.07, 0.06, 0.06, 0.07, 0.09, 0.12, 0.18, 0.25, 0.22, 0.15, 0.14,
  0.13, 0.14, 0.2, 0.28, 0.35, 0.4, 0.38, 0.3, 0.2, 0.14, 0.1, 0.08,
];
const SEED_PATTERN: PatternInterval[] = SEED_PRICES.map((p, h) => ({
  start: iso(MIDNIGHT + h * HOUR),
  end: iso(MIDNIGHT + (h + 1) * HOUR),
  payloads: [{ type: "PRICE", value: p }],
}));
const SEED_EVENT = {
  id: "ev-seed",
  programID: "prog-1",
  eventName: "tou-pricing-day-ahead",
  duration: "P9999Y",
  priority: null,
  targets: [],
};

function seeded(): SignalDraft {
  return draftFromPattern(SEED_EVENT, SEED_PATTERN, REGISTRY);
}

describe("draftFromPattern", () => {
  it("reads the seeded tariff as 24 rows that repeat forever, not an expansion", () => {
    const d = seeded();
    expect(d.intervals).toHaveLength(24);
    expect(d.recurrence).toEqual({ kind: "forever" });
    expect(d.types).toEqual(["PRICE"]);
    expect(d.intervals[17].values.PRICE).toBe(0.4);
  });

  it("refuses an event carrying a type the registry does not hold", () => {
    const pattern = [{ ...SEED_PATTERN[0], payloads: [{ type: "GHG", value: 100 }] }];
    expect(() => draftFromPattern(SEED_EVENT, pattern, REGISTRY)).toThrow(UnknownSignalType);
  });

  it("round-trips the seeded event with identical duration and intervals", () => {
    const out = toEventInput(seeded(), REGISTRY, []);
    expect(out.duration).toBe("P9999Y");
    expect(out.intervals).toHaveLength(24);
    expect(out.intervals?.[3]).toEqual({
      id: 3,
      intervalPeriod: { start: iso(MIDNIGHT + 3 * HOUR), duration: "PT1H" },
      payloads: [{ type: "PRICE", values: [0.06] }],
    });
  });
});

describe("editing", () => {
  it("setValue changes one cell and leaves the original draft alone", () => {
    const d = seeded();
    const e = setValue(d, 17, "PRICE", 0.5);
    expect(e.intervals[17].values.PRICE).toBe(0.5);
    expect(d.intervals[17].values.PRICE).toBe(0.4);
  });

  it("fillRange sets every row in the range, inclusive", () => {
    const e = fillRange(seeded(), "PRICE", 2, 4, 0.1);
    expect(e.intervals.slice(2, 5).map((r) => r.values.PRICE)).toEqual([0.1, 0.1, 0.1]);
    expect(e.intervals[5].values.PRICE).toBe(0.09);
  });

  it("pasteRows fills 24 consecutive rows from one number per line", () => {
    const text = Array.from({ length: 24 }, (_, i) => String(0.01 * (i + 1))).join("\n");
    const e = pasteRows(seeded(), 0, 0, text);
    expect(e.intervals).toHaveLength(24);
    expect(e.intervals[23].values.PRICE).toBeCloseTo(0.24);
  });

  it("pasteRows extends the grid when the paste is longer than the draft", () => {
    const e = pasteRows(seeded(), 22, 0, "1\n2\n3\n4");
    expect(e.intervals).toHaveLength(26);
    expect(e.intervals[25].start).toBe(iso(MIDNIGHT + 25 * HOUR));
    expect(e.intervals[25].values.PRICE).toBe(4);
  });

  it("pasteRows spreads tab- or comma-separated columns over the draft's signals", () => {
    const d = draftFromPattern(
      SEED_EVENT,
      SEED_PATTERN.map((p) => ({
        ...p,
        payloads: [
          { type: "PRICE", value: 0.1 },
          { type: "EXPORT_PRICE", value: 0.05 },
        ],
      })),
      REGISTRY,
    );
    const e = pasteRows(d, 0, 0, "0.3\t0.2\n0.31,0.21");
    expect(e.intervals[0].values).toEqual({ PRICE: 0.3, EXPORT_PRICE: 0.2 });
    expect(e.intervals[1].values).toEqual({ PRICE: 0.31, EXPORT_PRICE: 0.21 });
  });

  it("a pasted token that is not a number becomes an empty cell", () => {
    const e = pasteRows(seeded(), 0, 0, "abc");
    expect(e.intervals[0].values.PRICE).toBeNull();
  });
});

describe("validateDraft", () => {
  it("accepts the seeded tariff", () => {
    expect(validateDraft(seeded(), REGISTRY)).toEqual([]);
  });

  it("flags a cell that is empty or outside the registered range", () => {
    const d = setValue(setValue(seeded(), 0, "PRICE", null), 1, "PRICE", 99);
    const issues = validateDraft(d, REGISTRY);
    expect(issues.map((i) => [i.code, i.row])).toEqual([
      ["value", 0],
      ["range", 1],
    ]);
  });

  it("flags an interval that does not end after it starts", () => {
    const d = seeded();
    d.intervals[4] = { ...d.intervals[4], end: d.intervals[4].start };
    expect(validateDraft(d, REGISTRY).some((i) => i.code === "period" && i.row === 4)).toBe(true);
  });

  it("flags a gap in a repeating pattern and names it", () => {
    const d = seeded();
    d.intervals.splice(3, 1); // 03:00-04:00 is no longer covered
    const issues = validateDraft(d, REGISTRY);
    const gap = issues.find((i) => i.code === "gap");
    expect(gap?.message).toContain("03:00");
    expect(gap?.message).toContain("04:00");
  });

  it("allows the same gap when the series is not repeating", () => {
    const d: SignalDraft = { ...seeded(), recurrence: { kind: "once" } };
    d.intervals.splice(3, 1);
    expect(validateDraft(d, REGISTRY)).toEqual([]);
  });

  it("flags overlapping rows", () => {
    const d = seeded();
    d.intervals[5] = { ...d.intervals[5], start: iso(MIDNIGHT + 4.5 * HOUR) };
    expect(validateDraft(d, REGISTRY).some((i) => i.code === "overlap")).toBe(true);
  });

  it("flags an unreadable recurrence", () => {
    const d: SignalDraft = { ...seeded(), recurrence: { kind: "invalid", duration: "soon" } };
    expect(validateDraft(d, REGISTRY).some((i) => i.code === "recurrence")).toBe(true);
  });

  it("flags an empty draft", () => {
    const d: SignalDraft = { ...seeded(), intervals: [] };
    expect(validateDraft(d, REGISTRY).some((i) => i.code === "empty")).toBe(true);
  });
});

describe("toEventInput declares its contract", () => {
  it("emits the registry's payload descriptors", () => {
    const out = toEventInput(seeded(), REGISTRY, []);
    expect(out.payloadDescriptors).toEqual([
      { payloadType: "PRICE", units: "KWH", currency: "EUR" },
    ]);
  });

  it("declares every signal the draft carries, once", () => {
    const d = fillRange(
      { ...seeded(), types: ["PRICE", "EXPORT_PRICE"] },
      "EXPORT_PRICE",
      0,
      23,
      0.05,
    );
    const out = toEventInput(d, REGISTRY, []);
    expect(out.payloadDescriptors?.map((p) => p.payloadType)).toEqual(["PRICE", "EXPORT_PRICE"]);
  });

  it("cannot build a payload of a type the registry does not hold", () => {
    const d: SignalDraft = { ...seeded(), types: ["GHG"] };
    expect(() => toEventInput(d, REGISTRY, [])).toThrow(UnknownSignalType);
  });

  it("refuses to build an invalid draft", () => {
    const d = setValue(seeded(), 0, "PRICE", null);
    expect(() => toEventInput(d, REGISTRY, [])).toThrow(/invalid/i);
  });

  it("omits duration for a series that does not repeat", () => {
    const out = toEventInput({ ...seeded(), recurrence: { kind: "once" } }, REGISTRY, []);
    expect(out.duration).toBeNull();
  });

  it("anchors the event at the first interval", () => {
    const out = toEventInput(seeded(), REGISTRY, []);
    expect(out.intervalPeriod).toEqual({ start: iso(MIDNIGHT) });
  });
});

describe("eventPayloadTypes", () => {
  it("lists each payload type an event carries, once, in order", () => {
    const intervals = [
      { payloads: [{ type: "PRICE", values: [1] }, { type: "EXPORT_PRICE", values: [1] }] },
      { payloads: [{ type: "PRICE", values: [2] }] },
    ];
    expect(eventPayloadTypes(intervals)).toEqual(["PRICE", "EXPORT_PRICE"]);
  });

  it("is empty for an event with no readable intervals", () => {
    expect(eventPayloadTypes(undefined)).toEqual([]);
    expect(eventPayloadTypes("nope")).toEqual([]);
    expect(eventPayloadTypes([{ id: 0 }])).toEqual([]);
  });
});

describe("supersedePriority", () => {
  it("beats the best priority among the events it replaces (lower number wins)", () => {
    expect(supersedePriority([5, 3, null])).toBe(2);
  });

  it("beats an event with no priority, which is the lowest", () => {
    expect(supersedePriority([null, undefined])).toBe(0);
    expect(supersedePriority([])).toBe(0);
  });

  it("floors at 0, where the newer createdDateTime breaks the tie", () => {
    expect(supersedePriority([0])).toBe(0);
  });
});
