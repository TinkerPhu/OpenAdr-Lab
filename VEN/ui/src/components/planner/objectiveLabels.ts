import type { ObjectiveSource, PlannerObjective } from "../../api/types";

/** One label per planner objective, keyed by the generated union so a new variant cannot go
 * unlabelled. `custom` is the profile's own weights; it is not offered in the Planner's picker. */
export const OBJECTIVE_LABELS: Record<PlannerObjective, string> = {
  min_cost: "Cost",
  min_ghg: "GHG",
  min_grid: "Grid",
  min_import: "Autarky",
  max_revenue: "Revenue",
  custom: "Custom",
  opportunistic: "Opportunistic",
};

/** Who set the objective in force, as the Planner page and the Dashboard say it. */
export const OBJECTIVE_SOURCE_LABELS: Record<ObjectiveSource, string> = {
  profile: "from the profile",
  ven_ui: "set here",
  fleet: "set by the fleet (VTN UI)",
};
