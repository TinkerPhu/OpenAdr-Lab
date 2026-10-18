import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { PlannerPage } from "../pages/Planner";
import type { Plan, ControllerEvent, PlannerEvent, UserRequestWithSession } from "../api/types";

// ─── Mock hooks ───────────────────────────────────────────────────────────────

const mockInvalidateQueries = vi.fn();
vi.mock("@tanstack/react-query", async () => {
  const actual = await vi.importActual("@tanstack/react-query");
  return { ...actual, useQueryClient: () => ({ invalidateQueries: mockInvalidateQueries }) };
});

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  usePlan: vi.fn(),
  usePlanObjective: vi.fn(() => ({ data: undefined })),
  useTrace: vi.fn(),
  useRequests: vi.fn(),
  useSim: vi.fn(),
  useSetObjective: vi.fn(),
  usePlannerEvents: vi.fn(),
  useAllTimelines: () => ({ data: { zones: [], timelines: {} }, refetch: vi.fn() }),
}));

import { usePlan, usePlanObjective, useTrace, useRequests, useSim, useSetObjective, usePlannerEvents } from "../api/hooks";
import { makePlan, makeSlot } from "./fixtures/plan";

/** Captured SSE callback from the most recent usePlannerEvents call. */
let capturedOnEvent: ((event: PlannerEvent) => void) | null = null;

// ─── Helpers ──────────────────────────────────────────────────────────────────

function makeMockPlan(): Plan {
  return makePlan();
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
    comfort_rates: [],
    status: "ACTIVE",
    estimated_cost_eur: 1.0,
    estimated_co2_g: 300,
    accumulated_cost_eur: 0.4,
    interruptible: true,
    tolerance_min: null,
    budget_eur: null,
    created_at: "2026-04-04T08:00:00Z",
    updated_at: "2026-04-04T10:00:00Z",
    session: {
      type: "ev", asset_id: "ev", origin: "USER_REQUEST", comfort_rates: [], id: "sess-0001", target_soc: 0.9, departure_time: departure,
      window_start: "2026-04-04T08:00:00Z", expected_trip_distance_km: null,
      expected_return_time: null,
      soft_deadline: false, mode: "BY_DEADLINE", budget_eur: null,
      created_at: "2026-04-04T08:00:00Z", updated_at: "2026-04-04T10:00:00Z",
    },
  };
}

const mockPlanCycle: ControllerEvent = {
  type: "PlanCycle",
  ts: "2026-04-04T10:00:00Z",
  trigger_reason: "PERIODIC",
  event_ids: [],
  total_slots: 48,
};

