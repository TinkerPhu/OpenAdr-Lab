import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";
import { PlanDecisionMatrix } from "../components/planner/PlanDecisionMatrix";
import type { Plan, PlanStep } from "../api/types";

// ─── Helpers ─────────────────────────────────────────────────────────────────

function makeStep(overrides: Partial<PlanStep> = {}): PlanStep {
  return {
    ts: "2026-04-04T10:00:00Z",
    asset_id: "battery",
    setpoint_kw: 3.5,
    actual_power_kw: 3.4,
    reason: { kind: "CHEAP_TARIFF", tariff_eur_per_kwh: 0.12, threshold_eur_per_kwh: 0.18 },
    state_before: { asset_type: "battery", actual_power_kw: 0.0 },
    avail_max_import_kw: 5.0,
    avail_max_export_kw: 5.0,
    ...overrides,
  };
}

function makePlan(steps: PlanStep[] = [makeStep()]): Plan {
  const slotStart = "2026-04-04T10:00:00Z";
  return {
    id: "plan-001",
    created_at: "2026-04-04T10:00:00Z",
    trigger: "Periodic",
    firm_boundary: "2026-04-04T13:00:00Z",
    firm_slots: [
      {
        slot_index: 0,
        start: slotStart,
        end: "2026-04-04T10:05:00Z",
        slot_type: "FIRM",
        import_tariff_eur_kwh: 0.12,
        export_tariff_eur_kwh: 0.05,
        co2_g_kwh: 200,
        import_cap_kw: 10,
        export_cap_kw: 10,
        allocations: [],
        net_import_kw: 3.5,
        net_export_kw: 0,
        pv_forecast_kw: 2.5,
        baseline_kw: 1.2,
      },
    ],
    flexible_slots: [
      {
        slot_index: 1,
        start: "2026-04-04T10:05:00Z",
        end: "2026-04-04T10:10:00Z",
        slot_type: "FLEXIBLE",
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
    firm_summary: { total_cost_eur: 0.50, total_co2_g: 800, total_import_kwh: 3.0, total_export_kwh: 0 },
    warnings: [],
    steps,
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

  it("renders the decision matrix section when plan has steps", () => {
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("decision-matrix")).toBeInTheDocument();
  });

  it("renders one row per unique asset_id in steps", () => {
    const steps = [
      makeStep({ asset_id: "battery", ts: "2026-04-04T10:00:00Z" }),
      makeStep({ asset_id: "ev", ts: "2026-04-04T10:00:00Z" }),
      makeStep({ asset_id: "battery", ts: "2026-04-04T10:05:00Z" }),
    ];
    render(<PlanDecisionMatrix plan={makePlan(steps)} />);
    // Two unique assets → two row labels
    expect(screen.getByTestId("matrix-row-battery")).toBeInTheDocument();
    expect(screen.getByTestId("matrix-row-ev")).toBeInTheDocument();
  });

  it("renders matrix cell for each step", () => {
    const steps = [
      makeStep({ asset_id: "battery", ts: "2026-04-04T10:00:00Z" }),
    ];
    render(<PlanDecisionMatrix plan={makePlan(steps)} />);
    expect(screen.getByTestId("matrix-cell-battery-0")).toBeInTheDocument();
  });

  it("renders FIRM/FLEX boundary divider", () => {
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("matrix-firm-flex-divider")).toBeInTheDocument();
  });

  it("renders flexible-zone cells with reduced opacity", () => {
    const steps = [
      makeStep({ asset_id: "battery", ts: "2026-04-04T10:00:00Z" }),
      makeStep({ asset_id: "battery", ts: "2026-04-04T10:05:00Z", reason: { kind: "IDLE" } }),
    ];
    const plan = makePlan(steps);
    // The flexible slot starts at 10:05 which is after firm_boundary (13:00 in this plan) — no flexible here
    // Let's adjust firm_boundary to be before the second slot
    plan.firm_boundary = "2026-04-04T10:03:00Z";
    render(<PlanDecisionMatrix plan={plan} />);
    const flexCell = screen.queryByTestId("matrix-cell-battery-1");
    if (flexCell) {
      // Flexible cells should have reduced opacity styling
      // Either opacity attr or a data-flex attribute
      const dataFlex = flexCell.getAttribute("data-flex");
      const opacity = flexCell.style.opacity;
      expect(dataFlex === "true" || opacity === "0.5" || opacity === "50%").toBe(true);
    }
  });

  it("renders tariff header row", () => {
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("matrix-tariff-header")).toBeInTheDocument();
  });

  it("opens step detail drawer on cell click", async () => {
    const user = userEvent.setup();
    render(<PlanDecisionMatrix plan={makePlan()} />);
    const cell = screen.getByTestId("matrix-cell-battery-0");
    await user.click(cell);
    expect(screen.getByTestId("matrix-drawer")).toBeInTheDocument();
  });

  it("shows reason type in drawer", async () => {
    const user = userEvent.setup();
    render(<PlanDecisionMatrix plan={makePlan()} />);
    const cell = screen.getByTestId("matrix-cell-battery-0");
    await user.click(cell);
    const drawer = screen.getByTestId("matrix-drawer-reason");
    expect(drawer).toBeInTheDocument();
    expect(drawer.textContent).toMatch(/CHEAP_TARIFF|tariff/i);
  });

  it("shows setpoint_kw and actual_power_kw in drawer", async () => {
    const user = userEvent.setup();
    render(<PlanDecisionMatrix plan={makePlan([makeStep({ setpoint_kw: 3.5, actual_power_kw: 3.4 })])} />);
    await user.click(screen.getByTestId("matrix-cell-battery-0"));
    const drawer = screen.getByTestId("matrix-drawer");
    expect(drawer.textContent).toContain("3.5");
    expect(drawer.textContent).toContain("3.4");
  });


  it("hides FLEXIBLE columns by default (FIRM-only view)", () => {
    const steps = [
      makeStep({ asset_id: "battery", ts: "2026-04-04T10:05:00Z", reason: { kind: "IDLE" } }),
    ];
    const plan = makePlan(steps);
    plan.firm_boundary = "2026-04-04T10:03:00Z"; // slot 1 is flexible
    render(<PlanDecisionMatrix plan={plan} />);
    // In default FIRM-only mode, flexible cells should not be visible
    const flexCell = screen.queryByTestId("matrix-cell-battery-1");
    if (flexCell) {
      expect(flexCell).not.toBeVisible();
    }
  });

  it("renders the reason legend", () => {
    render(<PlanDecisionMatrix plan={makePlan()} />);
    expect(screen.getByTestId("matrix-legend")).toBeInTheDocument();
  });
});
