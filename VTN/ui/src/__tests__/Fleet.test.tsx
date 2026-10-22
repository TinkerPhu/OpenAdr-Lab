import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { FleetPage } from "../pages/Fleet";

const mockLive = {
  source: "live" as const,
  vens: [
    {
      venName: "ven-1",
      netPowerKw: 4,
      state: "online",
      receivedAt: new Date().toISOString(),
      objective: {
        objective: "min_cost",
        source: "profile" as const,
        set_at: null,
        profile_objective: "min_cost",
        available_objectives: ["min_cost", "opportunistic"],
      },
      command: { objective: "opportunistic", requested_at: "2026-10-17T12:00:00Z" },
      commandPending: true,
      objectiveChangedAt: "2026-09-22T09:10:00Z",
    },
    {
      venName: "ven-2",
      netPowerKw: -1.5,
      state: "online",
      receivedAt: new Date().toISOString(),
    },
    {
      venName: "ven-3",
      netPowerKw: null,
      state: "offline",
      receivedAt: new Date().toISOString(),
    },
  ],
  fleet: { netPowerKw: 2.5, contributingVens: 2, knownVens: 3 },
};

const mockHistory = {
  source: "raw" as const,
  from: "2026-09-22T09:00:00Z",
  to: "2026-09-22T10:00:00Z",
  stepSeconds: 60,
  vens: [],
  fleet: [
    { ts: "2026-09-22T09:00:00Z", netPowerKw: 1, contributingVens: 2 },
    { ts: "2026-09-22T09:01:00Z", netPowerKw: 2, contributingVens: 2 },
  ],
};

const setVen = vi.fn();
const setFleet = vi.fn();

vi.mock("../components/VenPlanChart", () => ({
  VenPlanChart: ({ plan }: { plan: { objective: string } }) => (
    <div data-testid="ven-plan-stub">{plan.objective}</div>
  ),
}));

// The chart has its own test file; here it only has to not be recharts.
vi.mock("../components/FleetPowerChart", () => ({
  FleetPowerChart: ({ windowMinutes, reportingVens }: { windowMinutes: number; reportingVens: number }) => (
    <div data-testid="fleet-chart-stub" data-reporting={reportingVens}>
      {windowMinutes}
    </div>
  ),
}));

vi.mock("../components/FleetTariffChart", () => ({
  FleetTariffChart: ({ windowMinutes }: { windowMinutes: number }) => (
    <div data-testid="fleet-tariff-stub">{windowMinutes}</div>
  ),
}));

const mockPlan = {
  at: "2026-09-22T09:30:00Z",
  stepSeconds: 300,
  points: [
    { ts: "2026-09-22T09:30:00Z", netImportKw: 4, netExportKw: 0, contributingVens: 2 },
    { ts: "2026-09-22T09:35:00Z", netImportKw: 0, netExportKw: 1.5, contributingVens: 2 },
  ],
  plans: [
    { venName: "ven-1", planId: "a", createdAt: "2026-09-22T09:00:00Z" },
    { venName: "ven-2", planId: "b", createdAt: "2026-09-22T09:20:00Z" },
  ],
  vensWithoutPlan: ["ven-3"],
};
const useFleetPlanMock = vi.fn<(atIso?: string, enabled?: boolean) => { data: typeof mockPlan; isError: boolean }>(
  () => ({ data: mockPlan, isError: false }),
);

vi.mock("../components/FleetPlanChart", () => ({
  FleetPlanChart: ({ plan, reference }: { plan: { points: unknown[] }; reference?: { at: string } }) => (
    <div data-testid="fleet-plan-chart-stub" data-reference={reference?.at ?? ""}>
      {plan.points.length}
    </div>
  ),
}));

vi.mock("../api/hooks", () => ({
  useFleetPlan: (atIso?: string, enabled?: boolean) => useFleetPlanMock(atIso, enabled),
  useFleetPower: vi.fn(() => ({ data: mockLive, isError: false })),
  useFleetHistory: vi.fn(() => ({ data: mockHistory, isError: false })),
  // The page also hosts the reactions card; it has its own test file, so here
  // it only has to be idle rather than absent.
  useFleetReactions: vi.fn(() => ({ data: undefined, isError: false })),
  useFleetSignals: vi.fn(() => ({ data: undefined, isError: false })),
  // The service-level card has its own test file (ServiceDelivered.test.tsx); here it is idle.
  useFleetService: vi.fn(() => ({ data: undefined, isError: false })),
  useVenPlan: vi.fn(() => ({ data: { objective: "opportunistic", solve_status: "SIMULATED", slots: [] }, isError: false })),
  useSetVenObjective: vi.fn(() => ({ mutate: setVen, isError: false })),
  useSetFleetObjective: vi.fn(() => ({ mutate: setFleet, isError: false })),
}));

