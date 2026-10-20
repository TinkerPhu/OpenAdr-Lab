import { describe, it, expect } from "vitest";
import {
  FOREVER_DURATION,
  addIsoDuration,
  crossesUtcOffsetChange,
  describePeriod,
  formatIsoDuration,
  durationFromRecurrence,
  recurrenceFromDuration,
} from "../utils/recurrence";

const START = new Date("2026-10-18T00:00:00Z");
const DAY_MS = 24 * 3_600_000;
const PATTERN_END = new Date(START.getTime() + DAY_MS);

describe("addIsoDuration", () => {
  it("adds hours and days", () => {
    expect(addIsoDuration(START, "PT4H")?.toISOString()).toBe("2026-10-18T04:00:00.000Z");
    expect(addIsoDuration(START, "P7D")?.toISOString()).toBe("2026-10-25T00:00:00.000Z");
  });

  it("resolves years and months against the anchor, as the receiver does", () => {
    expect(addIsoDuration(new Date("2024-01-31T00:00:00Z"), "P1M")).not.toBeNull();
    expect(addIsoDuration(START, "P1Y")?.toISOString()).toBe("2027-10-18T00:00:00.000Z");
    expect(addIsoDuration(START, FOREVER_DURATION)?.getUTCFullYear()).toBe(2026 + 9999);
  });

  it("rejects what is not an ISO 8601 duration", () => {
    expect(addIsoDuration(START, "")).toBeNull();
    expect(addIsoDuration(START, "4 hours")).toBeNull();
    expect(addIsoDuration(START, "P")).toBeNull();
    expect(addIsoDuration(START, "PT")).toBeNull();
  });
});

describe("recurrenceFromDuration", () => {
  it("no duration is once", () => {
    expect(recurrenceFromDuration(null, START, PATTERN_END)).toEqual({ kind: "once" });
    expect(recurrenceFromDuration(undefined, START, PATTERN_END)).toEqual({ kind: "once" });
    expect(recurrenceFromDuration("", START, PATTERN_END)).toEqual({ kind: "once" });
  });

  it("P9999Y is forever", () => {
    expect(recurrenceFromDuration("P9999Y", START, PATTERN_END)).toEqual({ kind: "forever" });
  });

  it("the VTN's long form of P9999Y is forever too", () => {
    // The VTN stores and returns the duration it was given as P9999Y0M0DT0H0M0S.
    expect(recurrenceFromDuration("P9999Y0M0DT0H0M0S", START, PATTERN_END)).toEqual({
      kind: "forever",
    });
  });

  it("a duration reaching the end of the pattern is a span", () => {
    expect(recurrenceFromDuration("P7D", START, PATTERN_END)).toEqual({
      kind: "span",
      duration: "P7D",
    });
  });

  it("a duration shorter than the pattern is truncated", () => {
    expect(recurrenceFromDuration("PT12H", START, PATTERN_END)).toEqual({
      kind: "truncated",
      duration: "PT12H",
    });
  });

  it("an unreadable duration is invalid, and keeps what was written", () => {
    expect(recurrenceFromDuration("soon", START, PATTERN_END)).toEqual({
      kind: "invalid",
      duration: "soon",
    });
  });
});

describe("durationFromRecurrence", () => {
  it("is the inverse of recurrenceFromDuration", () => {
    for (const d of [null, "P9999Y", "P7D", "PT12H"]) {
      const r = recurrenceFromDuration(d, START, PATTERN_END);
      expect(durationFromRecurrence(r)).toBe(d);
    }
  });

  it("writes forever as P9999Y and once as no duration", () => {
    expect(durationFromRecurrence({ kind: "forever" })).toBe("P9999Y");
    expect(durationFromRecurrence({ kind: "once" })).toBeNull();
  });
});

describe("formatIsoDuration", () => {
  it("writes whole hours, minutes and seconds", () => {
    expect(formatIsoDuration(3_600_000)).toBe("PT1H");
    expect(formatIsoDuration(1_800_000)).toBe("PT30M");
    expect(formatIsoDuration(5_400_000)).toBe("PT1H30M");
    expect(formatIsoDuration(90_000)).toBe("PT1M30S");
    expect(formatIsoDuration(0)).toBe("PT0S");
  });

  it("is read back by addIsoDuration", () => {
    for (const span of [3_600_000, 900_000, 5_400_000, 90_000]) {
      expect(addIsoDuration(START, formatIsoDuration(span))?.getTime()).toBe(START.getTime() + span);
    }
  });
});

describe("describePeriod", () => {
  it("names the period a pattern repeats by", () => {
    expect(describePeriod(24 * 3_600_000)).toBe("24 h");
    expect(describePeriod(7 * 24 * 3_600_000)).toBe("7 d");
    expect(describePeriod(90 * 60_000)).toBe("1 h 30 min");
    expect(describePeriod(15 * 60_000)).toBe("15 min");
  });
});

describe("crossesUtcOffsetChange", () => {
  it("is true across the autumn clock change in Zurich", () => {
    const from = Date.parse("2026-10-24T12:00:00Z");
    const to = Date.parse("2026-10-26T12:00:00Z");
    expect(crossesUtcOffsetChange(from, to, "Europe/Zurich")).toBe(true);
  });

  it("is false in an ordinary week, and always false in UTC", () => {
    const from = Date.parse("2026-10-18T00:00:00Z");
    const to = Date.parse("2026-10-20T00:00:00Z");
    expect(crossesUtcOffsetChange(from, to, "Europe/Zurich")).toBe(false);
    expect(crossesUtcOffsetChange(from, Date.parse("2027-01-01T00:00:00Z"), "UTC")).toBe(false);
  });
});
