# Proposal

## Why

The lab can show that planning moves load in time, but not what the same fleet would do without
planning, under the same weather, tariff and base load. The baseline every planning HEMS is
measured against is the common non-planning one ("opportunistic": use PV surplus as soon as it
is there, cover import from the battery, no look-ahead). Without it, every planning result is a
number with nothing to compare it to. Making it a planner objective, with its own forecast,
lets an experimenter switch a whole fleet from the VTN UI and see the fleet forecast change at
the same instant — no day-long recordings, no confounded comparison.

## What Changes

- **New planner objective `opportunistic`** (`PlannerObjective::Opportunistic`), selectable in
  the VEN UI Planner page and through `PUT /plan/objective`, like every other objective.
- **Opportunistic dispatch on the VEN.** Under that objective the live tick does not follow plan
  allocations. One arbiter pass reads the site balance: PV surplus is absorbed by battery, EV,
  heater and shiftable loads in a declared order; site import is covered by battery discharge.
  The battery never charges from the grid and never discharges into export. The existing
  limit-enforcement pass still bounds the result.
- **User requests keep their mode's meaning without look-ahead.** `ASAP` and `BY_DEADLINE`
  requests start now at full effort; `ASAP_FREE`, `BY_DEADLINE_FREE`, `OPPORTUNISTIC` and
  `MAX_COST` requests take surplus only. Declared on `UserRequestMode`, beside the variants.
- **Planning objectives are untouched.** Under every other objective the VEN decides every tick
  and every plan exactly as before: the opportunistic code sits behind two guards on the
  objective, the planning no-plan window and the EV surplus overlay are not changed, and a
  recorded tick golden, the plan golden and the gate golden must hold unchanged. Switching back
  from opportunistic always adopts the next plan, even a failed one, so a VEN never keeps
  dispatching opportunistically under a planning objective.
- **Opportunistic forecast.** Under the objective the planning cycle produces a normal `Plan` by
  stepping a clone of the simulator forward at the plan step with the opportunistic pass deciding
  each step — no MILP. `/plan`, `/forecast`, `/history/plans`, envelopes, the `USAGE_FORECAST`
  report and the fleet views read it unchanged. An opportunistic plan always replaces the
  previous one (there is no cost improvement to gate on).
- **Generic surplus and discharge levers.** The pass moves any asset through two generic levers
  that read only what every asset already reports about itself (capability, response, available
  energy), ordered by one new declaration per asset kind: its surplus-claim rank. The test-only
  `RequestResolvable::surplus_charge_kw` (a second answer to the same question) is deleted, and
  the EV surplus overlay becomes the EV case of the generic lever.
- **Fleet objective control from the VTN UI.** The Fleet page shows each VEN's active objective
  and where it came from, and can switch one VEN or all of them. The BFF publishes a retained
  command on the lab broker (`openadr-lab/fleet/<venName>/command/objective`); the VEN applies
  it, replans, and publishes its active objective back. A retained command also restores the
  objective after a VEN restart. Clearing the command returns the VEN to its profile objective.
- **Fleet forecast over OpenADR.** The standing fleet-monitoring event requests a
  `USAGE_FORECAST` report (`frequency: 0`, ad hoc), which every VEN submits on each plan
  adoption — the VEN side already builds this report. The BFF sums the latest report per VEN into
  a fleet forecast ("portfolio position"), and the Fleet power chart overlays it on recorded
  power. VENs also publish their adopted plan on the lab broker's `plan` leaf (designed in
  fleet-monitor phase 0) for a per-VEN drill-down on the Fleet page.
- **Objective source visible on the VEN.** The VEN UI shows whether the active objective comes
  from the profile, the VEN UI or the fleet command.
- **Removed:** `RequestResolvable::surplus_charge_kw` (test-only). (The
  former master plan `docs/plans/opportunistic-objective-fleet-forecast.md` is replaced by this
  change.)

## Capabilities

### New Capabilities
- `opportunistic-dispatch`: how a VEN dispatches without look-ahead — the opportunistic objective,
  the surplus and import directions, the surplus-claim order, request-mode behaviour, and the
  guarantee that planning objectives are left exactly as they are.
- `opportunistic-forecast`: the forecast a VEN publishes under the opportunistic objective, its
  shape, its adoption, and its agreement with live dispatch.
- `fleet-objective-control`: switching VEN objectives from the VTN UI through the lab broker,
  reporting the active objective and its source, and the fleet forecast series.

### Modified Capabilities
- None. `openspec/specs/` holds no capability today.

## Impact

- **VEN:** `entities/planner_params.rs`, `entities/design_vocabulary.rs`,
  `controller/arbiter.rs` and `arbiter/{arbiter_levers,limit}.rs`, `controller/dispatcher.rs`,
  `controller/sim_snapshot.rs`, `controller/openadr_interface.rs` (`frequency: 0`),
  `controller/milp_planner/{results,mod}.rs`, `assets/{battery,ev,heater,shiftable_load,
  capability_traits,asset_trait}.rs`, `simulator/snapshot.rs`, a new
  `simulator/opportunistic_forecast.rs`, a new `lab-core/src/units.rs`,
  `tasks/planning/cycle.rs`, `tasks/sim_tick/setpoints.rs`, `services/planning/mod.rs`
  (adoption gate), `routes/hems/misc.rs`, `fleet_telemetry.rs` (command subscribe, `objective`
  and `plan` leaves), `boot/` (objective state with source), VEN UI Planner page and Dashboard.
- **VTN:** `bff/src/fleet.rs` (ingest `objective`, `plan` and `command/objective` leaves, publish
  commands), `bff/src/routes/fleet.rs` and `main.rs` (objective routes, fleet forecast from the
  recorder's report table, per-VEN plan), `scripts/seed_vtn.py`,
  `ui/src/pages/Fleet.tsx`, `ui/src/components/FleetPowerChart.tsx`.
- **Wire:** OpenADR: a `USAGE_FORECAST` report descriptor on the seeded fleet-monitoring event
  (declared unit `KWH`), and the VEN honours `frequency: 0` (ad hoc) as the spec defines it. No
  new payload types. Lab broker: two new leaves (`command/objective`, `objective`); the `plan`
  leaf goes from designed to published. All three carry the JSON shape of an existing VEN route
  body (`dto`).
- **Depends on:** `openspec/changes/asset-declared-planner-model` (W1) for the forecast's plan
  assembly (`SolveOutput` with `AssetSolution`s through `translate_to_plan`). Dispatch, requests,
  objective control and the fleet channel do not wait for it; until W1 lands, an opportunistic
  plan has no slots and says so. The surplus-claim rank is declared here and W1 reads it.
  `fix/r94-heater-latch-pin` merges first (heater thermostat).
- **Tests:** arbiter unit tests per direction and per request mode, a live-vs-forecast agreement
  test, BFF topic/command unit tests, VEN and VTN UI unit tests, and BDD scenarios in
  `tests/features/` for VEN dispatch and for the fleet switch (the test stack already runs
  `mosquitto`, `FLEET_MQTT_HOST` and `test-bff`).
- **Risk:** the lab broker has no per-VEN topic ACL yet (fleet-monitor phase 0 §6.1), so any
  authenticated lab client can switch a VEN's objective. Acceptable for the lab; recorded.
