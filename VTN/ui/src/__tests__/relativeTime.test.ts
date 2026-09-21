import { describe, it, expect } from "vitest";
import { formatAge } from "../utils/relativeTime";

const now = new Date("2026-01-01T12:00:00Z");

describe("formatAge", () => {
  it("says never when nothing has arrived", () => {
    expect(formatAge(null, now)).toBe("never");
  });

  it("reports seconds, minutes and hours at the coarsest useful unit", () => {
    expect(formatAge("2026-01-01T11:59:57Z", now)).toBe("3s ago");
    expect(formatAge("2026-01-01T11:58:00Z", now)).toBe("2m ago");
    expect(formatAge("2026-01-01T09:00:00Z", now)).toBe("3h ago");
    expect(formatAge("2025-12-30T12:00:00Z", now)).toBe("2d ago");
  });

  /** A clock that disagrees is a real condition between two hosts, and "-4s
   * ago" reads as a bug in the page rather than in the clocks. */
  it("clamps a timestamp from the future to just now", () => {
    expect(formatAge("2026-01-01T12:00:04Z", now)).toBe("just now");
  });

  it("says so when the timestamp cannot be read, rather than showing NaN", () => {
    expect(formatAge("not a date", now)).toBe("unknown");
  });
});
