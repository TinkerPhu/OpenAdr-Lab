import { useMemo } from "react";
import { Typography } from "@mui/material";
import { TimeSeriesChart } from "@lab/ui/TimeSeriesChart";
import type { TimeSeriesSeriesSpec } from "@lab/ui/TimeSeriesChart";
import { mergeTimestampedSeries } from "@lab/ui/mergeSeries";
import type { NamedSample } from "@lab/ui/mergeSeries";
import { formatPowerTick, tightSpanDomain } from "@lab/ui/axisDomain";
import { EmptyState } from "@lab/ui/EmptyState";
import { CELL_CHART_HEIGHT } from "@lab/ui/chartLayout";
import type { VenPlan } from "../api/types";
import { FLEET_SUM_COLOR, venColor } from "../utils/venColor";

/** The series key the plan's net site power is drawn under; an asset cannot be named this. */
export const NET_KEY = "__net__";
const AXIS_ID = "kw";

/**
 * One VEN's adopted plan, as it published it on its `plan` leaf (openspec
 * `opportunistic-objective` D12): the net site power it plans and each asset's planned power,
 * slot by slot. Under the opportunistic objective this is the forecast of what the site will do
 * without look-ahead; the status says which.
 */
export function VenPlanChart({ plan, nowMs }: { plan: VenPlan; nowMs: number }) {
  const { rows, series, values } = useMemo(() => {
    const samples: NamedSample[] = [];
    const assets = new Set<string>();
    for (const slot of plan.slots) {
      const ts = Date.parse(slot.start);
      samples.push({ ts, key: NET_KEY, value: slot.net_import_kw - slot.net_export_kw });
      for (const [asset, kw] of Object.entries(slot.planned_kw_by_asset)) {
        assets.add(asset);
        samples.push({ ts, key: asset, value: kw });
      }
    }
    const line = (key: string, label: string, color: string, width = 1.5): TimeSeriesSeriesSpec => ({
      key,
      label,
      axisId: AXIS_ID,
      dataKey: (row) => row.values?.[key] ?? null,
      color,
      strokeWidth: width,
      type: "stepAfter",
      connectNulls: false,
      formatter: (v: number) => `${v.toFixed(2)} kW`,
    });
    const series = [
      line(NET_KEY, "net site power", FLEET_SUM_COLOR, 2.5),
      ...[...assets].sort().map((a) => line(a, a, venColor(a))),
    ];
    const values = samples.map((s) => s.value);
    return { rows: mergeTimestampedSeries([], samples), series, values };
  }, [plan]);

  if (rows.length === 0) {
    return <EmptyState message="The adopted plan has no slots." testId="ven-plan-empty" />;
  }
  return (
    <>
      <Typography variant="caption" color="text.secondary" data-testid="ven-plan-status">
        {plan.objective}, {plan.solve_status.toLowerCase()}
      </Typography>
      <TimeSeriesChart
        data={rows}
        axes={[
          {
            id: AXIS_ID,
            autoScale: true,
            autoScaleMinSpan: 1,
            domain: tightSpanDomain([...values, 0], 1),
            tickFormatter: formatPowerTick,
          },
        ]}
        series={series}
        nowMs={nowMs}
        referenceAxisId={AXIS_ID}
        height={CELL_CHART_HEIGHT}
        xAxisTickFormatter={(ts: number) => new Date(ts).toLocaleTimeString()}
        interactiveLegend
        testId="ven-plan-chart"
      />
    </>
  );
}
