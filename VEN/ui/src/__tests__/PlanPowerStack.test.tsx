/**
 * PlanPowerStack tests: regression coverage for the grid-power data-source fix
 * (unify-plan-power-stack-grid) — the chart must source gridPowerKw from the
 * backend-computed timeline (net_import_kw - net_export_kw), not from
 * net_import_kw alone.
 */
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, afterEach } from "vitest";
import { PlanPowerStack } from "../components/planner/PlanPowerStack";
import type { Plan, PlanTimeSlot } from "../api/types";
import type { AssetTimelinePoint } from "../components/controller/types";

// ─── Mock StackedTimeSeriesChart — capture the data/assetIds it receives ─────

let capturedProps: { data: unknown; assetIds: unknown } | null = null;

vi.mock("../components/charts/StackedTimeSeriesChart", () => ({
  StackedTimeSeriesChart: (props: { data: unknown; assetIds: unknown }) => {
    capturedProps = props;
    return <div data-testid="stacked-area-chart" />;
  },
}));

// ─── Mock useAllTimelines — a spy so tests can assert on the (hoursBack,
// hoursForward) query args, not just the returned data ──────────────────────

let allTimelinesData: { zones: unknown[]; timelines: Record<string, AssetTimelinePoint[]> } = {
  zones: [],
  timelines: {},
};

const useAllTimelinesSpy = vi.fn<(hoursBack?: number, hoursForward?: number) => { data: typeof allTimelinesData; refetch: () => void }>(
  () => ({ data: allTimelinesData, refetch: vi.fn() })
);

vi.mock("../api/hooks", () => ({
  useAllTimelines: (hoursBack?: number, hoursForward?: number) =>
    useAllTimelinesSpy(hoursBack, hoursForward),
}));

// ─── Helpers ──────────────────────────────────────────────────────────────────

function pt(ts: number, power_kw: number): AssetTimelinePoint {
  return { ts, values: { power_kw } };
}

function makeSlot(overrides: Partial<PlanTimeSlot> = {}): PlanTimeSlot {
  return {
    slot_index: 0,
    start: "2026-04-04T10:00:00Z",
    end: "2026-04-04T10:05:00Z",
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

function makePlan(slots: PlanTimeSlot[]): Plan {
  return {
    id: "plan-001",
    created_at: "2026-04-04T10:00:00Z",
    trigger: "Periodic",
    slots,
    summary: { total_cost_eur: 1.0, total_co2_g: 500, total_import_kwh: 3.0, total_export_kwh: 0 },
    envelopes: [],
    warnings: [],
    objective_eur: 0,
    friction_eur: 0,
    solve_status: "OPTIMAL",
  };
}

describe("PlanPowerStack", () => {
  afterEach(() => {
    allTimelinesData = { zones: [], timelines: {} };
    capturedProps = null;
    useAllTimelinesSpy.mockClear();
    vi.useRealTimers();
  });

  // ── Regression: the refetch-storm bug the timeline-source fix introduced ──
  it("does not recompute hoursForward (and thus the useAllTimelines query key) across re-renders when the plan is unchanged", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-04T10:00:00.000Z"));

    const plan = makePlan([makeSlot({ end: "2026-04-04T22:00:00Z" })]);
    const { rerender } = render(<PlanPowerStack plan={plan} />);

    expect(useAllTimelinesSpy).toHaveBeenCalledTimes(1);
    const [, firstHoursForward] = useAllTimelinesSpy.mock.calls[0];

    // Simulate real time passing (e.g. an SSE solving_progress event forcing a
    // re-render) without the plan itself changing.
    vi.setSystemTime(new Date("2026-04-04T10:00:05.000Z"));
    rerender(<PlanPowerStack plan={plan} />);

    expect(useAllTimelinesSpy).toHaveBeenCalledTimes(2);
    const [, secondHoursForward] = useAllTimelinesSpy.mock.calls[1];

    // Same plan object → the query's hoursForward argument must be identical,
    // not drift with Date.now() on every render (that drift is what turned
    // every render into a new React Query key and a new fetch).
    expect(secondHoursForward).toBe(firstHoursForward);
  });

  it("shows the no-plan empty state when plan is absent", () => {
    render(<PlanPowerStack plan={null} />);
    expect(screen.getByText(/No plan data available/)).toBeInTheDocument();
    expect(screen.queryByTestId("stacked-area-chart")).not.toBeInTheDocument();
  });

  it("shows the no-plan empty state when plan has no slots", () => {
    render(<PlanPowerStack plan={makePlan([])} />);
    expect(screen.getByText(/No plan data available/)).toBeInTheDocument();
  });

  it("renders the curtailment banner when the plan curtails PV", () => {
    const plan = makePlan([
      makeSlot({ pv_forecast_kw: 5.0, pv_used_kw: 3.0 }),
    ]);
    render(<PlanPowerStack plan={plan} />);
    expect(screen.getByTestId("pv-curtailment-indicator")).toHaveTextContent(/2.00 kW/);
  });

  // ── Regression: the exact shape of the bug this change fixes ──────────────
  it("shows a negative (export) grid line for a slot where net_export_kw is nonzero and net_import_kw is ~0", () => {
    // Autarky objective: import is fully avoided, surplus is exported.
    // net_import_kw alone (the old, buggy source) would read ~0 here.
    // The timeline's "grid" virtual asset already carries the correct signed
    // net value (net_import_kw - net_export_kw = -4.5), computed server-side.
    allTimelinesData = {
      zones: [],
      timelines: {
        grid: [pt(1000, -4.5)],
        pv: [pt(1000, -5.0)],
        base_load: [pt(1000, 0.5)],
      },
    };
    const plan = makePlan([
      makeSlot({ net_import_kw: 0.0, net_export_kw: 4.5, pv_forecast_kw: 5.0, pv_used_kw: 5.0 }),
    ]);

    render(<PlanPowerStack plan={plan} />);

    const data = capturedProps?.data as { gridPowerKw: number | null }[];
    expect(data).toHaveLength(1);
    expect(data[0].gridPowerKw).toBe(-4.5);
  });

  it("only renders asset series present in the timeline response", () => {
    allTimelinesData = {
      zones: [],
      timelines: {
        grid: [pt(1000, 1.0)],
        pv: [pt(1000, -2.0)],
        base_load: [pt(1000, 3.0)],
      },
    };
    const plan = makePlan([makeSlot()]);

    render(<PlanPowerStack plan={plan} />);

    expect(capturedProps?.assetIds).toEqual(["base_load", "pv"]);
  });
});
