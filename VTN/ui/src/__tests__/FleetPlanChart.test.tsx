import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FleetPlanChart } from "../components/FleetPlanChart";
import {
  PLANNED_EXPORT_KEY,
  PLANNED_IMPORT_KEY,
  REFERENCE_EXPORT_KEY,
  REFERENCE_IMPORT_KEY,
  planRows,
} from "../utils/fleetPlan";
import type { FleetPlan } from "../api/types";

const chartProps: Record<string, unknown>[] = [];
vi.mock("@lab/ui/TimeSeriesChart", () => ({
  TimeSeriesChart: (props: Record<string, unknown>) => {
    chartProps.push(props);
    return null;
  },
}));

const T0 = Date.parse("2026-10-18T10:00:00Z");

function plan(over: Partial<FleetPlan> = {}): FleetPlan {
  return {
    at: new Date(T0).toISOString(),
    stepSeconds: 300,
    points: [
      { ts: new Date(T0).toISOString(), netImportKw: 4, netExportKw: 0, contributingVens: 2 },
      { ts: new Date(T0 + 300_000).toISOString(), netImportKw: 0, netExportKw: 1.5, contributingVens: 2 },
    ],
    plans: [],
    vensWithoutPlan: [],
    ...over,
  };
}

describe("planRows", () => {
  it("puts import and export in the same row, as one array", () => {
    const rows = planRows(plan());
    expect(rows).toHaveLength(2);
    expect(rows[0].values).toEqual({ [PLANNED_IMPORT_KEY]: 4, [PLANNED_EXPORT_KEY]: 0 });
    expect(rows[1].ts).toBe(T0 + 300_000);
  });
});

describe("planRows by quantity", () => {
  it("accumulated energy starts at zero and rises by each slot's kWh, at the slot's end", () => {
    // 4 kW import for 5 minutes, then 1.5 kW export for 5 minutes.
    const rows = planRows(plan(), undefined, "energy");
    expect(rows.map((r) => r.ts)).toEqual([T0, T0 + 300_000, T0 + 600_000]);
    expect(rows[0].values).toEqual({ [PLANNED_IMPORT_KEY]: 0, [PLANNED_EXPORT_KEY]: 0 });
    expect(rows[1].values?.[PLANNED_IMPORT_KEY]).toBeCloseTo(4 / 12);
    expect(rows[2].values?.[PLANNED_EXPORT_KEY]).toBeCloseTo(1.5 / 12);
    expect(rows[2].values?.[PLANNED_IMPORT_KEY]).toBeCloseTo(4 / 12);
  });

  it("starts both curves at zero at the start of the window both plans cover", () => {
    const earlier = plan({
      points: [
        { ts: new Date(T0 - 300_000).toISOString(), netImportKw: 12, netExportKw: 0, contributingVens: 2 },
        ...plan().points,
      ],
    });
    const rows = planRows(plan(), earlier, "energy");
    expect(rows[0].ts).toBe(T0);
    expect(rows[0].values).toEqual({
      [PLANNED_IMPORT_KEY]: 0,
      [PLANNED_EXPORT_KEY]: 0,
      [REFERENCE_IMPORT_KEY]: 0,
      [REFERENCE_EXPORT_KEY]: 0,
    });
  });
});

describe("FleetPlanChart", () => {
  beforeEach(() => {
    chartProps.length = 0;
  });

  it("draws planned import and planned export as separate step series in kW", () => {
    render(<FleetPlanChart plan={plan()} nowMs={T0} />);
    fireEvent.click(screen.getByRole("button", { name: "Power" }));
    const props = chartProps[chartProps.length - 1];
    const series = props.series as { key: string; type?: string }[];
    expect(series.map((s) => s.key)).toEqual([PLANNED_IMPORT_KEY, PLANNED_EXPORT_KEY]);
    expect(series.every((s) => s.type === "stepAfter")).toBe(true);
    expect((props.axes as { unit: string }[])[0].unit).toBe(" kW");
  });

  it("opens on the accumulated plan in kWh, because that is the view that shows a result first", () => {
    render(<FleetPlanChart plan={plan()} nowMs={T0} />);
    const series = chartProps[0].series as { key: string; type?: string }[];
    expect(series.map((s) => s.key)).toEqual([PLANNED_IMPORT_KEY, PLANNED_EXPORT_KEY]);
    expect(series.every((s) => s.type === "linear")).toBe(true);
    expect((chartProps[0].axes as { unit: string }[])[0].unit).toBe(" kWh");
    const data = chartProps[0].data as { values: Record<string, number> }[];
    const last = data[data.length - 1].values;
    expect(last[PLANNED_IMPORT_KEY]).toBeCloseTo(4 / 12);
  });

  it("spans the plan, to the end of its last slot", () => {
    render(<FleetPlanChart plan={plan()} nowMs={T0} />);
    expect(chartProps[0].tMin).toBe(T0);
    expect(chartProps[0].tMax).toBe(T0 + 300_000 + 300_000);
  });

  it("draws the plan from before a publish faint behind the current one", () => {
    const before = plan({
      points: [{ ts: new Date(T0).toISOString(), netImportKw: 9, netExportKw: 0, contributingVens: 2 }],
    });
    render(<FleetPlanChart plan={plan()} reference={before} nowMs={T0} />);
    fireEvent.click(screen.getByRole("button", { name: "Power" }));
    const props = chartProps[chartProps.length - 1];
    const series = props.series as {
      key: string;
      strokeOpacity?: number;
      strokeDasharray?: string;
    }[];
    expect(series.map((s) => s.key)).toEqual([
      PLANNED_IMPORT_KEY,
      PLANNED_EXPORT_KEY,
      REFERENCE_IMPORT_KEY,
      REFERENCE_EXPORT_KEY,
    ]);
    expect(series[0].strokeOpacity).toBeUndefined();
    expect(series[2].strokeOpacity).toBeLessThan(1);
    expect(series[2].strokeDasharray).toBeTruthy();
    // The reference's value is in the same row as the current plan's.
    const first = (props.data as { values: Record<string, number> }[])[0].values;
    expect(first[REFERENCE_IMPORT_KEY]).toBe(9);
    expect(first[PLANNED_IMPORT_KEY]).toBe(4);
  });

  it("does not add reference series when the reference has no plan", () => {
    render(<FleetPlanChart plan={plan()} reference={plan({ points: [] })} nowMs={T0} />);
    expect((chartProps[0].series as unknown[]).length).toBe(2);
  });

  it("spans both plans when they cover different times", () => {
    const before = plan({
      points: [
        { ts: new Date(T0 - 600_000).toISOString(), netImportKw: 1, netExportKw: 0, contributingVens: 1 },
      ],
    });
    render(<FleetPlanChart plan={plan()} reference={before} nowMs={T0} />);
    expect(chartProps[0].tMin).toBe(T0 - 600_000);
    expect(chartProps[0].tMax).toBe(T0 + 600_000);
  });

  it("says so, rather than draw an empty chart, when no VEN has a plan", () => {
    render(<FleetPlanChart plan={plan({ points: [] })} nowMs={T0} />);
    expect(screen.getByTestId("fleet-plan-empty")).toBeVisible();
    expect(chartProps).toHaveLength(0);
  });
});
