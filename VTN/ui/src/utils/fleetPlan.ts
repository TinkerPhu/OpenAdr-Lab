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
import type { FleetPlan } from "../api/types";

/** Series keys, which are also the legend labels. */
export const PLANNED_IMPORT_KEY = "Planned import";
export const PLANNED_EXPORT_KEY = "Planned export";
export const REFERENCE_IMPORT_KEY = "Planned import (before)";
export const REFERENCE_EXPORT_KEY = "Planned export (before)";

const samplesOf = (plan: FleetPlan, importKey: string, exportKey: string): NamedSample[] =>
  plan.points.flatMap((p) => {
    const ts = Date.parse(p.ts);
    return [
      { ts, key: importKey, value: p.netImportKw },
      { ts, key: exportKey, value: p.netExportKw },
    ];
  });

/**
 * The instant to ask the fleet plan "as of" for the plan that stood before something happened at
 * `iso` (a tariff published, an objective changed): one second earlier, so a plan adopted in
 * answer to it is not counted.
 */
export function instantBefore(iso: string): string {
  return new Date(Date.parse(iso) - 1000).toISOString();
}

export function planRows(plan: FleetPlan, reference?: FleetPlan): TimestampedRow[] {
  return mergeTimestampedSeries(
    [],
    [
      ...samplesOf(plan, PLANNED_IMPORT_KEY, PLANNED_EXPORT_KEY),
      ...(reference ? samplesOf(reference, REFERENCE_IMPORT_KEY, REFERENCE_EXPORT_KEY) : []),
    ],
  );
}
