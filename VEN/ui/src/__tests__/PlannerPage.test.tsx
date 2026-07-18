import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { PlannerPage } from "../pages/Planner";
import type { Plan, TraceEntry, PlanTimeSlot, PlannerEvent, UserRequestWithSession } from "../api/types";

// ─── Mock hooks ───────────────────────────────────────────────────────────────

const mockInvalidateQueries = vi.fn();
vi.mock("@tanstack/react-query", async () => {
  const actual = await vi.importActual("@tanstack/react-query");
  return { ...actual, useQueryClient: () => ({ invalidateQueries: mockInvalidateQueries }) };
});

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  usePlan: vi.fn(),
  useTrace: vi.fn(),
  useRequests: vi.fn(),
  useSim: vi.fn(),
  useSetObjective: vi.fn(),
  usePlannerEvents: vi.fn(),
}));

import { usePlan, useTrace, useRequests, useSim, useSetObjective, usePlannerEvents } from "../api/hooks";

/** Captured SSE callback from the most recent usePlannerEvents call. */
let capturedOnEvent: ((event: PlannerEvent) => void) | null = null;

// ─── Helpers ──────────────────────────────────────────────────────────────────

function makeSlot(overrides: Partial<PlanTimeSlot> = {}): PlanTimeSlot {
  return {
    slot_index: 0,
    start: "2026-04-04T10:00:00Z",
    end: "2026-04-04T10:05:00Z",
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
    slots: [makeSlot()],
    summary: { total_cost_eur: 1.0, total_co2_g: 500, total_import_kwh: 3.0, total_export_kwh: 0 },
    envelopes: [],
    warnings: [],
    objective_eur: 0,
    friction_eur: 0,
    solve_status: "OPTIMAL",
  };
}

function makeMockRequest(): UserRequestWithSession {
  const departure = new Date(Date.now() + 3600000).toISOString();
  return {
    id: "req-0001",
    asset_id: "ev",
    target_energy_kwh: 10,
    target_soc: 0.9,
    desired_power_kw: 7.4,
    completion_policy: "STOP",
    deadlines: [{ latest_end: departure, max_total_cost_eur: 2.0, max_marginal_rate_eur_kwh: null, min_completion: 0.8 }],
    mode: "BY_DEADLINE",
    max_total_cost_eur: 2.0,
    tier_count: 1,
    session_id: "sess-0001",
    session_type: "ev",
    status: "ACTIVE",
    estimated_cost_eur: 1.0,
    estimated_co2_g: 300,
    interruptible: true,
    tolerance_min: null,
    budget_eur: null,
    created_at: "2026-04-04T08:00:00Z",
    updated_at: "2026-04-04T10:00:00Z",
    session: {
      type: "ev", id: "sess-0001", target_soc: 0.9, departure_time: departure,
      soft_deadline: false, mode: "BY_DEADLINE", budget_eur: null,
      created_at: "2026-04-04T08:00:00Z", updated_at: "2026-04-04T10:00:00Z",
    },
  };
}

const mockPlanCycle: TraceEntry = {
  type: "PlanCycle",
  ts: "2026-04-04T10:00:00Z",
  trigger_reason: "Periodic",
  total_slots: 48,
};

