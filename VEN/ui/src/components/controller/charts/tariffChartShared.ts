import type { TariffTimePoint } from "../types";
import type { TimestampedRow } from "../../charts/mergeSeries";
import { clipRowsToWindow, ensureNonEmptyRows } from "../../charts/mergeSeries";

/** X-axis tick label — shared by every Grid Signals / Grid Rates chart. */
export function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function toRow(p: TariffTimePoint): TimestampedRow {
  return {
    ts: p.ts,
    values: {
      importPriceEurKwh: p.importPriceEurKwh,
      exportPriceEurKwh: p.exportPriceEurKwh,
      co2GKwh: p.co2GKwh,
      totalCostRateEurH: p.totalCostRateEurH,
      totalCo2RateGH: p.totalCo2RateGH,
      gridPowerKw: p.gridPowerKw,
      importLimitKw: p.importLimitKw,
      exportLimitKw: p.exportLimitKw,
    },
  };
}

/**
 * Clip merged tariff/rate/envelope data to [tMin, tMax]. recharts does not clip rendered
 * data to the XAxis domain — without this the chart auto-scales to the full data extent
 * (e.g. 6×24h from /tariffs or /capacity/schedule). Keeps the last point before tMin as a
 * left anchor so stepAfter lines start at the correct value at the left edge of the window.
 */
export function clipToWindow(
  data: TariffTimePoint[],
  tMin: number,
  tMax: number
): TimestampedRow[] {
  return clipRowsToWindow(data.map(toRow), tMin, tMax);
}

/**
 * Carry forward the last known value of `keys` (a step-interpolated series, e.g. tariff or
 * capacity-limit steps) to `tMax`. The merged dataset contains power/rate points with null
 * step-series fields after the last snapshot for that series — connectNulls=false stops the
 * stepAfter line at the last non-null value rather than extending to the right edge; a
 * sentinel row at tMax prevents this gap.
 */
export function carryForwardLastKnown(
  rows: TimestampedRow[],
  tMax: number,
  keys: string[]
): TimestampedRow[] {
  const last = [...rows].reverse().find((p) => keys.some((k) => p.values?.[k] != null));
  if (!last) return rows;
  return [
    ...rows,
    {
      ts: tMax,
      values: Object.fromEntries(keys.map((k) => [k, last.values?.[k] ?? null])),
    },
  ];
}

/** Ensure at least a 2-point range so recharts can render the NOW line when data is empty. */
export function ensureNonEmpty(
  rows: TimestampedRow[],
  tMin: number,
  tMax: number
): TimestampedRow[] {
  return ensureNonEmptyRows(rows, tMin, tMax);
}
