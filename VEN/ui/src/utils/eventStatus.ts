import type { VtnEvent } from "../api/types";

export type EventStatus = "scheduled" | "active" | "completed" | "immediate";

/**
 * Parse an ISO 8601 duration string into milliseconds.
 *
 * Both spellings the VTN can send: the compact `"PT1H"` and the fully expanded
 * `"P0Y0M0DT1H0M0S"`. Since the VEN adopted the OpenADR wire types it
 * re-serialises durations in the expanded form, and a parser that only matched
 * the compact one returned 0 — which reads as "no duration", so every event
 * rendered as if it never ended.
 *
 * `M` is deliberately read twice: before `T` it is months, after `T` it is
 * minutes. Conflating them is the classic ISO-8601 duration bug.
 *
 * Years and months have no fixed length, so they are approximated (365d, 30d)
 * — enough for "is this event running now", which is all this function feeds.
 * The spec's `"P9999Y"` infinity lands far enough in the future either way.
 */
function parseDuration(dur: string): number {
  const match = dur.match(
    /^P(?:(\d+)Y)?(?:(\d+)M)?(?:(\d+)W)?(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?)?$/,
  );
  if (!match) return 0;
  const years = parseInt(match[1] || "0", 10);
  const months = parseInt(match[2] || "0", 10);
  const weeks = parseInt(match[3] || "0", 10);
  const days = parseInt(match[4] || "0", 10);
  const hours = parseInt(match[5] || "0", 10);
  const minutes = parseInt(match[6] || "0", 10);
  const seconds = parseFloat(match[7] || "0");
  const totalDays = years * 365 + months * 30 + weeks * 7 + days;
  return ((totalDays * 24 + hours) * 60 + minutes) * 60000 + seconds * 1000;
}

/**
 * Derive event status from intervalPeriod vs current time.
 * - no intervalPeriod.start → "immediate"
 * - now < start → "scheduled"
 * - start <= now < end → "active"
 * - now >= end → "completed"
 */
export function getEventStatus(event: VtnEvent, now?: Date): EventStatus {
  const ip = event.intervalPeriod;
  if (!ip?.start) return "immediate";

  const currentTime = (now ?? new Date()).getTime();
  const start = new Date(ip.start).getTime();

  if (isNaN(start)) return "immediate";

  if (currentTime < start) return "scheduled";

  if (ip.duration) {
    const durationMs = parseDuration(ip.duration);
    if (durationMs > 0) {
      const end = start + durationMs;
      if (currentTime >= end) return "completed";
    }
  }

  return "active";
}

/**
 * Map status to MUI Chip color.
 */
export function statusColor(status: EventStatus): "success" | "warning" | "default" | "info" {
  switch (status) {
    case "active": return "success";
    case "scheduled": return "info";
    case "completed": return "default";
    case "immediate": return "warning";
  }
}
