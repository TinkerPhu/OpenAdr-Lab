import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";
import { SessionProgressBoard } from "../components/sessions/SessionProgressBoard";
import type { Plan, PlanTimeSlot, SimSnapshot, UserRequestWithSession } from "../api/types";

// ─── Fixtures ─────────────────────────────────────────────────────────────────

const HOUR_MS = 3_600_000;

function makeEvRequest(overrides: Partial<UserRequestWithSession> = {}): UserRequestWithSession {
  const departure = new Date(Date.now() + 2 * HOUR_MS).toISOString();
  return {
    id: "req-ev-01",
    asset_id: "ev",
    target_energy_kwh: 10,
    target_soc: 0.9,
    desired_power_kw: 7.4,
    completion_policy: "STOP",
    deadlines: [{ latest_end: departure, max_total_cost_eur: null, max_marginal_rate_eur_kwh: null, min_completion: 1.0 }],
    mode: "BY_DEADLINE",
    max_total_cost_eur: null,
    tier_count: 1,
    session_id: "sess-ev-01",
    session_type: "ev",
    status: "ACTIVE",
    estimated_cost_eur: 1.8,
    estimated_co2_g: 300,
    interruptible: true,
    tolerance_min: null,
    budget_eur: null,
    created_at: "2026-04-04T08:00:00Z",
    updated_at: "2026-04-04T10:00:00Z",
    session: { type: "ev", id: "sess-ev-01", target_soc: 0.9, departure_time: departure, soft_deadline: false, mode: "BY_DEADLINE", budget_eur: null, created_at: "2026-04-04T08:00:00Z", updated_at: "2026-04-04T10:00:00Z" },
    ...overrides,
  };
}

function makeHeaterRequest(overrides: Partial<UserRequestWithSession> = {}): UserRequestWithSession {
  const readyBy = new Date(Date.now() + HOUR_MS).toISOString();
  return {
    ...makeEvRequest(),
    id: "req-htr-01",
    asset_id: "heater",
    target_soc: null,
    session_id: "sess-htr-01",
    session_type: "heater",
    session: { type: "heater", id: "sess-htr-01", target_temp_c: 21, ready_by: readyBy, mode: "BY_DEADLINE", created_at: "2026-04-04T08:00:00Z", updated_at: "2026-04-04T10:00:00Z" },
    deadlines: [{ latest_end: readyBy, max_total_cost_eur: null, max_marginal_rate_eur_kwh: null, min_completion: 1.0 }],
    ...overrides,
  };
}

function makeShiftableRequest(overrides: Partial<UserRequestWithSession> = {}): UserRequestWithSession {
  const latestEnd = new Date(Date.now() + 3 * HOUR_MS).toISOString();
  const earliestStart = new Date(Date.now() - HOUR_MS).toISOString();
  return {
    ...makeEvRequest(),
    id: "req-wm-01",
    asset_id: "wm",
    target_soc: null,
    target_energy_kwh: 2,
    session_id: "sess-wm-01",
    session_type: "shiftable_load",
    session: { type: "shiftable_load", id: "sess-wm-01", asset_id: "wm", power_kw: 2, duration_min: 60, earliest_start: earliestStart, latest_end: latestEnd, mode: "BY_DEADLINE", created_at: "2026-04-04T08:00:00Z", updated_at: "2026-04-04T10:00:00Z" },
    deadlines: [{ latest_end: latestEnd, max_total_cost_eur: null, max_marginal_rate_eur_kwh: null, min_completion: 1.0 }],
    ...overrides,
  };
}

