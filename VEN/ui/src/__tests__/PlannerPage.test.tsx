import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { PlannerPage } from "../pages/Planner";
import type { Plan, EnergyPacket, TraceEntry, PlanTimeSlot } from "../api/types";

// ─── Mock hooks ───────────────────────────────────────────────────────────────

vi.mock("../api/hooks", () => ({
  usePlan: vi.fn(),
  useTrace: vi.fn(),
  usePackets: vi.fn(),
}));

import { usePlan, useTrace, usePackets } from "../api/hooks";

// ─── Helpers ──────────────────────────────────────────────────────────────────

function makeSlot(overrides: Partial<PlanTimeSlot> = {}): PlanTimeSlot {
  return {
    slot_index: 0,
    start: "2026-04-04T10:00:00Z",
    end: "2026-04-04T10:05:00Z",
    slot_type: "FIRM",
    import_tariff_eur_kwh: 0.12,
    export_tariff_eur_kwh: 0.05,
    co2_g_kwh: 200,
    import_cap_kw: 10,
    export_cap_kw: 10,
    allocations: [],
    net_import_kw: 0,
    net_export_kw: 0,
    pv_forecast_kw: 0,
    baseline_kw: 1.0,
    ...overrides,
  };
}

function makeMockPlan(): Plan {
  return {
    id: "plan-001",
    created_at: "2026-04-04T10:00:00Z",
    trigger: "Periodic",
    firm_boundary: "2026-04-04T13:00:00Z",
    firm_slots: [makeSlot()],
    flexible_slots: [],
    firm_summary: { total_cost_eur: 1.0, total_co2_g: 500, total_import_kwh: 3.0, total_export_kwh: 0 },
    warnings: [],
    steps: [],
  };
}

function makeMockPacket(): EnergyPacket {
  return {
    id: "pkt-0001",
    asset_id: "ev",
    status: "ACTIVE",
    target_energy_kwh: 10,
    target_soc: 0.9,
    desired_power_kw: 7.4,
    estimated_cost_eur: 1.0,
    estimated_co2_g: 300,
    estimated_completion: 0.5,
    accumulated_cost_eur: 0.5,
    value_curve: {
      deadline_tiers: [{ deadline: new Date(Date.now() + 3600000).toISOString(), max_total_cost_eur: 2.0, min_completion: 0.8 }],
      active_tier_index: 0,
    },
    created_at: "2026-04-04T08:00:00Z",
    updated_at: "2026-04-04T10:00:00Z",
  };
}

const mockPlanCycle: TraceEntry = {
  type: "PlanCycle",
  ts: "2026-04-04T10:00:00Z",
  trigger_reason: "Periodic",
  firm_slots: 12,
  flexible_slots: 36,
};

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("PlannerPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(usePlan).mockReturnValue({ data: undefined } as ReturnType<typeof usePlan>);
    vi.mocked(useTrace).mockReturnValue({ data: [] } as unknown as ReturnType<typeof useTrace>);
    vi.mocked(usePackets).mockReturnValue({ data: [] } as unknown as ReturnType<typeof usePackets>);
  });

  it("renders the planner heading", () => {
    render(<PlannerPage />);
    expect(screen.getByTestId("planner-heading")).toBeInTheDocument();
  });

  it("renders plan-header section root", () => {
    render(<PlannerPage />);
    // Empty state — no-plan shown
    expect(screen.getByTestId("plan-no-plan")).toBeInTheDocument();
  });

  it("renders trigger-timeline section root", () => {
    render(<PlannerPage />);
    expect(screen.getByTestId("trigger-timeline")).toBeInTheDocument();
  });

  it("renders decision-matrix empty state when no plan", () => {
    render(<PlannerPage />);
    expect(screen.getByTestId("matrix-empty")).toBeInTheDocument();
  });

  it("renders packet-board empty state when no packets", () => {
    render(<PlannerPage />);
    expect(screen.getByTestId("packet-board-empty")).toBeInTheDocument();
  });

  it("renders plan header content when plan is available", () => {
    vi.mocked(usePlan).mockReturnValue({ data: makeMockPlan() } as ReturnType<typeof usePlan>);
    render(<PlannerPage />);
    expect(screen.getByTestId("plan-header")).toBeInTheDocument();
    expect(screen.getByTestId("plan-trigger-badge")).toBeInTheDocument();
  });

  it("renders decision matrix when plan has steps", () => {
    const plan = makeMockPlan();
    plan.steps = [{
      ts: "2026-04-04T10:00:00Z",
      asset_id: "battery",
      setpoint_kw: 3.5,
      actual_power_kw: 3.4,
      reason: { kind: "CHEAP_TARIFF", tariff_eur_per_kwh: 0.12, threshold_eur_per_kwh: 0.18 },
      state_before: { asset_type: "battery", actual_power_kw: 0.0 },
      avail_max_import_kw: 5.0,
      avail_max_export_kw: 5.0,
    }];
    vi.mocked(usePlan).mockReturnValue({ data: plan } as ReturnType<typeof usePlan>);
    render(<PlannerPage />);
    expect(screen.getByTestId("decision-matrix")).toBeInTheDocument();
  });

  it("renders trigger chips when events available", () => {
    vi.mocked(useTrace).mockReturnValue({ data: [mockPlanCycle] } as unknown as ReturnType<typeof useTrace>);
    render(<PlannerPage />);
    expect(screen.getByTestId("trigger-chip-0")).toBeInTheDocument();
  });

  it("renders packet board when packets available", () => {
    vi.mocked(usePackets).mockReturnValue({ data: [makeMockPacket()] } as unknown as ReturnType<typeof usePackets>);
    render(<PlannerPage />);
    expect(screen.getByTestId("packet-board")).toBeInTheDocument();
    expect(screen.getByTestId("packet-group-active")).toBeInTheDocument();
  });
});
