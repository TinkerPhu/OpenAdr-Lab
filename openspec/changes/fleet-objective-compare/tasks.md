# Tasks

- [x] 1. `planDelta.ts`: `cumulativeEnergyKwh(points, stepSeconds, flow)` and `seriesStats` on one conversion; tests first (`planDelta.test.ts`).
- [x] 2. `fleetPlan.ts`: `planRows` by quantity (power or accumulated, both curves level at the window start); tests first.
- [x] 3. `FleetPlanChart.tsx`: quantity toggle, declared per quantity (axis unit, formatter), opens on accumulated; tests first.
- [x] 4. `fleetObjective.ts`: `latestObjectiveSwitch` (first change of the newest cluster, 10 min gap); replaces `latestObjectiveChange`; tests first.
- [x] 5. `PlanComparison.tsx` takes the name of the change; `Fleet.tsx` uses it for the switch and drops its own faint overlay; tests first.
- [x] 6. BDD: `@ui` scenario in `fleet_objective_control.feature` (switch the test fleet, open the Fleet page, the card reports the replan progress, the accumulated chart is shown); use case in `SYSTEM-USE-CASES.md`; journal and KEY_LEARNINGS.
- [ ] 7. After merge and deploy: switch the Node2 fleet to opportunistic from the VTN UI, wait for every VEN to replan, read the card, release, read it again; record the numbers in the journal (closes `opportunistic-objective` 10.3).
  Partly done 2026-10-24: the day-long run was read once (service card, plan card) and released; no second reading after release (journal: "The day-long fleet run on `opportunistic`").
