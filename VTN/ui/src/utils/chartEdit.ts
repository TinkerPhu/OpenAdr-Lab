/**
 * The arithmetic behind editing a series on the chart: what a pointer position means for the draft.
 *
 * Kept free of recharts and the DOM so it can be tested with numbers. The chart only turns a pixel
 * into a value or an instant (with the scale it already has); everything after that, snapping,
 * which pattern interval a drawn band is, moving a boundary, is here.
 */
import type { SignalDraft } from "./signalDraft";
import type { SignalRegistryEntry } from "./signalRegistry";

const MINUTE_MS = 60_000;

const decimalsOf = (step: number) => (String(step).split(".")[1] ?? "").length;

/** `raw` on the signal's step, inside its range. */
export function snapValue(raw: number, entry: Pick<SignalRegistryEntry, "step" | "min" | "max">): number {
  const clamped = Math.min(entry.max, Math.max(entry.min, raw));
  return Number((Math.round(clamped / entry.step) * entry.step).toFixed(decimalsOf(entry.step)));
}

/** `ms` on a grid of `gridMinutes`, counted from the UTC epoch (the grid the draft is written on). */
export function snapTimeMs(ms: number, gridMinutes: number): number {
  const grid = gridMinutes * MINUTE_MS;
  return Math.round(ms / grid) * grid;
}

const isRepeating = (d: SignalDraft) => d.recurrence.kind === "forever" || d.recurrence.kind === "span";

/**
 * Which row of the draft's pattern a band drawn at `bandStartMs` is.
 *
 * A repeating series is drawn many times from one pattern, so a drag on any repetition must edit
 * the single row it stands for. A series that does not repeat has only its own times; a band
 * outside them belongs to no row (null).
 */
export function patternIndexOfBand(draft: SignalDraft, bandStartMs: number): number | null {
  const first = draft.intervals[0];
  const last = draft.intervals[draft.intervals.length - 1];
  if (!first || !last) return null;
  const origin = Date.parse(first.start);
  const period = Date.parse(last.end) - origin;
  if (period <= 0) return null;

  const offset = isRepeating(draft)
    ? (((bandStartMs - origin) % period) + period) % period
    : bandStartMs - origin;
  if (offset < 0 || offset >= period) return null;

  const index = draft.intervals.findIndex(
    (r) => offset >= Date.parse(r.start) - origin && offset < Date.parse(r.end) - origin,
  );
  return index === -1 ? null : index;
}

/**
 * Moves the edge between row `after` and row `after + 1` to `newMs` (snapped to the grid), so
 * `end[after] == start[after + 1] == snapped`. Returns the draft unchanged when that would give a row
 * no length or turn it round, and for the first start and last end, which would change the period a
 * repeating series shifts by.
 */
export function moveBoundary(
  draft: SignalDraft,
  after: number,
  newMs: number,
  gridMinutes: number,
): SignalDraft {
  const left = draft.intervals[after];
  const right = draft.intervals[after + 1];
  if (!left || !right) return draft;

  const t = snapTimeMs(newMs, gridMinutes);
  if (t <= Date.parse(left.start) || t >= Date.parse(right.end)) return draft;

  const iso = new Date(t).toISOString();
  return {
    ...draft,
    intervals: draft.intervals.map((r, i) =>
      i === after ? { ...r, end: iso } : i === after + 1 ? { ...r, start: iso } : r,
    ),
  };
}
