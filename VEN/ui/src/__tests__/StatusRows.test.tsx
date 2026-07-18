import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import {
  VtnConnectionRow,
  PlanStatusRow,
  TaskSummaryRow,
} from "../components/dashboard/StatusRows";
import type { Plan, TaskStatusEntry, VtnStatus } from "../api/types";

function makePlan(overrides: Partial<Plan> = {}): Plan {
  return {
    id: "p1",
    created_at: new Date(Date.now() - 45_000).toISOString(),
    trigger: "Periodic",
    slots: [],
    summary: { total_cost_eur: 0, total_co2_g: 0, total_import_kwh: 0, total_export_kwh: 0 },
    envelopes: [],
    warnings: [],
    objective_eur: 0,
    friction_eur: 0,
    solve_status: "OPTIMAL",
    ...overrides,
  };
}

describe("VtnConnectionRow", () => {
  it("shows a single healthy line when connected, no expand control", () => {
    const vtn: VtnStatus = {
      connected: true,
      last_success_ts: new Date().toISOString(),
      last_error: null,
      current_backoff_s: 0,
      token_expires_at: null,
    };
    render(<VtnConnectionRow vtnStatus={vtn} />);
    expect(screen.getByTestId("dash-vtn-status")).toHaveTextContent(/connected/i);
    expect(screen.queryByTestId("dash-vtn-expand")).not.toBeInTheDocument();
  });

  it("shows degraded state and expands with backoff/error detail when disconnected", () => {
    const vtn: VtnStatus = {
      connected: false,
      last_success_ts: null,
      last_error: "connection refused",
      current_backoff_s: 30.5,
      token_expires_at: null,
    };
    render(<VtnConnectionRow vtnStatus={vtn} />);
    expect(screen.getByTestId("dash-vtn-status")).toHaveTextContent(/disconnected/i);
    fireEvent.click(screen.getByTestId("dash-vtn-expand"));
    expect(screen.getByTestId("dash-vtn-detail")).toHaveTextContent(/30\.5/);
    expect(screen.getByTestId("dash-vtn-detail")).toHaveTextContent(/connection refused/);
  });

  it("shows an unknown state while /vtn/status hasn't loaded yet", () => {
    render(<VtnConnectionRow vtnStatus={undefined} />);
    expect(screen.getByTestId("dash-vtn-status")).toHaveTextContent(/unknown/i);
  });
});

describe("PlanStatusRow", () => {
  it("shows a neutral waiting line when no plan exists yet", () => {
    render(<PlanStatusRow plan={null} />);
    expect(screen.getByTestId("dash-plan-status")).toHaveTextContent(/waiting/i);
    expect(screen.queryByTestId("dash-plan-expand")).not.toBeInTheDocument();
  });

  it("shows a single healthy line with solve age when optimal", () => {
    render(<PlanStatusRow plan={makePlan({ solve_status: "OPTIMAL" })} />);
    expect(screen.getByTestId("dash-plan-status")).toHaveTextContent(/optimal/i);
    expect(screen.queryByTestId("dash-plan-expand")).not.toBeInTheDocument();
  });

  it("shows degraded state when infeasible", () => {
    render(<PlanStatusRow plan={makePlan({ solve_status: "INFEASIBLE" })} />);
    expect(screen.getByTestId("dash-plan-status")).toHaveTextContent(/infeasible/i);
  });
});

describe("TaskSummaryRow", () => {
  it("collapses to a single 'N/N running' line when all tasks are healthy", () => {
    const tasks: TaskStatusEntry[] = [
      { name: "sim_tick", last_run_ts: null, last_success: null, restart_count: 0 },
      { name: "poll_events", last_run_ts: null, last_success: true, restart_count: 0 },
    ];
    render(<TaskSummaryRow tasks={tasks} />);
    expect(screen.getByTestId("dash-tasks-status")).toHaveTextContent("2/2 running");
    expect(screen.queryByTestId("dash-tasks-expand")).not.toBeInTheDocument();
  });

  it("expands to list unhealthy tasks by name and restart count", () => {
    const tasks: TaskStatusEntry[] = [
      { name: "sim_tick", last_run_ts: null, last_success: null, restart_count: 0 },
      { name: "poll_events", last_run_ts: null, last_success: false, restart_count: 3 },
    ];
    render(<TaskSummaryRow tasks={tasks} />);
    expect(screen.getByTestId("dash-tasks-status")).toHaveTextContent("1/2 running");
    fireEvent.click(screen.getByTestId("dash-tasks-expand"));
    expect(screen.getByTestId("dash-tasks-detail")).toHaveTextContent("poll_events");
    expect(screen.getByTestId("dash-tasks-detail")).toHaveTextContent("3");
  });

  it("shows an empty/unknown state when no task status has been recorded", () => {
    render(<TaskSummaryRow tasks={[]} />);
    expect(screen.getByTestId("dash-tasks-status")).toHaveTextContent(/no task status/i);
  });
});
