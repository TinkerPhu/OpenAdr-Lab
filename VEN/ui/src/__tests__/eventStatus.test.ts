import { describe, it, expect } from "vitest";
import { getEventStatus, statusColor } from "../utils/eventStatus";
import type { VtnEvent } from "../api/types";

function makeEvent(overrides: Partial<VtnEvent> = {}): VtnEvent {
  return { id: "e1", ...overrides };
}

describe("getEventStatus", () => {
  it("returns 'no timing' when no intervalPeriod", () => {
    expect(getEventStatus(makeEvent())).toBe("no timing");
  });

  it("returns 'no timing' when intervalPeriod has no start", () => {
    expect(getEventStatus(makeEvent({ intervalPeriod: { start: "" } }))).toBe("no timing");
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
  it("maps no timing to warning", () => expect(statusColor("no timing")).toBe("warning"));
});
