/**
 * The fleet plan as chart rows: what `GET /api/fleet/plan` answers, shaped for one time-series
 * chart. Import and export sit in the same row, as the tariff's two series do, because the chart's
 * tooltip resolves by array index. A second plan (the one before a tariff was published) rides in
 * the same rows under its own keys, for the same reason.
 *
 * Nothing is carried forward between grid points: beyond a plan's horizon the VENs plan nothing,
 * and a held value there would claim a plan that does not exist.
 */
import { mergeTimestampedSeries } from "@lab/ui/mergeSeries";
import type { NamedSample, TimestampedRow } from "@lab/ui/mergeSeries";
import type { FleetPlan, FleetPlanPoint } from "../api/types";
import { cumulativeEnergyKwh, overlapWindow, pointsWithin } from "./planDelta";

/** Series keys, which are also the legend labels. */
export const PLANNED_IMPORT_KEY = "Planned import";
export const PLANNED_EXPORT_KEY = "Planned export";
export const REFERENCE_IMPORT_KEY = "Planned import (before)";
export const REFERENCE_EXPORT_KEY = "Planned export (before)";

export type PlanQuantity = "power" | "energy";

/**
 * One plan's samples under its two series keys. Power is each slot's kW at the slot's start. Energy is
 * the running kWh: zero at `points[0]`, then the total at the end of each slot, so the curve rises
 * through the slots instead of stepping at their starts.
 */
const samplesOf = (
  points: FleetPlanPoint[],
  stepSeconds: number,
  importKey: string,
  exportKey: string,
  quantity: PlanQuantity,
): NamedSample[] => {
  if (quantity === "power") {
    return points.flatMap((p) => {
      const ts = Date.parse(p.ts);
      return [
        { ts, key: importKey, value: p.netImportKw },
        { ts, key: exportKey, value: p.netExportKw },
      ];
    });
  }
  if (points.length === 0) return [];
  const importKwh = cumulativeEnergyKwh(points, stepSeconds, "netImportKw");
  const exportKwh = cumulativeEnergyKwh(points, stepSeconds, "netExportKw");
  const start = Date.parse(points[0].ts);
  return [
    { ts: start, key: importKey, value: 0 },
    { ts: start, key: exportKey, value: 0 },
    ...points.flatMap((p, i) => {
      const ts = Date.parse(p.ts) + stepSeconds * 1000;
      return [
        { ts, key: importKey, value: importKwh[i] },
        { ts, key: exportKey, value: exportKwh[i] },
      ];
    }),
  ];
};

/**
 * The instant to ask the fleet plan "as of" for the plan that stood before something happened at
 * `iso` (a tariff published, an objective changed): one second earlier, so a plan adopted in
 * answer to it is not counted.
 */
export function instantBefore(iso: string): string {
  return new Date(Date.parse(iso) - 1000).toISOString();
}

/**
 * The chart rows for a plan and, optionally, the plan from before a change. Accumulated energy is
 * compared over the window both plans cover, so both curves start level at zero and the gap between
 * them is what the change added, not the longer horizon of the newer plan.
 */
export function planRows(
  plan: FleetPlan,
  reference?: FleetPlan,
  quantity: PlanQuantity = "power",
): TimestampedRow[] {
  const window = quantity === "energy" && reference ? overlapWindow(reference, plan) : null;
  const pointsOf = (p: FleetPlan) => (window ? pointsWithin(p, window) : p.points);
  return mergeTimestampedSeries(
    [],
    [
      ...samplesOf(pointsOf(plan), plan.stepSeconds, PLANNED_IMPORT_KEY, PLANNED_EXPORT_KEY, quantity),
      ...(reference
        ? samplesOf(
            pointsOf(reference),
            reference.stepSeconds,
            REFERENCE_IMPORT_KEY,
            REFERENCE_EXPORT_KEY,
            quantity,
          )
        : []),
    ],
  );
}
