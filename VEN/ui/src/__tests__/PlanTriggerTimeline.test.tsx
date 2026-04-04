import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";
import { PlanTriggerTimeline } from "../components/planner/PlanTriggerTimeline";
import type { TraceEntry } from "../api/types";

// ─── Helpers ──────────────────────────────────────────────────────────────────

function makePlanCycle(overrides: Partial<Extract<TraceEntry, { type: "PlanCycle" }>> = {}): TraceEntry {
  return {
    type: "PlanCycle",
    ts: "2026-04-04T10:00:00Z",
    trigger_reason: "Periodic",
    firm_slots: 12,
    flexible_slots: 36,
    ...overrides,
  };
}

function makeRateChange(overrides: Partial<Extract<TraceEntry, { type: "RateChange" }>> = {}): TraceEntry {
  return {
    type: "RateChange",
    ts: "2026-04-04T09:55:00Z",
    interval_start: "2026-04-04T10:00:00Z",
    import_eur_kwh: 0.123,
    export_eur_kwh: 0.05,
    ...overrides,
  };
}

function makeCapacityChange(): TraceEntry {
  return {
    type: "CapacityChange",
    ts: "2026-04-04T09:50:00Z",
    import_limit_kw: 5.0,
    export_limit_kw: null,
  };
}

function makeOpenAdrArrived(): TraceEntry {
  return {
    type: "OpenAdrArrived",
    ts: "2026-04-04T09:45:00Z",
    event_name: "SummerPeakDR",
    signal_type: "PRICE",
    value: 2.0,
    interval: 3600,
  };
}

function makeOpenAdrExpired(): TraceEntry {
  return {
    type: "OpenAdrExpired",
    ts: "2026-04-04T09:40:00Z",
    event_name: "SummerPeakDR",
  };
}

function makePacketTransition(): TraceEntry {
  return {
    type: "PacketTransition",
    ts: "2026-04-04T09:35:00Z",
    packet_id: "pkt-abc123",
    asset_id: "ev",
    from_status: "PENDING",
    to_status: "ACTIVE",
  };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("PlanTriggerTimeline", () => {
  it("renders empty state when events array is empty", () => {
    render(<PlanTriggerTimeline events={[]} />);
    const timeline = screen.getByTestId("trigger-timeline");
    expect(timeline).toBeInTheDocument();
    expect(timeline.textContent).toMatch(/no.*event/i);
  });

  it("renders one chip per event", () => {
    const events: TraceEntry[] = [makePlanCycle(), makeRateChange()];
    render(<PlanTriggerTimeline events={events} />);
    // Two chips expected
    const chips = document.querySelectorAll('[data-testid^="trigger-chip-"]');
    expect(chips.length).toBe(2);
  });

  it("renders chips with data-testid trigger-chip-{i}", () => {
    render(<PlanTriggerTimeline events={[makePlanCycle()]} />);
    expect(screen.getByTestId("trigger-chip-0")).toBeInTheDocument();
  });

  it("shows trigger_reason in PlanCycle chip label", () => {
    render(<PlanTriggerTimeline events={[makePlanCycle({ trigger_reason: "RateChange" })]} />);
    const chip = screen.getByTestId("trigger-chip-0");
    expect(chip.textContent).toMatch(/RateChange|Plan/);
  });

  it("shows import tariff value in RateChange chip label", () => {
    render(<PlanTriggerTimeline events={[makeRateChange({ import_eur_kwh: 0.123 })]} />);
    const chip = screen.getByTestId("trigger-chip-0");
    expect(chip.textContent).toMatch(/0\.12|0\.123/);
  });

  it("shows import_limit_kw in CapacityChange chip label", () => {
    render(<PlanTriggerTimeline events={[makeCapacityChange()]} />);
    const chip = screen.getByTestId("trigger-chip-0");
    expect(chip.textContent).toMatch(/5|Cap/);
  });

  it("shows event_name in OpenAdrArrived chip", () => {
    render(<PlanTriggerTimeline events={[makeOpenAdrArrived()]} />);
    const chip = screen.getByTestId("trigger-chip-0");
    expect(chip.textContent).toMatch(/SummerPeak|PRICE/i);
  });

  it("shows expired event in OpenAdrExpired chip", () => {
    render(<PlanTriggerTimeline events={[makeOpenAdrExpired()]} />);
    const chip = screen.getByTestId("trigger-chip-0");
    expect(chip.textContent).toMatch(/SummerPeak|✗/);
  });

  it("shows asset transition in PacketTransition chip", () => {
    render(<PlanTriggerTimeline events={[makePacketTransition()]} />);
    const chip = screen.getByTestId("trigger-chip-0");
    expect(chip.textContent).toMatch(/ev|PENDING|ACTIVE/i);
  });

  it("opens popover on chip click", async () => {
    const user = userEvent.setup();
    render(<PlanTriggerTimeline events={[makePlanCycle()]} />);
    expect(screen.queryByTestId("trigger-popover")).toBeNull();
    await user.click(screen.getByTestId("trigger-chip-0"));
    expect(screen.getByTestId("trigger-popover")).toBeInTheDocument();
  });

  it("popover shows event type", async () => {
    const user = userEvent.setup();
    render(<PlanTriggerTimeline events={[makePlanCycle({ trigger_reason: "UserRequest" })]} />);
    await user.click(screen.getByTestId("trigger-chip-0"));
    const popover = screen.getByTestId("trigger-popover");
    expect(popover.textContent).toMatch(/PlanCycle/);
  });

  it("popover closes when close button clicked", async () => {
    const user = userEvent.setup();
    render(<PlanTriggerTimeline events={[makePlanCycle()]} />);
    await user.click(screen.getByTestId("trigger-chip-0"));
    expect(screen.getByTestId("trigger-popover")).toBeInTheDocument();
    await user.click(screen.getByTestId("trigger-popover-close"));
    await waitFor(() => expect(screen.queryByTestId("trigger-popover")).toBeNull());
  });

  it("renders events in oldest-left order (reverses newest-first input)", () => {
    // Input is newest-first (as returned by API ring buffer)
    const events: TraceEntry[] = [
      makePlanCycle({ ts: "2026-04-04T10:05:00Z", trigger_reason: "Periodic" }),  // newest
      makeRateChange({ ts: "2026-04-04T09:55:00Z" }),                             // older
    ];
    render(<PlanTriggerTimeline events={events} />);
    const chips = Array.from(document.querySelectorAll('[data-testid^="trigger-chip-"]')) as HTMLElement[];
    expect(chips.length).toBe(2);
    // chip-0 should be the older (RateChange), chip-1 the newer (PlanCycle)
    expect(chips[0].textContent).toMatch(/0\.12|€|Rate/i);  // RateChange
  });

  it("renders all 7 event types without throwing", () => {
    const events: TraceEntry[] = [
      makePlanCycle(),
      makeRateChange(),
      makeCapacityChange(),
      makeOpenAdrArrived(),
      makeOpenAdrExpired(),
      makePacketTransition(),
      { type: "RequestTransition", ts: "2026-04-04T09:30:00Z", request_id: "req-001", asset_id: "ev", from_status: "PENDING", to_status: "SCHEDULED" },
    ];
    render(<PlanTriggerTimeline events={events} />);
    const chips = document.querySelectorAll('[data-testid^="trigger-chip-"]');
    expect(chips.length).toBe(7);
  });
});
