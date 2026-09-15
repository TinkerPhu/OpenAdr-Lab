import type { TraceEntry } from "../../api/types";

type ArbiterDecision = Extract<TraceEntry, { type: "ArbiterDecision" }>;

/** Mirrors the backend's `DEAD_BAND_KW`: a smaller residual is not reported
 * as unresolved (`controller::arbiter::decision_event`). */
export const UNRESOLVED_THRESHOLD_KW = 0.1;

function formatKw(kw: number | null): string {
  return kw === null ? "—" : `${kw.toFixed(2)} kW`;
}

export function isUnresolved(decision: ArbiterDecision): boolean {
  return decision.unresolved_kw > UNRESOLVED_THRESHOLD_KW;
}

/** One line per GB-47 arbiter decision: which pass, which lever, what it
 * steered to, and any excess it could not shed. */
export function arbiterDecisionText(decision: ArbiterDecision): string {
  const parts = [
    `${decision.pass}: ${decision.active_lever ?? "no lever"}`,
    `target ${formatKw(decision.target_kw)}`,
    `excess ${formatKw(decision.excess_kw)}`,
  ];
  if (isUnresolved(decision)) parts.push(`unresolved ${formatKw(decision.unresolved_kw)}`);
  return parts.join(" · ");
}