function renderFleet() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <FleetPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("FleetPage", () => {
  /* What the fleet intends, beside what it did: the plan feed would otherwise be a server-side
   * capability with no surface (ui-transparency). */
  it("shows the planned fleet import and export, and how many VENs have a plan", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-plan-chart-stub")).toHaveTextContent("2");
    expect(screen.getByTestId("fleet-plan-contributors")).toHaveTextContent("2 of 3 VENs have a plan");
  });

  it("says when the newest plan was adopted", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-plan-contributors")).toHaveTextContent("newest adopted");
  });

  it("says so when there is no plan history, like the other fleet cards", () => {
    useFleetPlanMock.mockReturnValueOnce({ data: undefined as never, isError: true });
    renderFleet();
    expect(screen.getByTestId("fleet-plan-error")).toHaveTextContent("telemetry store is not connected");
    expect(screen.queryByTestId("fleet-plan-chart-stub")).toBeNull();
  });

  it("shows the fleet total in kW", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-total")).toHaveTextContent("2.50 kW");
  });

  /* The total is only as complete as the VENs behind it. Showing how many
   * contributed is what lets a reader judge it rather than trust it. */
  it("says how many VENs the total is built from", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-contributors")).toHaveTextContent("2 of 3");
  });

  /* A VEN that has not reported must not appear as 0.00 kW: that is a claim
   * about what it is drawing, and we do not have one. */
  it("shows a silent VEN as no reading rather than zero", () => {
    renderFleet();
    const row = screen.getByTestId("fleet-ven-ven-3");
    expect(row).toHaveTextContent("—");
    expect(row).not.toHaveTextContent("0.00 kW");
  });

  it("keeps an exporting VEN negative", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-ven-ven-2")).toHaveTextContent("-1.50 kW");
  });

  /* A last-will `offline` is what distinguishes a dead VEN from a quiet one,
   * so it has to reach the page. */
  it("shows the last-will state of a VEN that died", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-ven-ven-3")).toHaveTextContent("offline");
  });

  it("draws the per-VEN chart over the selected window", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-chart-stub")).toHaveTextContent("1440");
  });

  /* The window picker is what makes "did it react" and "what did today look
   * like" the same page rather than two. */
  it("puts the price above the power it explains", () => {
    renderFleet();
    const tariff = screen.getByTestId("fleet-tariff-stub");
    const power = screen.getByTestId("fleet-chart-stub");
    // Reading order is the point: the tariff is context for the curves below.
    expect(tariff.compareDocumentPosition(power)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  // The chart cannot tell an empty store from a filling one without knowing whether anything is
  // reporting now, and the page already holds that number (R-99).
  it("tells the chart how many VENs are reporting now", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-chart-stub")).toHaveAttribute("data-reporting", "2");
  });

  it("moves both charts with one window selector", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-tariff-stub")).toHaveTextContent("1440");
    expect(screen.getByTestId("fleet-chart-stub")).toHaveTextContent("1440");
  });

  it("offers the operator a window to choose", async () => {
    renderFleet();
    expect(screen.getByTestId("fleet-window-select")).toBeInTheDocument();
  });

  /* A store that is not connected is not an empty hour. Saying so beats an
   * empty chart that looks like a quiet fleet. */
  it("explains a missing history instead of drawing an empty chart", async () => {
    const { useFleetHistory } = await import("../api/hooks");
    vi.mocked(useFleetHistory).mockReturnValueOnce({
      data: undefined,
      isError: true,
    } as ReturnType<typeof useFleetHistory>);
    renderFleet();
    expect(screen.getByTestId("fleet-history-error")).toBeVisible();
    expect(screen.queryByTestId("sparkline")).not.toBeInTheDocument();
  });

  // openspec opportunistic-objective D12: the objective in force, who set it, and a fleet command
  // still on its way.
  it("shows each VEN's objective, its source and a pending command", () => {
    renderFleet();
    const cell = screen.getByTestId("fleet-objective-ven-1");
    expect(cell).toHaveTextContent("min_cost");
    expect(cell).toHaveTextContent("profile");
    expect(screen.getByTestId("fleet-pending-ven-1")).toHaveTextContent("pending: opportunistic");
    expect(screen.getByTestId("fleet-objective-ven-2")).toHaveTextContent("—");
  });

  it("releases one VEN's command", () => {
    setVen.mockClear();
    renderFleet();
    fireEvent.click(screen.getByTestId("fleet-release-ven-1"));
    expect(setVen).toHaveBeenCalledWith({ venName: "ven-1", objective: null });
    expect(screen.getByTestId("fleet-release-ven-2")).toBeDisabled();
  });

  it("switches and releases the whole fleet", () => {
    setFleet.mockClear();
    renderFleet();
    fireEvent.change(screen.getByTestId("fleet-objective-select"), { target: { value: "opportunistic" } });
    fireEvent.click(screen.getByTestId("fleet-objective-apply"));
    fireEvent.click(screen.getByTestId("fleet-objective-release"));
    expect(setFleet.mock.calls).toEqual([["opportunistic"], [null]]);
  });

  it("opens a VEN's plan under its row", () => {
    renderFleet();
    expect(screen.queryByTestId("fleet-plan-row-ven-1")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("fleet-expand-ven-1"));
    expect(screen.getByTestId("fleet-plan-row-ven-1")).toHaveTextContent("opportunistic");
  });

  // openspec opportunistic-objective D12: what an objective change did to what the fleet intends.
  // The switch is compared like a published tariff (PlanComparison): the plan as of just before the
  // first VEN changed against the plan now, and how many VENs have replanned since.
  it("compares the plan from before the objective switch with the plan now", () => {
    useFleetPlanMock.mockClear();
    renderFleet();
    expect(useFleetPlanMock).toHaveBeenCalledWith("2026-09-22T09:09:59.000Z", undefined);
    expect(screen.getByTestId("plan-replan-progress")).toHaveTextContent("of 3 VENs replanned");
    expect(screen.getByTestId("fleet-plan-chart-stub")).toHaveAttribute("data-reference", mockPlan.at);
    expect(screen.getByTestId("fleet-plan-objectives")).toHaveTextContent("Objectives: min_cost 1");
    expect(screen.getByTestId("fleet-plan-objectives")).toHaveTextContent("objective switched at");
  });

  // fleet-service-level: beside the plan comparison, what the switch did to what owners received.
  it("shows the service delivered card once an objective switch is known", () => {
    renderFleet();
    expect(within(screen.getByTestId("fleet-service-card")).getByTestId("service-loading")).toBeInTheDocument();
  });
});
