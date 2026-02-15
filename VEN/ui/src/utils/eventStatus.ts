import type { VtnEvent } from "../api/types";

export type EventStatus = "scheduled" | "active" | "completed" | "immediate";

/**
 * Parse an ISO 8601 duration string (e.g. "PT4H", "PT30M", "PT1H30M") into milliseconds.
 * Supports days (D), hours (H), minutes (M), seconds (S).
 */
function parseDuration(dur: string): number {
  const match = dur.match(/^P(?:(\d+)D)?T?(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?$/);
  if (!match) return 0;
  const days = parseInt(match[1] || "0", 10);
  const hours = parseInt(match[2] || "0", 10);
  const minutes = parseInt(match[3] || "0", 10);
  const seconds = parseFloat(match[4] || "0");
  return ((days * 24 + hours) * 60 + minutes) * 60000 + seconds * 1000;
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
