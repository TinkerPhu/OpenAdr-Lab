import { useState } from "react";
import { useHealth } from "../api/hooks";

/**
 * The server's clock, captured once and then frozen — the anchor for a page's
 * rolling-window query.
 *
 * Why the server's: every chart's time axis is built from it, so a client
 * whose OS clock has drifted (observed on a VM once) can no longer stretch or
 * squeeze the axis, and a 24 h window still lands where the data is. See
 * `Controller.tsx`'s `nowMs` for the full rationale.
 *
 * Why frozen: the window is meant to be computed once, not re-anchored on
 * every `/health` poll, which would reset table pagination underneath a user
 * mid-page.
 *
 * Set at most once, adjusted during render the first time `/health` resolves
 * (React's documented pattern for deriving state from a prop) rather than in
 * an effect, which would cost an extra commit for the same outcome. Until
 * then it falls back to a `Date.now()` snapshot taken once at mount — not
 * recomputed per render, which would change the derived window every render
 * and loop any state-reset that depends on it.
 *
 * Extracted from `pages/History.tsx`, whose copy was the only one:
 * `pages/PlanHistory.tsx` asked the browser instead, and so had the drift
 * problem this exists to avoid.
 */
export function useFrozenServerNowMs(): number {
  const health = useHealth();
  const [mountNowMs] = useState(() => Date.now());
  const [frozenServerNowMs, setFrozenServerNowMs] = useState<number | null>(null);
  if (frozenServerNowMs === null && health.data) {
    setFrozenServerNowMs(new Date(health.data.server_time).getTime());
  }
  return frozenServerNowMs ?? mountNowMs;
}
