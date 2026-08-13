import { render, screen, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { PlanHistoryPage, dayRangeIso } from "../pages/PlanHistory";

const mockPlans = [
  {
    plan_id: "plan-1",
    created_at: Date.UTC(2026, 0, 1, 6),
    trigger: "PERIODIC",
    solver_ms: 120,
    solve_status: "OPTIMAL",
    objective_eur: 1.2,
    friction_eur: 0.1,
    mip_gap_target: 0.02,
    warning_count: 1,
    warning_kinds: ["STALE_RATE_ESTIMATE"],
    c_energy_eur: 1.0,
    c_grid_eur: 0.1,
    c_wear_eur: 0.05,
    c_violations_eur: 0.0,
    c_peak_penalty_eur: 0.0,
  },
  {
    plan_id: "plan-2",
    created_at: Date.UTC(2026, 0, 1, 7),
    trigger: "RATE_CHANGE",
    solver_ms: null,
    solve_status: "INFEASIBLE",
    objective_eur: 0,
    friction_eur: 0,
    mip_gap_target: null,
    warning_count: 1,
    warning_kinds: ["SOLVER_INFEASIBLE"],
    c_energy_eur: null,
    c_grid_eur: null,
    c_wear_eur: null,
    c_violations_eur: null,
    c_peak_penalty_eur: null,
  },
];

const mockRefetch = vi.hoisted(() => ({ plans: vi.fn() }));

vi.mock("../api/hooks", () => ({
  useHistoryPlans: () => ({ data: mockPlans, refetch: mockRefetch.plans }),
}));

vi.mock("../App", () => ({
  useVenContext: () => ({ venUrl: "http://localhost:8081", venName: "ven-1", setVenUrl: vi.fn(), api: {} }),
}));

function renderPlanHistory() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <PlanHistoryPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("dayRangeIso", () => {
  it("returns a 24h [from, to) window for a UTC calendar day", () => {
    const { fromIso, toIso } = dayRangeIso("2026-01-01");
    expect(fromIso).toBe("2026-01-01T00:00:00.000Z");
    expect(toIso).toBe("2026-01-02T00:00:00.000Z");
  });
});

describe("PlanHistoryPage", () => {
  it("renders the page root", () => {
    renderPlanHistory();
    expect(screen.getByTestId("plan-history-page")).toBeInTheDocument();
  });

  it("renders one row per plan cycle", () => {
    renderPlanHistory();
    expect(screen.getByTestId("plan-history-row-plan-1")).toBeInTheDocument();
    expect(screen.getByTestId("plan-history-row-plan-2")).toBeInTheDocument();
  });

  it("renders the solve-time trend chart when at least one cycle has a solver_ms", () => {
    renderPlanHistory();
    expect(screen.getByTestId("plan-history-solver-ms-chart")).toBeInTheDocument();
    expect(screen.queryByTestId("plan-history-no-solver-ms")).toBeNull();
  });

  it("renders a warning-kind chip for each cycle's warnings", () => {
    renderPlanHistory();
    expect(screen.getAllByTestId("plan-history-warning-kind-STALE_RATE_ESTIMATE").length).toBeGreaterThan(0);
    expect(screen.getAllByTestId("plan-history-warning-kind-SOLVER_INFEASIBLE").length).toBeGreaterThan(0);
  });

  it("shows a dash for a cycle with no recorded solver_ms", () => {
    renderPlanHistory();
    const row = screen.getByTestId("plan-history-row-plan-2");
    expect(row.textContent).toContain("—");
  });

  it("updates the selected date when the date input changes", () => {
    renderPlanHistory();
    const input = screen.getByTestId("plan-history-date-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "2026-02-15" } });
    expect(input.value).toBe("2026-02-15");
  });

  it("returns to rolling last-24h mode when 'Last 24h' is clicked", () => {
    renderPlanHistory();
    const input = screen.getByTestId("plan-history-date-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "2026-02-15" } });
    expect(input.value).toBe("2026-02-15");

    fireEvent.click(screen.getByTestId("plan-history-last-24h-btn"));

    const todayUtc = new Date().toISOString().slice(0, 10);
    expect(input.value).toBe(todayUtc);
  });
});