const mockTraceEntries: ControllerEvent[] = [
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

  it("shows who set the objective in force, and selects it", () => {
    vi.mocked(usePlanObjective).mockReturnValue({
      data: {
        objective: "opportunistic",
        source: "fleet",
        set_at: "2026-10-16T12:00:00Z",
        profile_objective: "min_cost",
        available_objectives: ["min_cost", "opportunistic"],
      },
    } as unknown as ReturnType<typeof usePlanObjective>);
    render(<PlannerPage />);
    expect(screen.getByTestId("objective-source")).toHaveTextContent(/set by the fleet/);
    expect(screen.getByTestId("objective-select")).toHaveTextContent("Opportunistic");
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
        trigger: "PERIODIC",
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

    // BL-38: Decision Trace is now nested inside the collapsed Diagnostics zone.
    await user.click(screen.getByText("Diagnostics"));
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

    await user.click(screen.getByText("Diagnostics"));
    await user.click(screen.getByText(/Decision Trace/));
    expect(screen.getByText("No trace events yet")).toBeInTheDocument();
  });

  // ── BL-38: user/diagnostic layout split ───────────────────────────────────

  it("renders the diagnostics zone collapsed by default, below the user zone", () => {
    render(<PlannerPage />);
    const userZone = screen.getByTestId("planner-user-zone");
    const diagnostics = screen.getByTestId("planner-diagnostics-accordion");
    expect(userZone).toBeInTheDocument();
    expect(diagnostics).toBeInTheDocument();
    // User zone precedes the diagnostics accordion in document order.
    expect(
      userZone.compareDocumentPosition(diagnostics) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    // Diagnostics content (e.g. the trigger-timeline root) is not visible until expanded.
    expect(screen.getByTestId("trigger-timeline")).not.toBeVisible();
  });

  it("expanding the diagnostics zone reveals the decision matrix and trigger history", async () => {
    const user = userEvent.setup();
    render(<PlannerPage />);
    await user.click(screen.getByText("Diagnostics"));
    expect(screen.getByTestId("trigger-timeline")).toBeVisible();
    expect(screen.getByTestId("matrix-empty")).toBeVisible();
  });

  // ── BL-38: matrix-slot → trace filtering ──────────────────────────────────

  it("clicking a matrix slot filters the trace table to that slot's window", async () => {
    const plan = makeMockPlan();
    plan.slots = [
      makeSlot({ slot_index: 0, start: "2026-04-04T10:00:00Z", end: "2026-04-04T10:05:00Z" }),
      makeSlot({ slot_index: 1, start: "2026-04-04T10:05:00Z", end: "2026-04-04T10:10:00Z" }),
    ];
    vi.mocked(usePlan).mockReturnValue({ data: plan } as ReturnType<typeof usePlan>);
    const inSlot0: ControllerEvent = { type: "PlanCycle", ts: "2026-04-04T10:02:00Z", trigger_reason: "PERIODIC", event_ids: [], total_slots: 2 };
    const inSlot1: ControllerEvent = { type: "PlanCycle", ts: "2026-04-04T10:07:00Z", trigger_reason: "PERIODIC", event_ids: [], total_slots: 2 };
    vi.mocked(useTrace).mockReturnValue({ data: [inSlot0, inSlot1] } as unknown as ReturnType<typeof useTrace>);

    const user = userEvent.setup();
    render(<PlannerPage />);
    await user.click(screen.getByText("Diagnostics"));
    await user.click(screen.getByText(/Decision Trace/));
    expect(screen.getByTestId("trace-row-0")).toBeInTheDocument();
    expect(screen.getByTestId("trace-row-1")).toBeInTheDocument();

    await user.click(screen.getByTestId("matrix-slot-0"));

    expect(screen.getByText(/1 events/)).toBeInTheDocument();
    expect(screen.getByTestId("trace-row-0")).toBeInTheDocument();
    expect(screen.queryByTestId("trace-row-1")).not.toBeInTheDocument();
    expect(screen.getByTestId("trace-slot-filter-chip")).toBeInTheDocument();
  });

  it("clicking the same slot again clears the filter", async () => {
    const plan = makeMockPlan();
    plan.slots = [
      makeSlot({ slot_index: 0, start: "2026-04-04T10:00:00Z", end: "2026-04-04T10:05:00Z" }),
    ];
    vi.mocked(usePlan).mockReturnValue({ data: plan } as ReturnType<typeof usePlan>);
    vi.mocked(useTrace).mockReturnValue({ data: mockTraceEntries } as unknown as ReturnType<typeof useTrace>);

    const user = userEvent.setup();
    render(<PlannerPage />);
    await user.click(screen.getByText("Diagnostics"));
    await user.click(screen.getByTestId("matrix-slot-0"));
    expect(screen.getByTestId("trace-slot-filter-chip")).toBeInTheDocument();

    await user.click(screen.getByTestId("matrix-slot-0"));
    expect(screen.queryByTestId("trace-slot-filter-chip")).not.toBeInTheDocument();
  });

  it("the filter chip's delete button clears the filter without re-toggling the slot", async () => {
    const plan = makeMockPlan();
    plan.slots = [
      makeSlot({ slot_index: 0, start: "2026-04-04T10:00:00Z", end: "2026-04-04T10:05:00Z" }),
    ];
    vi.mocked(usePlan).mockReturnValue({ data: plan } as ReturnType<typeof usePlan>);
    vi.mocked(useTrace).mockReturnValue({ data: mockTraceEntries } as unknown as ReturnType<typeof useTrace>);

    const user = userEvent.setup();
    render(<PlannerPage />);
    await user.click(screen.getByText("Diagnostics"));
    await user.click(screen.getByTestId("matrix-slot-0"));

    const chip = screen.getByTestId("trace-slot-filter-chip");
    // MUI Chip renders exactly one <svg> (the delete icon) when onDelete is set.
    const deleteIcon = chip.querySelector("svg");
    await user.click(deleteIcon!);

    expect(screen.queryByTestId("trace-slot-filter-chip")).not.toBeInTheDocument();
  });
});
