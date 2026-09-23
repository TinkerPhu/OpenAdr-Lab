/**
 * The lab's tariff, folded out of per-VEN signal bands.
 *
 * `GET /api/fleet/signals` answers per VEN, because an OpenADR event may target
 * a subset of the fleet. Today every VEN gets the same two open price events, so
 * twenty identical band lists describe one lab tariff — but that is a property
 * of how the lab is seeded, not of the mechanism. Drawing one VEN's bands and
 * calling them "the tariff" would be right today and silently wrong the first
 * time somebody prices one site differently, so this folds them explicitly and
 * reports who disagrees.
 */
import { mergeTimestampedSeries, clipRowsToWindow, locfFillKeys } from "@lab/charts/mergeSeries";
import type { NamedSample, TimestampedRow } from "@lab/charts/mergeSeries";
import type { FleetSignals, SignalBand } from "../api/types";
import { compareVenNames } from "./venOrder";

/** Series keys, which are also the legend labels. */
export const IMPORT_KEY = "Import tariff";
export const EXPORT_KEY = "Export tariff";

const PRICE_TYPES = new Set(["PRICE", "EXPORT_PRICE"]);

/** The price bands out of a VEN's mixed signal list, oldest first. */
export function priceBands(bands: SignalBand[]): SignalBand[] {
  return bands
    .filter((b) => PRICE_TYPES.has(b.payloadType) && b.value !== null)
    .sort((a, b) => Date.parse(a.from) - Date.parse(b.from));
}

/**
 * What a VEN was told about price, as one comparable string.
 *
 * `eventID` is deliberately absent: two events announcing identical prices over
 * identical intervals are the same tariff as far as the fleet is concerned, and
 * including the id would report a re-published event as a disagreement.
 */
function fingerprint(bands: SignalBand[]): string {
  return bands.map((b) => `${b.from}|${b.to}|${b.payloadType}|${b.value}`).join(";");
}

export interface SharedTariff {
  /** The bands the largest agreeing group of VENs was sent. */
  bands: SignalBand[];
  agreeingVens: number;
  /** VENs sent something else, in counting order. Empty is the normal case. */
  dissentingVens: string[];
  knownVens: number;
}

export function sharedTariff(signals?: FleetSignals): SharedTariff {
  const vens = signals?.vens ?? [];
  // A VEN with no price band was not targeted; that is not a disagreement about
  // price, so it is counted but never named as a dissenter.
  const priced = vens
    .map((v) => ({ venName: v.venName, bands: priceBands(v.bands) }))
    .filter((v) => v.bands.length > 0);

  if (priced.length === 0) {
    return { bands: [], agreeingVens: 0, dissentingVens: [], knownVens: vens.length };
  }

  const groups = new Map<string, { venName: string; bands: SignalBand[] }[]>();
  for (const v of priced) {
    const key = fingerprint(v.bands);
    const group = groups.get(key);
    if (group) group.push(v);
    else groups.set(key, [v]);
  }

  const largest = [...groups.values()].sort((a, b) => b.length - a.length)[0];
  const agreeing = new Set(largest.map((v) => v.venName));

  return {
    bands: largest[0].bands,
    agreeingVens: largest.length,
    dissentingVens: priced
      .map((v) => v.venName)
      .filter((name) => !agreeing.has(name))
      .sort(compareVenNames),
    knownVens: vens.length,
  };
}

/**
 * Bands to the single row array both tariff series read from.
 *
 * One merged array rather than two, because recharts resolves a hovered tooltip
 * by array index — two independently indexed series would show the import price
 * under the export label.
 *
 * The two edges are what make this more than a map():
 *
 *   left  — `clipRowsToWindow` keeps the last row *before* `tMin` and pins it to
 *           `tMin`, so a stepAfter line starts at the price actually in force at
 *           the window's left edge rather than an hour into it.
 *   right — a bare row at `tMax` plus `locfFillKeys` carries the last announced
 *           price to the right edge, so the final band needs no special case
 *           whether it ends before or after `tMax`.
 *
 * Carrying forward is a claim — "the last announced price still holds" — and it
 * is an honest one for a time-of-use tariff, where a price stands until the next
 * one is published. Inventing a value at `tMax` would not be.
 */
export function tariffRows(
  bands: SignalBand[],
  tMin: number,
  tMax: number,
): TimestampedRow[] {
  if (bands.length === 0) return [];

  const samples: NamedSample[] = bands.map((b) => ({
    ts: Date.parse(b.from),
    key: b.payloadType === "PRICE" ? IMPORT_KEY : EXPORT_KEY,
    value: b.value as number,
  }));

  const merged = mergeTimestampedSeries([{ ts: tMax, values: {} }], samples);
  // Fill before clipping, so the row clipping pins to tMin carries both series
  // even when import and export were announced on different timestamps.
  const filled = locfFillKeys(merged, [IMPORT_KEY, EXPORT_KEY]);
  return clipRowsToWindow(filled, tMin, tMax);
}
