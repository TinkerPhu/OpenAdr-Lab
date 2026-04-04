import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";
import { PacketProgressBoard } from "../components/planner/PacketProgressBoard";
import type { EnergyPacket } from "../api/types";

// ─── Mock data helpers ────────────────────────────────────────────────────────

function makePacket(overrides: Partial<EnergyPacket> = {}): EnergyPacket {
  const futureDeadline = new Date(Date.now() + 2.5 * 60 * 60 * 1000).toISOString(); // +2h30m
  return {
    id: "pkt-0001",
    asset_id: "ev",
    status: "ACTIVE",
    target_energy_kwh: 10.0,
    target_soc: 0.9,
    desired_power_kw: 7.4,
    estimated_cost_eur: 1.20,
    estimated_co2_g: 3600,
    estimated_completion: 0.62,
    accumulated_cost_eur: 0.44,
    value_curve: {
      deadline_tiers: [
        { deadline: futureDeadline, max_total_cost_eur: 1.20, min_completion: 0.8 },
      ],
      active_tier_index: 0,
    },
    created_at: "2026-04-04T08:00:00Z",
    updated_at: "2026-04-04T10:00:00Z",
    ...overrides,
  };
}

const futureDeadline = new Date(Date.now() + 2.5 * 60 * 60 * 1000).toISOString();
const pastDeadline   = new Date(Date.now() - 1 * 60 * 60 * 1000).toISOString();

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("PacketProgressBoard", () => {
  it("renders empty state when packets array is empty", () => {
    render(<PacketProgressBoard packets={[]} />);
    expect(screen.getByTestId("packet-board-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("packet-board")).toBeNull();
  });

  it("renders the board when packets exist", () => {
    render(<PacketProgressBoard packets={[makePacket()]} />);
    expect(screen.getByTestId("packet-board")).toBeInTheDocument();
    expect(screen.queryByTestId("packet-board-empty")).toBeNull();
  });

  it("places ACTIVE packet in Active group", () => {
    render(<PacketProgressBoard packets={[makePacket({ status: "ACTIVE" })]} />);
    const activeGroup = screen.getByTestId("packet-group-active");
    expect(activeGroup).toBeInTheDocument();
    expect(activeGroup.querySelector('[data-testid="packet-card-pkt-0001"]')).toBeTruthy();
  });

  it("places SCHEDULED packet in Queued group", () => {
    render(<PacketProgressBoard packets={[makePacket({ id: "pkt-scheduled", status: "SCHEDULED" })]} />);
    const queuedGroup = screen.getByTestId("packet-group-queued");
    expect(queuedGroup.querySelector('[data-testid="packet-card-pkt-scheduled"]')).toBeTruthy();
  });

  it("places PENDING packet in Queued group", () => {
    render(<PacketProgressBoard packets={[makePacket({ id: "pkt-pending", status: "PENDING" })]} />);
    const queuedGroup = screen.getByTestId("packet-group-queued");
    expect(queuedGroup.querySelector('[data-testid="packet-card-pkt-pending"]')).toBeTruthy();
  });

  it("places COMPLETED packet in Done group", () => {
    render(<PacketProgressBoard packets={[makePacket({ id: "pkt-done", status: "COMPLETED" })]} />);
    const doneGroup = screen.getByTestId("packet-group-done");
    expect(doneGroup.querySelector('[data-testid="packet-card-pkt-done"]')).toBeTruthy();
  });

  it("places ABANDONED packet in Done group", () => {
    render(<PacketProgressBoard packets={[makePacket({ id: "pkt-abandoned", status: "ABANDONED" })]} />);
    const doneGroup = screen.getByTestId("packet-group-done");
    expect(doneGroup.querySelector('[data-testid="packet-card-pkt-abandoned"]')).toBeTruthy();
  });

  it("renders fill gauge with success color when > 80%", () => {
    render(<PacketProgressBoard packets={[makePacket({ estimated_completion: 0.85 })]} />);
    const gauge = screen.getByTestId("packet-fill-pkt-0001");
    expect(gauge).toBeInTheDocument();
    expect(gauge.getAttribute("data-color") ?? gauge.className).toMatch(/success/);
  });

  it("renders fill gauge with warning color when 40-80%", () => {
    render(<PacketProgressBoard packets={[makePacket({ estimated_completion: 0.62 })]} />);
    const gauge = screen.getByTestId("packet-fill-pkt-0001");
    expect(gauge.getAttribute("data-color") ?? gauge.className).toMatch(/warning/);
  });

  it("renders fill gauge with error color when < 40%", () => {
    render(<PacketProgressBoard packets={[makePacket({ estimated_completion: 0.20 })]} />);
    const gauge = screen.getByTestId("packet-fill-pkt-0001");
    expect(gauge.getAttribute("data-color") ?? gauge.className).toMatch(/error/);
  });

  it("shows deadline countdown for future deadline", () => {
    render(<PacketProgressBoard packets={[makePacket()]} />);
    const deadline = screen.getByTestId("packet-deadline-pkt-0001");
    expect(deadline).toBeInTheDocument();
    // Should contain T− prefix or hours/minutes
    expect(deadline.textContent).toMatch(/T[−-]|h|m/);
  });

  it("shows OVERDUE for past deadline", () => {
    render(<PacketProgressBoard packets={[makePacket({
      value_curve: {
        deadline_tiers: [{ deadline: pastDeadline, max_total_cost_eur: 1.20, min_completion: 0.8 }],
        active_tier_index: 0,
      },
    })]} />);
    const deadline = screen.getByTestId("packet-deadline-pkt-0001");
    expect(deadline.textContent?.toUpperCase()).toContain("OVERDUE");
  });

  it("omits deadline field when deadline_tiers is empty", () => {
    render(<PacketProgressBoard packets={[makePacket({
      value_curve: { deadline_tiers: [], active_tier_index: 0 },
    })]} />);
    expect(screen.queryByTestId("packet-deadline-pkt-0001")).toBeNull();
  });

  it("shows budget bar when max_total_cost_eur is set", () => {
    render(<PacketProgressBoard packets={[makePacket()]} />);
    expect(screen.getByTestId("packet-budget-pkt-0001")).toBeInTheDocument();
  });

  it("omits budget bar when max_total_cost_eur is null", () => {
    render(<PacketProgressBoard packets={[makePacket({
      value_curve: {
        deadline_tiers: [{ deadline: futureDeadline, max_total_cost_eur: null, min_completion: 0.8 }],
        active_tier_index: 0,
      },
    })]} />);
    expect(screen.queryByTestId("packet-budget-pkt-0001")).toBeNull();
  });

  it("shows deadline tiers table after expanding card", async () => {
    const user = userEvent.setup();
    render(<PacketProgressBoard packets={[makePacket()]} />);
    expect(screen.queryByTestId("packet-tiers-pkt-0001")).toBeNull();
    await user.click(screen.getByTestId("packet-expand-pkt-0001"));
    expect(screen.getByTestId("packet-tiers-pkt-0001")).toBeInTheDocument();
  });
});
