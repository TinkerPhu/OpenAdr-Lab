/**
 * How a series repeats, as the OpenADR event declares it.
 *
 * The only wire control is `event.duration` (User Guide 647): longer than the
 * interval sequence repeats it to fill the window, "P9999Y" is "indefinitely",
 * shorter drops the surplus. This module maps that one field to a choice a person
 * can make and back, and does nothing else: *when* each repetition runs is
 * decided by `lab_core::event_timing::timed_intervals`, and the workbench asks
 * the BFF for that rather than re-implementing it here.
 */

/** The spec's "infinity" (User Guide 574, 582). */
export const FOREVER_DURATION = "P9999Y";

export type Recurrence =
  | { kind: "once" }
  | { kind: "forever" }
  /** Repeat until `duration` after the first interval's start. */
  | { kind: "span"; duration: string }
  /** `duration` ends before the sequence does: receivers drop the surplus. */
  | { kind: "truncated"; duration: string }
  /** Not an ISO 8601 duration; kept verbatim so it can be shown and fixed. */
  | { kind: "invalid"; duration: string };

const ISO_DURATION =
  /^P(?:(\d+)Y)?(?:(\d+)M)?(?:(\d+)W)?(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?)?$/;

/**
 * `start` plus an ISO 8601 duration, or null when `duration` is not one.
 *
 * Years and months are added to the calendar date, not converted to a fixed
 * number of days, because they only have a length relative to a start — which is
 * how the receiver resolves them (`to_chrono_at_datetime`). Days are 24 h:
 * everything here is UTC, and a local-time day is exactly what a fixed-period
 * repetition cannot express.
 */
export function addIsoDuration(start: Date, duration: string): Date | null {
  const m = ISO_DURATION.exec(duration);
  if (!m) return null;
  const [years, months, weeks, days, hours, minutes, seconds] = m
    .slice(1)
    .map((g) => (g === undefined ? 0 : Number(g)));
  // "P" and "PT" match the pattern but name no span.
  if (m.slice(1).every((g) => g === undefined)) return null;
  if (duration.endsWith("T")) return null;

  const end = new Date(start.getTime());
  end.setUTCFullYear(end.getUTCFullYear() + years, end.getUTCMonth() + months);
  const ms =
    (((weeks * 7 + days) * 24 + hours) * 60 + minutes) * 60_000 + seconds * 1000;
  const result = new Date(end.getTime() + ms);
  return Number.isNaN(result.getTime()) ? null : result;
}

/** A span of milliseconds as an ISO 8601 time duration ("PT1H", "PT90M" as "PT1H30M"). */
export function formatIsoDuration(ms: number): string {
  const totalSeconds = Math.round(ms / 1000);
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  return `PT${h ? `${h}H` : ""}${m ? `${m}M` : ""}${s || (!h && !m) ? `${s}S` : ""}`;
}

/** Is `duration` the spec's "infinity" in any spelling: "P9999Y", or the long form the VTN
 *  stores and returns ("P9999Y0M0DT0H0M0S")? */
function isForeverDuration(duration: string): boolean {
  const m = ISO_DURATION.exec(duration);
  if (!m) return false;
  const [years, ...rest] = m.slice(1).map((g) => (g === undefined ? 0 : Number(g)));
  return years === 9999 && rest.every((n) => n === 0);
}

/**
 * An event's `duration` as a recurrence, given where its interval sequence
 * starts and ends.
 *
 * The string is kept for `span`, `truncated` and `invalid` so that loading an
 * event and publishing it unedited writes back exactly what was read; `forever` is
 * written back as `FOREVER_DURATION`, which the VTN stores as the same value.
 */
export function recurrenceFromDuration(
  duration: string | null | undefined,
  patternStart: Date,
  patternEnd: Date,
): Recurrence {
  if (!duration) return { kind: "once" };
  if (isForeverDuration(duration)) return { kind: "forever" };
  const end = addIsoDuration(patternStart, duration);
  if (!end) return { kind: "invalid", duration };
  return end.getTime() < patternEnd.getTime()
    ? { kind: "truncated", duration }
    : { kind: "span", duration };
}

/** The `event.duration` a recurrence is written as; null means omit the field. */
export function durationFromRecurrence(r: Recurrence): string | null {
  switch (r.kind) {
    case "once":
      return null;
    case "forever":
      return FOREVER_DURATION;
    default:
      return r.duration;
  }
}

/** "24 h", "7 d", "1 h 30 min": the span a pattern repeats by, for a person to read. A single
 *  day stays "24 h" because that is the wording a daily tariff is described in. */
export function describePeriod(ms: number): string {
  const minutes = Math.round(ms / 60_000);
  if (minutes >= 2 * 1440 && minutes % 1440 === 0) return `${minutes / 1440} d`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return [h ? `${h} h` : "", m ? `${m} min` : ""].filter(Boolean).join(" ") || "0 min";
}

function utcOffsetLabel(ms: number, timeZone: string): string {
  return (
    new Intl.DateTimeFormat("en-US", { timeZone, timeZoneName: "longOffset" })
      .formatToParts(new Date(ms))
      .find((p) => p.type === "timeZoneName")?.value ?? ""
  );
}

/**
 * Does `timeZone` change its UTC offset between `fromMs` and `toMs`?
 *
 * A repeating series repeats by a fixed elapsed time, so across such a change a
 * pattern authored at local midnight lands an hour off local clock time. Sampled
 * every six hours, which cannot miss a clock change (they are months apart).
 */
export function crossesUtcOffsetChange(
  fromMs: number,
  toMs: number,
  timeZone: string = Intl.DateTimeFormat().resolvedOptions().timeZone,
): boolean {
  const first = utcOffsetLabel(fromMs, timeZone);
  for (let t = fromMs + 6 * 3_600_000; t < toMs; t += 6 * 3_600_000) {
    if (utcOffsetLabel(t, timeZone) !== first) return true;
  }
  return utcOffsetLabel(toMs, timeZone) !== first;
}
