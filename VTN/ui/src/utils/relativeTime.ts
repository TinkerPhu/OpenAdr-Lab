/**
 * How long ago something happened, in words.
 *
 * `now` is a parameter rather than read from the clock inside, for the same
 * reason the backend injects its clock (`determinism`): a formatter that reads
 * the wall clock can only be tested against the wall clock.
 */
export function formatAge(iso: string | null | undefined, now: Date): string {
  if (!iso) return "never";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "unknown";

  const seconds = Math.floor((now.getTime() - then) / 1000);
  // The BFF stamps its own receive time and the browser reads its own clock;
  // the two can disagree by a few seconds. "just now" is true either way,
  // where "-4s ago" would read as a fault in this page.
  if (seconds < 0) return "just now";
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}