const mockTraceEntries: TraceEntry[] = [
  mockPlanCycle,
  {
    type: "RateChange",
    ts: "2026-04-04T09:55:00Z",
    interval_start: "2026-04-04T10:00:00Z",
    import_eur_kwh: 0.3012,
    export_eur_kwh: 0.0821,
  },
];

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("PlannerPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    capturedOnEvent = null;
    vi.mocked(usePlan).mockReturnValue({ data: undefined } as ReturnType<typeof usePlan>);
    vi.mocked(useTrace).mockReturnValue({ data: [] } as unknown as ReturnType<typeof useTrace>);
    vi.mocked(useRequests).mockReturnValue({ data: [] } as unknown as ReturnType<typeof useRequests>);
    vi.mocked(useSim).mockReturnValue({ data: undefined } as unknown as ReturnType<typeof useSim>);
    vi.mocked(useSetObjective).mockReturnValue({ mutate: vi.fn() } as unknown as ReturnType<typeof useSetObjective>);
    vi.mocked(usePlannerEvents).mockImplementation((cb: (e: PlannerEvent) => void) => {
      capturedOnEvent = cb;
    });
  });

  it("renders the planner heading", () => {
    render(<PlannerPage />);
    expect(screen.getByTestId("planner-heading")).toBeInTheDocument();
  });

  it("renders objective legend accordion", () => {
    render(<PlannerPage />);
    expect(screen.getByTestId("objective-legend")).toBeInTheDocument();
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

  it("renders session-board empty state when no requests", () => {
    render(<PlannerPage />);
    expect(screen.getByTestId("session-board-empty")).toBeInTheDocument();
  });

  it("renders plan header content when plan is available", () => {
    vi.mocked(usePlan).mockReturnValue({ data: makeMockPlan() } as ReturnType<typeof usePlan>);
    render(<PlannerPage />);
    expect(screen.getByTestId("plan-header")).toBeInTheDocument();
    expect(screen.getByTestId("plan-trigger-badge")).toBeInTheDocument();
  });

  it("renders decision matrix when plan has allocations", () => {
    const plan = makeMockPlan();
    plan.slots[0].allocations = [{
      asset_id: "battery",
      power_kw: 3.5,
      surplus_power_kw: 0,
      grid_power_kw: 3.5,
      marginal_value: 0.12,
      cost_eur: 0.029,
      co2_g: 58.3,
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

  it("renders session board when requests available", () => {
    vi.mocked(useRequests).mockReturnValue({ data: [makeMockRequest()] } as unknown as ReturnType<typeof useRequests>);
    render(<PlannerPage />);
    expect(screen.getByTestId("session-board")).toBeInTheDocument();
    expect(screen.getByTestId("session-group-active")).toBeInTheDocument();
  });

  // ── Plan E: Planner status SSE tests ──────────────────────────────────────

  it("does not render status bar content when idle", () => {
    render(<PlannerPage />);
    // Wrapper always present for layout stability; inner states absent when idle
    expect(screen.getByTestId("planner-status")).toBeInTheDocument();
    expect(screen.queryByTestId("planner-status-solving")).not.toBeInTheDocument();
    expect(screen.queryByTestId("planner-status-updated")).not.toBeInTheDocument();
  });

  it("shows solving status when solving_started event fires", () => {
    render(<PlannerPage />);
    expect(capturedOnEvent).toBeTruthy();
    act(() => {
      capturedOnEvent!({
        type: "solving_started",
        objective: "min_cost",
        num_slots: 288,
        triggered_at: "2026-04-04T10:00:00Z",
      });
    });
    expect(screen.getByTestId("planner-status-solving")).toBeInTheDocument();
    expect(screen.getByText(/Solving/)).toBeInTheDocument();
  });

  it("updates elapsed time on solving_progress events", () => {
    render(<PlannerPage />);
    act(() => {
      capturedOnEvent!({
        type: "solving_started",
        objective: "min_ghg",
        num_slots: 288,
        triggered_at: "2026-04-04T10:00:00Z",
      });
    });
    act(() => {
      capturedOnEvent!({ type: "solving_progress", elapsed_ms: 5000, iteration: 5 });
    });
    expect(screen.getByText(/5 s/)).toBeInTheDocument();
    expect(screen.getByText(/tick 5/)).toBeInTheDocument();
  });

  it("shows updated chip when plan_ready fires", () => {
    render(<PlannerPage />);
    act(() => {
      capturedOnEvent!({
        type: "plan_ready",
        plan_id: "abc-123",
        objective: "min_cost",
        solver_ms: 23400,
        objective_eur: 1.5,
        friction_eur: 0,
        solve_status: "OPTIMAL",
        slot_count: 288,
        trigger: "Periodic",
      });
    });
    expect(screen.getByTestId("planner-status-updated")).toBeInTheDocument();
    expect(screen.getByText(/23\.4 s/)).toBeInTheDocument();
    expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ["plan"] });
  });

  // ── Embedded trace accordion tests ──────────────────────────────────────────

  it("renders trace accordion collapsed by default", () => {
    render(<PlannerPage />);
    const accordion = screen.getByTestId("trace-accordion");
    expect(accordion).toBeInTheDocument();
    // Table should not be visible when collapsed
    expect(screen.queryByTestId("trace-table")).not.toBeVisible();
  });

  it("expands trace accordion to show table", async () => {
    vi.mocked(useTrace).mockReturnValue({ data: mockTraceEntries } as unknown as ReturnType<typeof useTrace>);
    const user = userEvent.setup();
    render(<PlannerPage />);

    const summary = screen.getByText(/Decision Trace/);
    await user.click(summary);

    expect(screen.getByTestId("trace-table")).toBeVisible();
    expect(screen.getByTestId("trace-row-0")).toBeInTheDocument();
    expect(screen.getByTestId("trace-row-1")).toBeInTheDocument();
  });

  it("shows event count in accordion summary", () => {
    vi.mocked(useTrace).mockReturnValue({ data: mockTraceEntries } as unknown as ReturnType<typeof useTrace>);
    render(<PlannerPage />);
    expect(screen.getByText(/2 events/)).toBeInTheDocument();
  });

  it("shows empty state in trace table when no events", async () => {
    vi.mocked(useTrace).mockReturnValue({ data: [] } as unknown as ReturnType<typeof useTrace>);
    const user = userEvent.setup();
    render(<PlannerPage />);

    await user.click(screen.getByText(/Decision Trace/));
    expect(screen.getByText("No trace events yet")).toBeInTheDocument();
  });
});