function makeSim(assets: SimSnapshot["assets"]): SimSnapshot {
  return {
    ts: new Date().toISOString(),
    grid: { net_power_w: 0, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
    assets,
  };
}

/** Plan with hourly slots starting at the next hour boundary, planned_kw_by_asset per slot. */
function makePlan(plannedKwByAsset: Record<string, number>, numSlots = 4, overrides: Partial<Plan> = {}): Plan {
  const slots: PlanTimeSlot[] = [];
  const base = Date.now() + 60_000; // first slot starts in the future
  for (let i = 0; i < numSlots; i++) {
    slots.push({
      slot_index: i,
      start: new Date(base + i * HOUR_MS).toISOString(),
      end: new Date(base + (i + 1) * HOUR_MS).toISOString(),
      import_tariff_eur_kwh: 0.3,
      export_tariff_eur_kwh: 0.08,
      co2_g_kwh: 200,
      import_cap_kw: 10,
      export_cap_kw: 10,
      allocations: [],
      net_import_kw: 0,
      net_export_kw: 0,
      pv_forecast_kw: 0,
      baseline_kw: 0.5,
      planned_kw_by_asset: plannedKwByAsset,
    });
  }
  return {
    id: "plan-001",
    created_at: new Date().toISOString(),
    trigger: "Periodic",
    slots,
    summary: { total_cost_eur: 1, total_co2_g: 500, total_import_kwh: 3, total_export_kwh: 0 },
    envelopes: [],
    warnings: [],
    objective_eur: 0,
    friction_eur: 0,
    solve_status: "OPTIMAL",
    ...overrides,
  };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("SessionProgressBoard", () => {
  it("renders empty state when there are no requests", () => {
    render(<SessionProgressBoard requests={[]} />);
    expect(screen.getByTestId("session-board-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("session-board")).not.toBeInTheDocument();
  });

  it("routes ACTIVE requests to the active group and terminal ones to done", () => {
    const active = makeEvRequest();
    const done = makeHeaterRequest({ id: "req-htr-done", status: "COMPLETED" });
    const cancelled = makeShiftableRequest({ id: "req-wm-cxl", status: "CANCELLED" });
    render(<SessionProgressBoard requests={[active, done, cancelled]} />);
    expect(screen.getByTestId("session-board")).toBeInTheDocument();
    expect(screen.getByTestId("session-group-active")).toHaveTextContent("req-ev-01".slice(-6));
    const doneGroup = screen.getByTestId("session-group-done");
    expect(doneGroup).toHaveTextContent("req-htr-done".slice(-6));
    expect(doneGroup).toHaveTextContent("req-wm-cxl".slice(-6));
  });

  it("shows an EV fill gauge from live soc vs target_soc", () => {
    const sim = makeSim({ ev: { power_kw: 7.4, soc: 0.45 } });
    render(<SessionProgressBoard requests={[makeEvRequest()]} sim={sim} />);
    const gauge = screen.getByTestId("session-fill-req-ev-01");
    // 0.45 / 0.9 = 50 % → warning band (40–80 %)
    expect(gauge).toHaveAttribute("data-color", "warning");
    expect(screen.getByText("50%")).toBeInTheDocument();
  });

  it("uses success color when fill is above 80 %", () => {
    const sim = makeSim({ ev: { power_kw: 7.4, soc: 0.81 } });
    render(<SessionProgressBoard requests={[makeEvRequest()]} sim={sim} />);
    expect(screen.getByTestId("session-fill-req-ev-01")).toHaveAttribute("data-color", "success");
  });

  it("hides the fill gauge when the sim snapshot lacks the asset", () => {
    const sim = makeSim({ battery: { power_kw: 0, soc: 0.5 } });
    render(<SessionProgressBoard requests={[makeEvRequest()]} sim={sim} />);
    expect(screen.queryByTestId("session-fill-req-ev-01")).not.toBeInTheDocument();
  });

  it("shows heater progress as current → target temperature, without a % gauge", () => {
    const sim = makeSim({ heater: { power_kw: 2, temp_c: 19.5 } });
    render(<SessionProgressBoard requests={[makeHeaterRequest()]} sim={sim} />);
    const temp = screen.getByTestId("session-temp-req-htr-01");
    expect(temp).toHaveTextContent("19.5");
    expect(temp).toHaveTextContent("21");
    expect(screen.queryByTestId("session-fill-req-htr-01")).not.toBeInTheDocument();
  });

  it("shows a T− countdown for a future deadline", () => {
    render(<SessionProgressBoard requests={[makeEvRequest()]} />);
    expect(screen.getByTestId("session-deadline-req-ev-01")).toHaveTextContent(/T−1h \d+m/);
  });

  it("shows OVERDUE when the deadline has passed", () => {
    const past = new Date(Date.now() - HOUR_MS).toISOString();
    const req = makeEvRequest();
    req.session = { ...req.session!, departure_time: past } as typeof req.session;
    render(<SessionProgressBoard requests={[req]} />);
    expect(screen.getByTestId("session-deadline-req-ev-01")).toHaveTextContent("OVERDUE");
  });

  it("shows an estimated budget line when a budget is set", () => {
    const req = makeEvRequest({ budget_eur: 2.5 });
    render(<SessionProgressBoard requests={[req]} />);
    const budget = screen.getByTestId("session-budget-req-ev-01");
    expect(budget).toHaveTextContent("€1.80 / €2.50");
    expect(budget).toHaveTextContent("est.");
  });

  it("omits the budget line when no budget or cost cap is set", () => {
    render(<SessionProgressBoard requests={[makeEvRequest()]} />);
    expect(screen.queryByTestId("session-budget-req-ev-01")).not.toBeInTheDocument();
  });

  it("marks an EV session on track when planned energy covers the envelope remainder", () => {
    const plan = makePlan({ ev: 7.4 }, 2, {
      envelopes: [{
        asset_id: "ev", energy_needed_kwh: 5, power_min_kw: 1.4, power_max_kw: 7.4,
        window_start: new Date().toISOString(), window_end: new Date(Date.now() + 2 * HOUR_MS).toISOString(),
        slots_available: 2, max_acceptable_rate: 0.35, min_acceptable_rate: 0.05,
        budget_remaining_eur: 1e9, estimated_cost_eur: 1.5, estimated_co2_g: 900,
      }],
    });
    render(<SessionProgressBoard requests={[makeEvRequest()]} plan={plan} />);
    expect(screen.getByTestId("session-ontrack-req-ev-01")).toHaveTextContent(/on track/i);
  });

  it("marks an EV session at risk when planned energy falls short of the remainder", () => {
    const plan = makePlan({ ev: 1.0 }, 2, {
      envelopes: [{
        asset_id: "ev", energy_needed_kwh: 5, power_min_kw: 1.4, power_max_kw: 7.4,
        window_start: new Date().toISOString(), window_end: new Date(Date.now() + 2 * HOUR_MS).toISOString(),
        slots_available: 2, max_acceptable_rate: 0.35, min_acceptable_rate: 0.05,
        budget_remaining_eur: 1e9, estimated_cost_eur: 1.5, estimated_co2_g: 900,
      }],
    });
    render(<SessionProgressBoard requests={[makeEvRequest()]} plan={plan} />);
    expect(screen.getByTestId("session-ontrack-req-ev-01")).toHaveTextContent(/at risk/i);
  });

  it("marks a shiftable load on track when the plan schedules its full runtime energy", () => {
    // target = 2 kW × 60 min = 2 kWh; plan gives 2 kW for slots within the window
    const plan = makePlan({ wm: 2.0 }, 2);
    render(<SessionProgressBoard requests={[makeShiftableRequest()]} plan={plan} />);
    expect(screen.getByTestId("session-ontrack-req-wm-01")).toHaveTextContent(/on track/i);
  });

  it("expands the deadline tier table", async () => {
    const user = userEvent.setup();
    render(<SessionProgressBoard requests={[makeEvRequest()]} />);
    expect(screen.queryByTestId("session-tiers-req-ev-01")).not.toBeInTheDocument();
    await user.click(screen.getByTestId("session-expand-req-ev-01"));
    expect(screen.getByTestId("session-tiers-req-ev-01")).toBeInTheDocument();
  });

  it("condensed variant renders one chip per active session only", () => {
    const sim = makeSim({ ev: { power_kw: 7.4, soc: 0.45 } });
    const done = makeHeaterRequest({ id: "req-htr-done", status: "COMPLETED" });
    render(
      <SessionProgressBoard requests={[makeEvRequest(), done]} sim={sim} variant="condensed" />,
    );
    const chip = screen.getByTestId("session-chip-req-ev-01");
    expect(chip).toBeInTheDocument();
    // Same countdown format as the full board: T−1h 59m, not T−119m
    expect(chip).toHaveTextContent(/T−1h \d+m/);
    expect(screen.queryByTestId("session-chip-req-htr-done")).not.toBeInTheDocument();
    expect(screen.queryByTestId("session-board")).not.toBeInTheDocument();
  });

  it("condensed variant renders empty state when nothing is active", () => {
    render(<SessionProgressBoard requests={[]} variant="condensed" />);
    expect(screen.getByTestId("session-board-empty")).toBeInTheDocument();
  });
});
