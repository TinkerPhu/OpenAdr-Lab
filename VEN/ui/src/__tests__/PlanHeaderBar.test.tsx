import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, afterEach } from "vitest";
import { PlanHeaderBar } from "../components/planner/PlanHeaderBar";
import type { Plan, PlanTimeSlot } from "../api/types";

// ─── Helpers ──────────────────────────────────────────────────────────────────

const now = new Date("2026-04-04T10:00:00Z");

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

function makePlan(overrides: Partial<Plan> = {}): Plan {
  return {
    id: "plan-001",
    created_at: new Date(now.getTime() - 45 * 1000).toISOString(), // 45s ago
    trigger: "Periodic",
    firm_boundary: "2026-04-04T13:00:00Z",
    firm_slots: [makeSlot()],
    flexible_slots: [],
    firm_summary: {
      total_cost_eur: 1.84,
      total_co2_g: 2100,
      total_import_kwh: 8.2,
      total_export_kwh: 0,
    },
    warnings: [],
    steps: [],
    ...overrides,
  };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("PlanHeaderBar", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders empty state when plan is null", () => {
    render(<PlanHeaderBar plan={null} />);
    expect(screen.getByTestId("plan-no-plan")).toBeInTheDocument();
    expect(screen.queryByTestId("plan-header")).toBeNull();
  });

  it("renders empty state when plan is undefined", () => {
    render(<PlanHeaderBar plan={undefined} />);
    expect(screen.getByTestId("plan-no-plan")).toBeInTheDocument();
  });

  it("renders plan-header when plan exists", () => {
    render(<PlanHeaderBar plan={makePlan()} />);
    expect(screen.getByTestId("plan-header")).toBeInTheDocument();
    expect(screen.queryByTestId("plan-no-plan")).toBeNull();
  });

  it("renders trigger badge with plan trigger", () => {
    render(<PlanHeaderBar plan={makePlan({ trigger: "RateChange" })} />);
    const badge = screen.getByTestId("plan-trigger-badge");
    expect(badge).toBeInTheDocument();
    expect(badge.textContent).toContain("RateChange");
  });

  it("Periodic trigger badge has default color class", () => {
    render(<PlanHeaderBar plan={makePlan({ trigger: "Periodic" })} />);
    const badge = screen.getByTestId("plan-trigger-badge");
    // MUI Chip with color="default" — check data-color or class
    const dataColor = badge.getAttribute("data-color") ?? badge.className;
    expect(dataColor).toMatch(/default|Periodic/i);
  });

  it("renders age in plan-age element", () => {
    vi.spyOn(Date, "now").mockReturnValue(now.getTime());
    render(<PlanHeaderBar plan={makePlan()} />);
    const age = screen.getByTestId("plan-age");
    // created_at is 45s ago → should show "45s ago"
    expect(age.textContent).toMatch(/\d+s ago/);
  });

  it("renders cost in plan-cost element", () => {
    render(<PlanHeaderBar plan={makePlan()} />);
    const cost = screen.getByTestId("plan-cost");
    expect(cost.textContent).toMatch(/1\.84/);
  });

  it("renders import kWh in plan-import-kwh element", () => {
    render(<PlanHeaderBar plan={makePlan()} />);
    const kwh = screen.getByTestId("plan-import-kwh");
    expect(kwh.textContent).toMatch(/8\.2/);
  });

  it("renders CO2 in plan-co2 element", () => {
    render(<PlanHeaderBar plan={makePlan()} />);
    const co2 = screen.getByTestId("plan-co2");
    // 2100 g = 2.1 kg
    expect(co2.textContent).toMatch(/2\.1/);
  });

  it("does not render warnings badge when no warnings", () => {
    render(<PlanHeaderBar plan={makePlan({ warnings: [] })} />);
    expect(screen.queryByTestId("plan-warnings-badge")).toBeNull();
  });

  it("renders warnings badge showing count when warnings exist", () => {
    const plan = makePlan({
      warnings: [
        { severity: "WARNING", message: "Packet infeasible", packet_id: null, suggested_action: null },
        { severity: "CRITICAL", message: "Grid limit exceeded", packet_id: "pkt-001", suggested_action: "Reduce load" },
      ],
    });
    render(<PlanHeaderBar plan={plan} />);
    const badge = screen.getByTestId("plan-warnings-badge");
    expect(badge.textContent).toMatch(/2/);
  });

  it("clicking warnings expand shows warning list", async () => {
    const user = userEvent.setup({});
    const plan = makePlan({
      warnings: [
        { severity: "WARNING", message: "Something wrong", packet_id: null, suggested_action: null },
      ],
    });
    render(<PlanHeaderBar plan={plan} />);
    expect(screen.queryByTestId("plan-warning-0")).toBeNull();
    await user.click(screen.getByTestId("plan-warnings-expand"));
    expect(screen.getByTestId("plan-warning-0")).toBeInTheDocument();
    expect(screen.getByTestId("plan-warning-0").textContent).toContain("Something wrong");
  });

  it("clicking expand again collapses the warning list", async () => {
    const user = userEvent.setup({});
    const plan = makePlan({
      warnings: [
        { severity: "WARNING", message: "A warning", packet_id: null, suggested_action: null },
      ],
    });
    render(<PlanHeaderBar plan={plan} />);
    await user.click(screen.getByTestId("plan-warnings-expand"));
    expect(screen.getByTestId("plan-warning-0")).toBeInTheDocument();
    await user.click(screen.getByTestId("plan-warnings-expand"));
    await waitFor(() => expect(screen.queryByTestId("plan-warning-0")).toBeNull());
  });

  it("warning with suggested_action shows action text", async () => {
    const user = userEvent.setup({});
    const plan = makePlan({
      warnings: [
        { severity: "CRITICAL", message: "Overload", packet_id: null, suggested_action: "Curtail load immediately" },
      ],
    });
    render(<PlanHeaderBar plan={plan} />);
    await user.click(screen.getByTestId("plan-warnings-expand"));
    const warning = screen.getByTestId("plan-warning-0");
    expect(warning.textContent).toContain("Curtail load immediately");
  });
});
