import { useEffect, useState } from "react";

/** `value`, but only once it has stayed unchanged for `delayMs` — so a stream of
 * rapid changes (cursor movement) settles into one update instead of one per event. */
export function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);
  return settled;
}
