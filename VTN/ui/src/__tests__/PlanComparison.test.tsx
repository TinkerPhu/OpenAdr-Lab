import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PlanComparison } from "../components/PlanComparison";
import type { FleetPlan, FleetPlanPoint } from "../api/types";

const PUBLISHED = Date.parse("2026-10-18T10:00:00Z");
const STEP = 3600;

const point = (i: number, importKw: number, exportKw = 0): FleetPlanPoint => ({
  ts: new Date(PUBLISHED + i * STEP * 1000).toISOString(),
  netImportKw: importKw,
  netExportKw: exportKw,
  contributingVens: 2,
});

const plan = (points: FleetPlanPoint[], over: Partial<FleetPlan> = {}): FleetPlan => ({
  at: new Date(PUBLISHED).toISOString(),
  stepSeconds: STEP,
  points,
  plans: [],
  vensWithoutPlan: [],
  ...over,
});

const after = new Date(PUBLISHED + 60_000).toISOString();
const before = new Date(PUBLISHED - 3_600_000).toISOString();

const queries: Record<string, { data?: FleetPlan; isError?: boolean }> = {};
const askedFor: (string | undefined)[] = [];
vi.mock("../api/hooks", () => ({
  useFleetPlan: (at?: string) => {
    askedFor.push(at);
    return queries[at ? "reference" : "result"] ?? {};
  },
}));
vi.mock("../components/FleetPlanChart", () => ({
  FleetPlanChart: ({ plan, reference }: { plan: FleetPlan; reference?: FleetPlan }) => (
    <div data-testid="plan-chart-stub" data-ref={reference ? "yes" : "no"} data-points={plan.points.length} />
  ),
}));

function show(targets: string[] = []) {
  render(
    <PlanComparison
      publishedAt={new Date(PUBLISHED).toISOString()}
      targets={targets}
      nowMs={PUBLISHED + 600_000}
    />,
  );
}

describe("PlanComparison", () => {
  beforeEach(() => {
    askedFor.length = 0;
    for (const k of Object.keys(queries)) delete queries[k];
    queries.reference = {
      data: plan([point(0, 4), point(1, 8), point(2, 4)], {
        plans: [
          { venName: "ven-1", planId: "a", createdAt: before },
          { venName: "ven-2", planId: "b", createdAt: before },
        ],
      }),
    };
    queries.result = {
      data: plan([point(0, 4), point(1, 4, 2), point(2, 4)], {
        plans: [
          { venName: "ven-1", planId: "c", createdAt: after },
          { venName: "ven-2", planId: "d", createdAt: after },
        ],
      }),
    };
  });

  it("asks for the plan as of one second before the publish, and for now", () => {
    show();
    expect(askedFor).toContain(new Date(PUBLISHED - 1000).toISOString());
    expect(askedFor).toContain(undefined);
  });

  it("draws the result with the reference behind it", () => {
    show();
    expect(screen.getByTestId("plan-chart-stub")).toHaveAttribute("data-ref", "yes");
  });

  it("states the change in planned energy and peak, import and export apart", () => {
    show();
    expect(screen.getByTestId("plan-delta-import")).toHaveTextContent("16.0 → 12.0 kWh");
    expect(screen.getByTestId("plan-delta-import")).toHaveTextContent("−4.0 kWh");
    expect(screen.getByTestId("plan-delta-import")).toHaveTextContent("8.0 → 4.0 kW");
    expect(screen.getByTestId("plan-delta-export")).toHaveTextContent("+2.0 kWh");
  });

  it("is not provisional once every targeted VEN has replanned", () => {
    show();
    expect(screen.getByTestId("plan-replan-progress")).toHaveTextContent("2 of 2 VENs replanned");
    expect(screen.queryByTestId("plan-delta-provisional")).toBeNull();
  });

  it("marks the delta provisional while some VENs have not replanned", () => {
    queries.result.data = plan([point(0, 4), point(1, 4), point(2, 4)], {
      plans: [
        { venName: "ven-1", planId: "c", createdAt: after },
        { venName: "ven-2", planId: "b", createdAt: before },
      ],
    });
    show();
    expect(screen.getByTestId("plan-replan-progress")).toHaveTextContent("1 of 2 VENs replanned");
    expect(screen.getByTestId("plan-delta-provisional")).toHaveTextContent("not final");
  });

  it("names the missing part when there was no plan before the publish", () => {
    queries.reference = { data: plan([], { vensWithoutPlan: ["ven-1", "ven-2"] }) };
    show();
    expect(screen.getByTestId("plan-comparison-no-reference")).toHaveTextContent("ven-1");
    expect(screen.queryByTestId("plan-delta-import")).toBeNull();
  });

  it("says there is nothing to compare when the store is not connected", () => {
    queries.reference = { isError: true };
    show();
    expect(screen.getByTestId("plan-comparison-unavailable")).toHaveTextContent(
      "telemetry store is not connected",
    );
  });

  it("waits quietly while the plans load", () => {
    queries.reference = {};
    show();
    expect(screen.getByTestId("plan-comparison-loading")).toBeVisible();
  });
});
