import { describe, it, expect } from "vitest";
import { dayRangeIso, last24hRangeIso } from "../utils/dayRange";

describe("dayRangeIso", () => {
  it("returns a 24h [from, to) window for a UTC calendar day", () => {
    const { fromIso, toIso } = dayRangeIso("2026-01-01");
    expect(fromIso).toBe("2026-01-01T00:00:00.000Z");
    expect(toIso).toBe("2026-01-02T00:00:00.000Z");
  });

  it("crosses a month boundary", () => {
    const { fromIso, toIso } = dayRangeIso("2026-01-31");
    expect(fromIso).toBe("2026-01-31T00:00:00.000Z");
    expect(toIso).toBe("2026-02-01T00:00:00.000Z");
  });
});

describe("last24hRangeIso", () => {
  it("ends at the supplied instant and starts 24h before it", () => {
    const nowMs = Date.parse("2026-01-02T09:30:00.000Z");
    const { fromIso, toIso } = last24hRangeIso(nowMs);
    expect(toIso).toBe("2026-01-02T09:30:00.000Z");
    expect(fromIso).toBe("2026-01-01T09:30:00.000Z");
  });

  it("uses the caller's clock, not the browser's", () => {
    // PlanHistory used to call its own copy of this with `new Date()`, so a
    // client with a skewed OS clock queried a window that need not contain
    // any data. The window must follow the instant it is given.
    const skewed = Date.parse("2020-05-05T00:00:00.000Z");
    expect(last24hRangeIso(skewed).toIso).toBe("2020-05-05T00:00:00.000Z");
  });
});
