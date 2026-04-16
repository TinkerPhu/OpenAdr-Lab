import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";
import { PlanDecisionMatrix } from "../components/planner/PlanDecisionMatrix";
import type { AssetAllocation, Plan } from "../api/types";

// ─── Helpers ─────────────────────────────────────────────────────────────────

function makeAlloc(overrides: Partial<AssetAllocation> = {}): AssetAllocation {
  return {
    asset_id: "battery",
    power_kw: 3.5,
    surplus_power_kw: 0,
    grid_power_kw: 3.5,
    marginal_value: 0.12,
    cost_eur: 0.029,
    co2_g: 58.3,
    ...overrides,
  };
}

function makePlan(slot0Allocs: AssetAllocation[] = [makeAlloc()]): Plan {
  return {
    id: "plan-001",
    created_at: "2026-04-04T10:00:00Z",
    trigger: "Periodic",
    slots: [
      {
        slot_index: 0,
        start: "2026-04-04T10:00:00Z",
        end: "2026-04-04T10:05:00Z",
        import_tariff_eur_kwh: 0.12,
        export_tariff_eur_kwh: 0.05,
        co2_g_kwh: 200,
        import_cap_kw: 10,
        export_cap_kw: 10,
        allocations: slot0Allocs,
        net_import_kw: 3.5,
        net_export_kw: 0,
        pv_forecast_kw: 2.5,
        baseline_kw: 1.2,
      },
      {
        slot_index: 1,
        start: "2026-04-04T10:05:00Z",
        end: "2026-04-04T10:10:00Z",
        import_tariff_eur_kwh: 0.15,
        export_tariff_eur_kwh: 0.05,
        co2_g_kwh: 220,
        import_cap_kw: 10,
        export_cap_kw: 10,
        allocations: [],
        net_import_kw: 0,
        net_export_kw: 0,
        pv_forecast_kw: 1.8,
        baseline_kw: 1.2,
      },
    ],
    summary: { total_cost_eur: 0.50, total_co2_g: 800, total_import_kwh: 3.0, total_export_kwh: 0 },
    envelopes: [],
    warnings: [],
  };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("PlanDecisionMatrix", () => {
  it("renders empty state when plan is null", () => {
    render(<PlanDecisionMatrix plan={null} />);
    expect(screen.getByTestId("matrix-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("decision-matrix")).toBeNull();
  });

  it("renders empty state when plan is undefined", () => {
    render(<PlanDecisionMatrix plan={undefined} />);
    expect(screen.getByTestId("matrix-empty")).toBeInTheDocument();
  });

  it("renders the decision matrix section when plan has allocations", () => {
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("decision-matrix")).toBeInTheDocument();
  });

  it("renders one row per unique asset_id in allocations", () => {
    const allocs = [
      makeAlloc({ asset_id: "battery" }),
      makeAlloc({ asset_id: "ev" }),
    ];
    render(<PlanDecisionMatrix plan={makePlan(allocs)} />);
    expect(screen.getByTestId("matrix-row-battery")).toBeInTheDocument();
    expect(screen.getByTestId("matrix-row-ev")).toBeInTheDocument();
  });

  it("renders matrix cell for each allocation", () => {
    render(<PlanDecisionMatrix plan={makePlan([makeAlloc({ asset_id: "battery" })])} />);
    expect(screen.getByTestId("matrix-cell-battery-0")).toBeInTheDocument();
  });

  it("renders tariff header row", () => {
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("matrix-tariff-header")).toBeInTheDocument();
  });

  it("opens allocation detail drawer on cell click", async () => {
    const user = userEvent.setup();
    render(<PlanDecisionMatrix plan={makePlan()} />);
    const cell = screen.getByTestId("matrix-cell-battery-0");
    await user.click(cell);
    expect(screen.getByTestId("matrix-drawer")).toBeInTheDocument();
  });

  it("shows power_kw and cost_eur in drawer", async () => {
    const user = userEvent.setup();
    render(<PlanDecisionMatrix plan={makePlan([makeAlloc({ power_kw: 3.50, cost_eur: 0.029 })])} />);
    await user.click(screen.getByTestId("matrix-cell-battery-0"));
    const drawer = screen.getByTestId("matrix-drawer");
    expect(drawer.textContent).toContain("3.50");
    expect(drawer.textContent).toContain("0.0290");
  });

  it("renders the legend", () => {
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("matrix-legend")).toBeInTheDocument();
  });

  it("renders pv forecast row", () => {
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("matrix-row-pv")).toBeInTheDocument();
    expect(screen.getByTestId("matrix-row-pv-cells")).toBeInTheDocument();
  });

  it("renders baseline load row", () => {
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("matrix-row-baseline")).toBeInTheDocument();
    expect(screen.getByTestId("matrix-row-baseline-cells")).toBeInTheDocument();
  });

  it("collapses matrix when collapse button clicked", async () => {
    const user = userEvent.setup();
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("decision-matrix")).toBeInTheDocument();
    await user.click(screen.getByTestId("matrix-collapse-btn"));
    expect(screen.queryByTestId("decision-matrix")).toBeNull();
  });

  it("shows non-zero data-power on allocated cells, zero on idle cells", () => {
    const plan = makePlan([makeAlloc({ asset_id: "battery", power_kw: 5.0 })]);
    render(<PlanDecisionMatrix plan={plan} />);
    const cell0 = screen.getByTestId("matrix-cell-battery-0");
    const cell1 = screen.getByTestId("matrix-cell-battery-1");
    expect(parseFloat(cell0.getAttribute("data-power") ?? "0")).toBeGreaterThan(0);
    expect(parseFloat(cell1.getAttribute("data-power") ?? "0")).toBe(0);
  });
});
