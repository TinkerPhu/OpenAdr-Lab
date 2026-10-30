/**
 * The lab's tariffs, folded out of per-VEN signal bands.
 *
 * `GET /api/fleet/signals` answers per VEN, because an OpenADR event may target a subset of the
 * fleet, and it has already ranked overlapping events the way the VEN does, so each VEN's price
 * bands are the ones it acts on. VENs with identical price bands are one tariff; the lab can hold
 * several at once (the 2026-10-29 fleet experiment sent five targeted events to five VEN groups).
 * There is no group object on the VTN: a group is just the VENs an identical tariff reached.
 */
import { mergeTimestampedSeries, clipRowsToWindow, locfFillKeys } from "@lab/ui/mergeSeries";
import type { NamedSample, TimestampedRow } from "@lab/ui/mergeSeries";
import type { FleetSignals, SignalBand } from "../api/types";
import { compareVenNames } from "./venOrder";

/** Series keys, which are also the legend labels. */
export const IMPORT_KEY = "Import tariff";
export const EXPORT_KEY = "Export tariff";
/** The signal workbench's unpublished draft, drawn beside the live tariff. */
export const DRAFT_IMPORT_KEY = "Import tariff (draft)";
export const DRAFT_EXPORT_KEY = "Export tariff (draft)";

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
 * including the id would report a re-published event as a different tariff.
 */
function fingerprint(bands: SignalBand[]): string {
  return bands.map((b) => `${b.from}|${b.to}|${b.payloadType}|${b.value}`).join(";");
}

/** One tariff and the VENs it reached. */
export interface TariffGroup {
  /** Identifies the tariff by its bands; stable while the bands are. */
  key: string;
  /** The VENs on this tariff, in counting order. */
  vens: string[];
  /** The price bands (PRICE and EXPORT_PRICE), oldest first. */
  bands: SignalBand[];
  /** `<short event name> · <n> VENs`, unique among the groups drawn together. */
  label: string;
  /** What a hover shows: the event name and the VEN list. */
  hover: string;
}

export interface TariffGroups {
  /** Largest group first, ties by the first VEN's counting order. */
  groups: TariffGroup[];
  knownVens: number;
  /** VENs with no price band: not targeted by any price, which is not a tariff of its own. */
  unpricedVens: string[];
}

/**
 * Event names with the leading part they all share left out, cut at a `-` or `_`
 * boundary: `exp-flat-PRICE-g0`..`g4` read `g0`..`g4`. Fewer than two names, or
 * names that are all the same, are kept whole: there is nothing to tell apart.
 */
export function shortNames(names: (string | null)[]): (string | null)[] {
  const present = names.filter((n): n is string => !!n);
  if (present.length < 2 || present.every((n) => n === present[0])) return names;
  let prefix = present[0];
  for (const n of present) {
    while (!n.startsWith(prefix)) prefix = prefix.slice(0, -1);
  }
  const cut = Math.max(prefix.lastIndexOf("-"), prefix.lastIndexOf("_")) + 1;
  return names.map((n) => (n === null ? null : n.slice(cut) || n));
}

/** The PRICE band's event name in force at `atMs`, else the first PRICE band's: one name per group. */
function winningEventName(bands: SignalBand[], atMs: number): string | null {
  const prices = bands.filter((b) => b.payloadType === "PRICE");
  const inForce = prices.find((b) => Date.parse(b.from) <= atMs && atMs < Date.parse(b.to));
  return (inForce ?? prices[0])?.eventName ?? null;
}

const venCount = (n: number) => `${n} VEN${n === 1 ? "" : "s"}`;

export function tariffGroups(signals: FleetSignals | undefined, nowMs: number): TariffGroups {
  const vens = signals?.vens ?? [];
  const priced = vens
    .map((v) => ({ venName: v.venName, bands: priceBands(v.bands) }))
    .filter((v) => v.bands.length > 0);

  const byTariff = new Map<string, { venName: string; bands: SignalBand[] }[]>();
  for (const v of priced) {
    const key = fingerprint(v.bands);
    byTariff.set(key, [...(byTariff.get(key) ?? []), v]);
  }

  const raw = [...byTariff.entries()]
    .map(([key, members]) => ({
      key,
      vens: members.map((m) => m.venName).sort(compareVenNames),
      bands: members[0].bands,
    }))
    .sort((a, b) => b.vens.length - a.vens.length || compareVenNames(a.vens[0], b.vens[0]));

  const eventNames = raw.map((g) => winningEventName(g.bands, nowMs));
  const short = shortNames(eventNames);
  const seen = new Map<string, number>();
  const groups = raw.map((g, i): TariffGroup => {
    const base = `${short[i] ?? "tariff"} · ${venCount(g.vens.length)}`;
    const n = (seen.get(base) ?? 0) + 1;
    seen.set(base, n);
    return {
      ...g,
      label: n === 1 ? base : `${base} (${n})`,
      hover: `${eventNames[i] ?? "no event name"}: ${g.vens.join(", ")}`,
    };
  });

  const pricedNames = new Set(priced.map((v) => v.venName));
  return {
    groups,
    knownVens: vens.length,
    unpricedVens: vens
      .map((v) => v.venName)
      .filter((name) => !pricedNames.has(name))
      .sort(compareVenNames),
  };
}

/** One drawn line: a key and the bands of one payload type (PRICE or EXPORT_PRICE) it follows. */
export interface TariffLine {
  key: string;
  bands: SignalBand[];
}

/**
 * Lines to the single row array every series of one chart reads from.
 *
 * One merged array rather than one per line, because recharts resolves a hovered tooltip
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
export function tariffLineRows(lines: TariffLine[], tMin: number, tMax: number): TimestampedRow[] {
  const samples: NamedSample[] = lines.flatMap((line) =>
    line.bands.map((b) => ({ ts: Date.parse(b.from), key: line.key, value: b.value as number })),
  );
  if (samples.length === 0) return [];

  const merged = mergeTimestampedSeries([{ ts: tMax, values: {} }], samples);
  // Fill before clipping, so the row clipping pins to tMin carries every line
  // even when import and export were announced on different timestamps.
  const filled = locfFillKeys(
    merged,
    lines.map((line) => line.key),
  );
  return clipRowsToWindow(filled, tMin, tMax);
}

/** The workbench's rows: one live import/export pair and an unpublished draft beside it. */
export function tariffRows(
  bands: SignalBand[],
  tMin: number,
  tMax: number,
  draftBands: SignalBand[] = [],
): TimestampedRow[] {
  const of = (bs: SignalBand[], payloadType: string) => bs.filter((b) => b.payloadType === payloadType);
  return tariffLineRows(
    [
      { key: IMPORT_KEY, bands: of(bands, "PRICE") },
      { key: EXPORT_KEY, bands: of(bands, "EXPORT_PRICE") },
      { key: DRAFT_IMPORT_KEY, bands: of(draftBands, "PRICE") },
      { key: DRAFT_EXPORT_KEY, bands: of(draftBands, "EXPORT_PRICE") },
    ],
    tMin,
    tMax,
  );
}
