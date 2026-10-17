import { fireEvent, render, screen } from "@testing-library/react";
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
  FleetPowerChart: ({
    windowMinutes,
    reportingVens,
    forecast,
    aheadMinutes,
  }: {
    windowMinutes: number;
    reportingVens: number;
    forecast?: { slots: unknown[] };
    aheadMinutes?: number;
  }) => (
    <div
      data-testid="fleet-chart-stub"
      data-reporting={reportingVens}
      data-forecast-slots={forecast?.slots.length ?? 0}
      data-ahead={aheadMinutes}
    >
      {windowMinutes}
    </div>
  ),
}));

vi.mock("../components/FleetTariffChart", () => ({
  FleetTariffChart: ({ windowMinutes, aheadMinutes }: { windowMinutes: number; aheadMinutes?: number }) => (
    <div data-testid="fleet-tariff-stub" data-ahead={aheadMinutes}>
      {windowMinutes}
    </div>
  ),
}));

vi.mock("../api/hooks", () => ({
  useFleetPower: vi.fn(() => ({ data: mockLive, isError: false })),
  useFleetHistory: vi.fn(() => ({ data: mockHistory, isError: false })),
  // The page also hosts the reactions card; it has its own test file, so here
  // it only has to be idle rather than absent.
  useFleetReactions: vi.fn(() => ({ data: undefined, isError: false })),
  useFleetSignals: vi.fn(() => ({ data: undefined, isError: false })),
  useFleetForecast: vi.fn(() => ({
    data: { stepSeconds: 900, slots: [{ start: "2026-10-17T12:15:00Z", kw: 3, vens: 2, objectives: {} }], before: null, assumedUnitVens: [] },
    isError: false,
  })),
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

  it("hands the forecast and one look-ahead to both charts", () => {
    renderFleet();
    const chart = screen.getByTestId("fleet-chart-stub");
    expect(chart).toHaveAttribute("data-forecast-slots", "1");
    expect(chart.getAttribute("data-ahead")).toBe(screen.getByTestId("fleet-tariff-stub").getAttribute("data-ahead"));
    expect(chart).toHaveAttribute("data-ahead", "720");
  });
});
