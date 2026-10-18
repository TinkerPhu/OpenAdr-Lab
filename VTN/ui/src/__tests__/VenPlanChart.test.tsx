import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NET_KEY, VenPlanChart } from "../components/VenPlanChart";
import type { VenPlan } from "../api/types";

const chartProps: Record<string, unknown>[] = [];
vi.mock("@lab/ui/TimeSeriesChart", () => ({
  TimeSeriesChart: (props: Record<string, unknown>) => {
    chartProps.push(props);
    return null;
  },
}));

type Row = { ts: number; values: Record<string, number | null> | null };

const plan: VenPlan = {
  objective: "opportunistic",
  solve_status: "SIMULATED",
  slots: [
    {
      start: "2026-10-17T12:00:00Z",
      end: "2026-10-17T12:15:00Z",
      net_import_kw: 0,
      net_export_kw: 1.5,
      planned_kw_by_asset: { battery: 2, ev: 0.5 },
    },
  ],
};

describe("VenPlanChart", () => {
  it("draws the plan's net site power and each asset's planned power", () => {
    chartProps.length = 0;
    render(<VenPlanChart plan={plan} nowMs={Date.parse("2026-10-17T12:00:00Z")} />);
    const props = chartProps[0];
    expect((props.series as { key: string }[]).map((s) => s.key)).toEqual([NET_KEY, "battery", "ev"]);
    const row = (props.data as Row[])[0];
    expect(row.values![NET_KEY]).toBe(-1.5);
    expect(row.values!.battery).toBe(2);
    expect(screen.getByTestId("ven-plan-status")).toHaveTextContent("opportunistic, simulated");
  });

  it("says when the adopted plan has no slots", () => {
    render(<VenPlanChart plan={{ ...plan, slots: [] }} nowMs={0} />);
    expect(screen.getByTestId("ven-plan-empty")).toBeInTheDocument();
  });
});
