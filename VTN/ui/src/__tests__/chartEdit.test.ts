import { describe, it, expect } from "vitest";
import {
  moveBoundary,
  patternIndexOfBand,
  snapTimeMs,
  snapValue,
} from "../utils/chartEdit";
import { draftFromPattern, type SignalDraft } from "../utils/signalDraft";
import { buildSignalRegistry } from "../utils/signalRegistry";

const REGISTRY = buildSignalRegistry([
  { payloadType: "PRICE", units: "KWH", currency: "EUR" },
  { payloadType: "EXPORT_PRICE", units: "KWH", currency: "EUR" },
]);
const PRICE = REGISTRY.PRICE;
const MIDNIGHT = Date.parse("2026-10-18T00:00:00Z");
const HOUR = 3_600_000;
const DAY = 24 * HOUR;

function day(duration: string | null = "P9999Y"): SignalDraft {
  return draftFromPattern(
    { programID: "p", eventName: "tou", duration },
    Array.from({ length: 24 }, (_, h) => ({
      start: new Date(MIDNIGHT + h * HOUR).toISOString(),
      end: new Date(MIDNIGHT + (h + 1) * HOUR).toISOString(),
      payloads: [{ type: "PRICE", value: 0.1 + h / 100 }],
    })),
    REGISTRY,
  );
}

describe("snapValue", () => {
  it("snaps to the registry step", () => {
    expect(snapValue(0.3047, PRICE)).toBe(0.3);
    expect(snapValue(0.306, PRICE)).toBe(0.31);
  });

  it("clamps to the registered range", () => {
    expect(snapValue(99, PRICE)).toBe(PRICE.max);
    expect(snapValue(-99, PRICE)).toBe(PRICE.min);
  });
});

describe("snapTimeMs", () => {
  it("snaps to the interval grid of the signal, in UTC", () => {
    expect(snapTimeMs(MIDNIGHT + 40 * 60_000, 60)).toBe(MIDNIGHT + HOUR);
    expect(snapTimeMs(MIDNIGHT + 20 * 60_000, 60)).toBe(MIDNIGHT);
    expect(snapTimeMs(MIDNIGHT + 29 * 60_000, 60)).toBe(MIDNIGHT);
    expect(snapTimeMs(MIDNIGHT + 8 * 60_000, 15)).toBe(MIDNIGHT + 15 * 60_000);
  });
});

describe("patternIndexOfBand", () => {
  it("finds the pattern interval a first-day band is", () => {
    expect(patternIndexOfBand(day(), MIDNIGHT + 17 * HOUR)).toBe(17);
  });

  it("maps a band on a later repetition back to the same pattern interval", () => {
    expect(patternIndexOfBand(day(), MIDNIGHT + DAY + 17 * HOUR)).toBe(17);
    expect(patternIndexOfBand(day(), MIDNIGHT + 5 * DAY + 3 * HOUR)).toBe(3);
  });

  it("maps a band before the pattern's first start onto the previous repetition", () => {
    expect(patternIndexOfBand(day(), MIDNIGHT - HOUR)).toBe(23);
  });

  it("does not wrap a series that does not repeat", () => {
    expect(patternIndexOfBand(day(null), MIDNIGHT + 17 * HOUR)).toBe(17);
    expect(patternIndexOfBand(day(null), MIDNIGHT + DAY + HOUR)).toBeNull();
  });

  it("is null for an empty draft", () => {
    expect(patternIndexOfBand({ ...day(), intervals: [] }, MIDNIGHT)).toBeNull();
  });
});

describe("moveBoundary", () => {
  it("moves the edge shared by two rows, keeping the pattern contiguous", () => {
    const out = moveBoundary(day(), 4, MIDNIGHT + 5 * HOUR + 30 * 60_000, 30);
    expect(out.intervals[4].end).toBe(new Date(MIDNIGHT + 5.5 * HOUR).toISOString());
    expect(out.intervals[5].start).toBe(new Date(MIDNIGHT + 5.5 * HOUR).toISOString());
    expect(out.intervals[5].end).toBe(new Date(MIDNIGHT + 6 * HOUR).toISOString());
  });

  it("snaps to the grid", () => {
    const out = moveBoundary(day(), 4, MIDNIGHT + 5 * HOUR + 20 * 60_000, 30);
    expect(out.intervals[4].end).toBe(new Date(MIDNIGHT + 5.5 * HOUR).toISOString());
  });

  it("will not collapse a row to nothing or invert it", () => {
    const d = day();
    // Past the next boundary: row 5 would end before it starts.
    expect(moveBoundary(d, 4, MIDNIGHT + 7 * HOUR, 60)).toBe(d);
    // Onto the previous boundary: row 4 would have no length.
    expect(moveBoundary(d, 4, MIDNIGHT + 4 * HOUR, 60)).toBe(d);
  });

  it("leaves the first start and the last end alone: that would change the period", () => {
    const d = day();
    expect(moveBoundary(d, -1, MIDNIGHT + HOUR, 60)).toBe(d);
    expect(moveBoundary(d, 23, MIDNIGHT + 25 * HOUR, 60)).toBe(d);
  });

  it("does not mutate the draft it was given", () => {
    const d = day();
    moveBoundary(d, 4, MIDNIGHT + 5.5 * HOUR, 30);
    expect(d.intervals[4].end).toBe(new Date(MIDNIGHT + 5 * HOUR).toISOString());
  });
});
