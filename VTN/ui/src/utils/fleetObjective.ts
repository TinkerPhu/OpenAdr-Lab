import type { FleetVenLive, VenObjective } from "../api/types";

/**
 * The objectives a fleet-wide switch may offer: those every VEN that declares its objectives
 * accepts. The BFF refuses a fleet switch any declaring VEN does not accept (all or nothing), so
 * offering anything else would offer a button that always fails.
 */
export function fleetObjectiveChoices(vens: FleetVenLive[]): string[] {
  const declared = vens
    .map((v) => v.objective?.available_objectives)
    .filter((a): a is string[] => Array.isArray(a));
  if (declared.length === 0) return [];
  return declared[0].filter((o) => declared.every((list) => list.includes(o)));
}

/** How many VENs report each objective in force. */
export function objectiveCounts(vens: FleetVenLive[]): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const v of vens) {
    const name = v.objective?.objective;
    if (name) counts[name] = (counts[name] ?? 0) + 1;
  }
  return counts;
}

/** The last time any VEN's objective changed, as the BFF saw it (`objectiveChangedAt`). */
export function latestObjectiveChange(vens: FleetVenLive[]): string | undefined {
  return vens
    .map((v) => v.objectiveChangedAt)
    .filter((t): t is string => typeof t === "string")
    .reduce<string | undefined>((a, b) => (a === undefined || Date.parse(b) > Date.parse(a) ? b : a), undefined);
}

/** "opportunistic 3 · min_cost 2": the objectives behind a fleet view, most VENs first. */
export function objectiveMix(objectives: Record<string, number>): string {
  return Object.entries(objectives)
    .sort(([a, na], [b, nb]) => nb - na || a.localeCompare(b))
    .map(([name, n]) => `${name} ${n}`)
    .join(" · ");
}

/** Who set the objective in force, in the words the VEN UI uses. */
const SOURCE_LABEL: Record<VenObjective["source"], string> = {
  profile: "profile",
  ven_ui: "VEN UI",
  fleet: "fleet",
};

export function sourceLabel(source: VenObjective["source"]): string {
  return SOURCE_LABEL[source] ?? source;
}
