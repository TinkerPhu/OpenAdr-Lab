import { describe, it, expect } from "vitest";
import { getEventStatus, statusColor } from "../utils/eventStatus";
import type { VtnEvent } from "../api/types";

function makeEvent(overrides: Partial<VtnEvent> = {}): VtnEvent {
  return { id: "e1", ...overrides };
}

describe("getEventStatus", () => {
  it("returns 'immediate' when no intervalPeriod", () => {
    expect(getEventStatus(makeEvent())).toBe("immediate");
  });

  it("returns 'immediate' when intervalPeriod has no start", () => {
    expect(getEventStatus(makeEvent({ intervalPeriod: { start: "" } }))).toBe("immediate");
  });

  it("returns 'scheduled' when start is in the future", () => {
    const future = new Date(Date.now() + 3600000).toISOString();
    expect(getEventStatus(makeEvent({ intervalPeriod: { start: future, duration: "PT1H" } }))).toBe("scheduled");
  });

  it("returns 'active' when now is between start and end", () => {
    const now = new Date("2026-02-08T12:00:00Z");
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T11:00:00Z", duration: "PT2H" },
    });
    expect(getEventStatus(event, now)).toBe("active");
  });

  it("returns 'completed' when now is past end", () => {
    const now = new Date("2026-02-08T14:00:00Z");
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T10:00:00Z", duration: "PT2H" },
    });
    expect(getEventStatus(event, now)).toBe("completed");
  });

  it("returns 'active' when duration is missing (open-ended)", () => {
    const now = new Date("2026-02-08T12:00:00Z");
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T11:00:00Z" },
    });
    expect(getEventStatus(event, now)).toBe("active");
  });

  it("parses PT30M duration correctly", () => {
    const now = new Date("2026-02-08T11:35:00Z");
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T11:00:00Z", duration: "PT30M" },
    });
    expect(getEventStatus(event, now)).toBe("completed");
  });

  // Since the VEN adopted the OpenADR wire types it re-serialises durations in
  // the fully expanded form. A parser that only matched the compact spelling
  // returned 0, which reads as "no duration" — so every event rendered as if
  // it never ended.
  it("parses the expanded P0Y0M0DT1H0M0S spelling the wire types emit", () => {
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T11:00:00Z", duration: "P0Y0M0DT1H0M0S" },
    });
    expect(getEventStatus(event, new Date("2026-02-08T11:30:00Z"))).toBe("active");
    expect(getEventStatus(event, new Date("2026-02-08T12:01:00Z"))).toBe("completed");
  });

  it("reads M as months before T and as minutes after it", () => {
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T11:00:00Z", duration: "P0Y0M0DT0H30M0S" },
    });
    // 30 *minutes*: still running at 11:20, over by 11:40. Read as 30 months
    // this would be active for years.
    expect(getEventStatus(event, new Date("2026-02-08T11:20:00Z"))).toBe("active");
    expect(getEventStatus(event, new Date("2026-02-08T11:40:00Z"))).toBe("completed");
  });

  it("treats the spec's P9999Y infinity as still running", () => {
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T11:00:00Z", duration: "P9999Y" },
    });
    expect(getEventStatus(event, new Date("2030-01-01T00:00:00Z"))).toBe("active");
  });

  it("parses P1DT2H30M duration", () => {
    const now = new Date("2026-02-09T13:29:00Z");
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T11:00:00Z", duration: "P1DT2H30M" },
    });
    expect(getEventStatus(event, now)).toBe("active");
  });

  it("handles exactly-at-start as active", () => {
    const now = new Date("2026-02-08T11:00:00Z");
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T11:00:00Z", duration: "PT1H" },
    });
    expect(getEventStatus(event, now)).toBe("active");
  });

  it("handles exactly-at-end as completed", () => {
    const now = new Date("2026-02-08T12:00:00Z");
    const event = makeEvent({
      intervalPeriod: { start: "2026-02-08T11:00:00Z", duration: "PT1H" },
    });
    expect(getEventStatus(event, now)).toBe("completed");
  });
});

describe("statusColor", () => {
  it("maps active to success", () => expect(statusColor("active")).toBe("success"));
  it("maps scheduled to info", () => expect(statusColor("scheduled")).toBe("info"));
  it("maps completed to default", () => expect(statusColor("completed")).toBe("default"));
  it("maps immediate to warning", () => expect(statusColor("immediate")).toBe("warning"));
});
