import type { TraceEntry } from "../../api/types";

type RequestTransition = Extract<TraceEntry, { type: "RequestTransition" }>;

/** A request's status change as the trace shows it everywhere: `ACTIVE → CANCELLED`, or
 * `NEW → ACTIVE` for a created request (`from_status` is `null`, R-130). */
export function requestTransitionText(transition: RequestTransition): string {
  return `${transition.from_status ?? "NEW"} → ${transition.to_status}`;
}
