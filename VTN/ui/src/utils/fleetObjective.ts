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

/** "opportunistic 3 · min_cost 2": the objectives behind a forecast slot, most VENs first. */
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
