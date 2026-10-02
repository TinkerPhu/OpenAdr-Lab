/**
 * The `[from, to)` ISO windows the history pages query over.
 *
 * Both functions existed twice, once in `pages/History.tsx` and once in
 * `pages/PlanHistory.tsx`, with identical names and identical duplicated
 * tests. `dayRangeIso` had not drifted; `last24hRangeIso` had — History's
 * copy takes the instant from the caller (the server clock) precisely so a
 * client with a skewed OS clock still queries a window containing data, while
 * PlanHistory's copy called `new Date()` and had that bug. One copy, and the
 * clock is always the caller's.
 */

export interface IsoRange {
  fromIso: string;
  toIso: string;
}

const DAY_MS = 24 * 3600 * 1000;

/** `[from, to)` ISO bounds for the UTC calendar day `dateStr` ("YYYY-MM-DD"). */
export function dayRangeIso(dateStr: string): IsoRange {
  const from = new Date(`${dateStr}T00:00:00.000Z`);
  const to = new Date(from.getTime() + DAY_MS);
  return { fromIso: from.toISOString(), toIso: to.toISOString() };
}

/**
 * `[from, to)` ISO bounds for the rolling 24 h window ending at `nowMs` — the
 * default view, so a history tab is useful the moment it opens instead of
 * showing a mostly-empty calendar day.
 *
 * `nowMs` is the caller's to supply, and should be the server clock rather
 * than the browser's (see `useFrozenServerNowMs`).
 */
export function last24hRangeIso(nowMs: number): IsoRange {
  const to = new Date(nowMs);
  return { fromIso: new Date(to.getTime() - DAY_MS).toISOString(), toIso: to.toISOString() };
}
