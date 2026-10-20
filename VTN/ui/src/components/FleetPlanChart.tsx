import { useMemo, useState } from "react";
import { Stack, ToggleButton, ToggleButtonGroup } from "@mui/material";
import { TimeSeriesChart } from "@lab/ui/TimeSeriesChart";
import type { TimeSeriesSeriesSpec } from "@lab/ui/TimeSeriesChart";
import { EmptyState } from "@lab/ui/EmptyState";
import { minSpanDomain } from "@lab/ui/axisDomain";
import { CELL_CHART_HEIGHT } from "@lab/ui/chartLayout";
import type { FleetPlan } from "../api/types";
import {
  type PlanQuantity,
  PLANNED_EXPORT_KEY,
  PLANNED_IMPORT_KEY,
  REFERENCE_EXPORT_KEY,
  REFERENCE_IMPORT_KEY,
  planRows,
} from "../utils/fleetPlan";
import { fleetChartTicks, FLEET_AXIS_WIDTH, FLEET_TICK_INTERVAL } from "./fleetChartWindow";

const AXIS_ID = "plan";
const PLANNED_IMPORT_COLOR = "#1976d2";
const PLANNED_EXPORT_COLOR = "#ef6c00";

/** The result series, then (when a reference is drawn) the same two, faint, for the plan before. */
const SERIES = [
  { key: PLANNED_IMPORT_KEY, color: PLANNED_IMPORT_COLOR, reference: false },
  { key: PLANNED_EXPORT_KEY, color: PLANNED_EXPORT_COLOR, reference: false },
  { key: REFERENCE_IMPORT_KEY, color: PLANNED_IMPORT_COLOR, reference: true },
  { key: REFERENCE_EXPORT_KEY, color: PLANNED_EXPORT_COLOR, reference: true },
];

const endOf = (plan: FleetPlan) =>
  Date.parse(plan.points[plan.points.length - 1].ts) + plan.stepSeconds * 1000;

type Span = { tMin: number; tMax: number };

/** How the chart reads a quantity: declared here, looked up by the one chart below. */
const QUANTITIES: Record<
  PlanQuantity,
  {
    label: string;
    unit: string;
    /** A planned kW holds for its slot; accumulated energy rises through it. */
    type: "stepAfter" | "linear";
    format: (v: number) => string;
    /** The time the chart spans: every plan's own extent, or the (clipped) rows. */
    span: (rows: { ts: number }[], plans: FleetPlan[], nowMs: number) => Span;
  }
> = {
  power: {
    label: "Power",
    unit: " kW",
    type: "stepAfter",
    format: (v) => `${v.toFixed(2)} kW`,
    span: (_rows, plans, nowMs) => ({
      tMin: plans.length ? Math.min(...plans.map((p) => Date.parse(p.points[0].ts))) : nowMs,
      tMax: plans.length ? Math.max(...plans.map(endOf)) : nowMs,
    }),
  },
  energy: {
    label: "Accumulated energy",
    unit: " kWh",
    type: "linear",
    format: (v) => `${v.toFixed(2)} kWh`,
    span: (rows) => ({ tMin: rows[0].ts, tMax: rows[rows.length - 1].ts }),
  },
};

/**
 * What the fleet intends to import and export over the plan horizon, summed across VENs.
 *
 * Drawn from the plans the VENs adopted (`GET /api/fleet/plan`), not from what the sites did: it is
 * how a change of price shows up minutes after it is published, before the hours it concerns have
 * happened. Import and export are separate series because that is how the plan states them; a
 * single signed line would hide a site importing while another exports. With a `reference` (the plan
 * as it stood before a tariff was published) it is drawn faint behind the current one.
 *
 * Opens on the plan as accumulated energy: each VEN's plan shows its effect the moment that VEN has
 * replanned, and the running totals separate long before the hours they concern have happened.
 * Power is one click away.
 */
export function FleetPlanChart({
  plan,
  reference,
  nowMs,
  tickMinutes = 180,
}: {
  plan: FleetPlan;
  reference?: FleetPlan;
  nowMs: number;
  /** Tick spacing; the plan runs ahead for a day, so hours, not minutes. */
  tickMinutes?: number;
}) {
  const [quantity, setQuantity] = useState<PlanQuantity>("energy");
  const { rows, tMin, tMax, values } = useMemo(() => {
    const rows = planRows(plan, reference?.points.length ? reference : undefined, quantity);
    const all = [plan, reference].filter((p): p is FleetPlan => !!p && p.points.length > 0);
    return {
      rows,
      ...(rows.length ? QUANTITIES[quantity].span(rows, all, nowMs) : { tMin: nowMs, tMax: nowMs }),
      values: rows.flatMap((r) => Object.values(r.values ?? {})),
    };
  }, [plan, reference, nowMs, quantity]);
  const declared = QUANTITIES[quantity];

  if (rows.length === 0) {
    return (
      <EmptyState
        message="No VEN has published a plan yet."
        testId="fleet-plan-empty"
        height={CELL_CHART_HEIGHT}
      />
    );
  }

  const drawn = SERIES.filter((s) => !s.reference || rows.some((r) => r.values?.[s.key] != null));
  const series: TimeSeriesSeriesSpec[] = drawn.map(({ key, color, reference: isReference }) => ({
    key,
    axisId: AXIS_ID,
    dataKey: (row) => row.values?.[key] ?? null,
    color,
    type: declared.type,
    // Within a plan's horizon a missing point is a different grid, not a gap; past the horizon
    // there are no later points to bridge to.
    connectNulls: true,
    ...(isReference ? { strokeOpacity: 0.45, strokeDasharray: "5 5" } : {}),
    formatter: declared.format,
  }));

  return (
    <Stack spacing={1}>
      <ToggleButtonGroup
        exclusive
        size="small"
        value={quantity}
        onChange={(_, next: PlanQuantity | null) => next && setQuantity(next)}
        aria-label="Plan quantity"
        data-testid="fleet-plan-quantity"
      >
        {(Object.keys(QUANTITIES) as PlanQuantity[]).map((q) => (
          <ToggleButton key={q} value={q}>
            {QUANTITIES[q].label}
          </ToggleButton>
        ))}
      </ToggleButtonGroup>
      <TimeSeriesChart
        data={rows}
        tMin={tMin}
        tMax={tMax}
        axes={[
          {
            id: AXIS_ID,
            unit: declared.unit,
            width: FLEET_AXIS_WIDTH,
            domain: minSpanDomain(values, 1),
          },
        ]}
        series={series}
        nowMs={nowMs}
        referenceAxisId={AXIS_ID}
        xAxisTicks={fleetChartTicks(tMin, tMax, tickMinutes)}
        xAxisInterval={FLEET_TICK_INTERVAL}
        height={CELL_CHART_HEIGHT}
        xAxisTickFormatter={(ts: number) => new Date(ts).toLocaleTimeString()}
        interactiveLegend
        testId="fleet-plan-chart"
      />
    </Stack>
  );
}
