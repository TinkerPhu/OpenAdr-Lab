# Design

## Context

See proposal.md for the motivation. The facts below shape the approach.

**Live tick.** `tasks/sim_tick/setpoints.rs::build_tick_setpoints` runs one pipeline per tick:
base setpoints (`dispatcher::build_setpoints`: plan allocations, else each asset's
`default_setpoint_kw`, plus thermostat setpoints where the plan allocates nothing) → either
`arbiter::reconcile` (deviation correction, off by default) or `dispatcher::apply_surplus_ev_overlay`
→ DISPATCH_SETPOINT override → comms-loss clamp → `arbiter::limit::enforce_import_limit` (on by
default, runs with or without a plan). The tick takes its objective from the **adopted plan**
(`plan_snap.objective`), not from `ActiveObjective`.

**No-plan window.** `reconcile` without a plan slot calls only `apply_ev_lever_opportunistic` and
returns. With deviation correction disabled (the default) the no-plan path is
`apply_surplus_ev_overlay`, which calls the same function. Battery and heater do nothing.

**Shared lever machinery.** `arbiter::apply_ranked_levers` builds candidates (`battery_lever`,
`ev_lever`, `heater_pause_lever`, `heater_emergency_lever`, `pv_curtailment_lever`), ranks them
with `rank_levers` (marginal cost, incumbent hysteresis) and applies them greedily. What differs
between passes is declared as a `LeverPolicy` (`DEVIATION_POLICY`, `limit::LIMIT_POLICY`).

**Objective switch.** `PUT /plan/objective` (`routes/hems/misc.rs`) writes `ActiveObjective`
(`Arc<RwLock<PlannerObjective>>`, seeded from the profile in `boot/mod.rs`, not persisted) and
sends `PlanTrigger::UserRequest`. The VEN UI Planner page sends it and reads the objective back
from `plan.objective`; there is no GET for the objective itself.

**Planning cycle.** `tasks/planning/cycle.rs` clones the whole `SimState`
(`simulator::plan_context::clone_sim_snapshot`), assembles a `SolveRequest`
(`tasks/planning/assemble.rs`), solves through `SolverPort` (`milp_planner::MilpSolver` →
`run_planner`), then gates (`services::planning::evaluate_acceptance_gate`: any non-periodic
trigger adopts) and adopts. Plan assembly is `milp_planner::results::translate_to_plan` (from a
`SolveOutput`); `fallback_plan` builds the same slot skeleton a second time. W1
(`openspec/changes/asset-declared-planner-model`, 0 of 29 tasks done) replaces the per-kind
`SolveOutput` fields with `assets: Vec<AssetSolution>` and makes allocation one loop ordered by
a declared `surplus_claim_rank` (EV 0, heater 1, shiftable 2, battery 3).

**Simulator.** `SimState::tick(TickInputs)` steps every asset with `Asset::step(state, setpoint,
dt)`, honours `pv_measured_kw` / `base_load_measured_kw`, `ev_plugged_override`,
`ev_soc_target_override`, and integrates the grid meter. `dt_s` is a parameter.

**Fleet channel.** `fleet_telemetry.rs` publishes `status`, `telemetry`, `trace` on `lab-mqtt`
under `openadr-lab/fleet/<venName>/`; the BFF (`VTN/bff/src/fleet.rs`) subscribes to
`<root>/fleet/+/#` and ignores leaves it does not know. Phase 0 §6.2 designs a retained `plan`
leaf that nobody publishes yet. No command flows from the BFF to a VEN.

**OpenADR forecast.** The VEN already answers a `USAGE_FORECAST` obligation from the adopted plan
(`controller/report_intervals.rs::build_forecast_intervals`, energy per slot via
`forecast_payload`). The seeded `fleet-telemetry` event requests only `DEMAND` (and
`OPERATING_STATE`). `frequency: 0` (spec: ad hoc, "the VEN generates a report at any time it
chooses") is not handled: `openadr_interface::report_timing` treats it like the default. The BFF
recorder stores every report in Postgres; nothing reads forecasts.

**Requests.** `UserRequestMode` (`entities/design_vocabulary.rs`) already declares
`states_a_firm_deadline` and `charges_until_departure` beside its variants. EV sessions, heater
targets and shiftable loads carry a `mode`; only the MILP reads it today.

## Inventory — where each concept lives, and what this change does with it

| Concept | Lives in | Here |
|---|---|---|
| Lever candidate/rank/apply loop | `arbiter.rs::apply_ranked_levers`, `rank_levers` | Reused; ranking basis becomes a `LeverPolicy` field |
| Per-pass rules as data | `LeverPolicy`, `DEVIATION_POLICY`, `LIMIT_POLICY` | Gains `OPPORTUNISTIC_POLICY` |
| EV surplus decision | `arbiter_levers::apply_ev_lever_opportunistic` | Becomes the EV case of the generic `LEVER_ABSORB` (D2); the EV-only no-plan branch in `reconcile` is deleted |
| Pre-arbiter EV overlay | `dispatcher::apply_surplus_ev_overlay` | Kept for planned objectives with deviation correction off; calls the same EV candidate code |
| Asset surplus capability | `RequestResolvable::surplus_charge_kw` (test-only) **and** the EV lever's `power_when_command_lands_kw` read | The capability/response/available-energy fields already on `AssetSnapshot` are the answer for every asset (D2); `surplus_charge_kw` deleted |
| Surplus-claim order | W1 D7 `AssetMilpContext::surplus_claim_rank` (planned); today the fixed block order in `results.rs` | One `Asset::surplus_claim_rank()` per kind, on `AssetSnapshot`; the MILP context and the arbiter both read it (D3) |
| Asset response to a command | `AssetSnapshot::{power_when_command_lands_kw, power_drawn_for_setpoint_kw, setpoint_for_power_at_or_below_kw}` | The generic levers' capacity and apply (D2) |
| Battery lever | `battery_lever`, `apply_battery_lever` (dead-beat seeding, `MaxRevenue` refusal, lag bridge) | Unchanged for the deviation and limit passes; the opportunistic pass moves the battery through the generic levers (D2). Consolidation filed as debt |
| Heater above comfort | `heater_emergency_lever` Absorb (past `temp_max_c`, needs a plan slot) | Not used by the opportunistic pass; a heater surplus candidate stops at `temp_max_c` |
| Max effort of an asset | `Asset::max_effort_setpoint` (import = `capability.max_import_kw`), on the snapshot as `cap_max_import_kw` | Start-now requests (D4) |
| Request semantics | `UserRequestMode::{states_a_firm_deadline, charges_until_departure}` | Gains `non_planning_dispatch()` beside them |
| Heater thermostat rule | `assets/heater_thermostat.rs` (cut-out at `temp_max_c`, latched off until `temp_max_c − thermostat_delta_c`; emergency at `temp_min_c`); per-tick `TickInputs::heater_temp_max_override` (snaps back to the profile value when `None`) | A start-now heater request regulates at its target through this rule (D4) |
| Bare on/off thermostat | `Thermostat::thermostat_setpoint_kw` (`assets/heater_capabilities.rs`: full power below target, off at or above, no hysteresis) | **Not used** by this change: at a 1 s tick it would switch every tick around the target. R-140 (`TECHNICAL_DEBTS.md`) moves the sim's comfort-target inject onto the same ceiling override and deletes it; whichever lands second keeps one precedence: a sim inject beats a request target |
| EV target and plugging | `TickInputs::{ev_soc_target_override, ev_departure_time}`; `EvCharger::apply_usage_sim_tick` (plugging follows the EV's own usage schedule at `now`) | Session target injected under opportunistic dispatch (D4); the forecaster's slot `now` drives plugging (D6) |
| Base setpoints | `dispatcher::build_setpoints` | Gains an "ignore allocations" input; one function for both regimes |
| Limit pass | `limit::enforce_import_limit`, `hard_import_limit_kw`, `limit_target_kw` | Reused unchanged, live and in the forecast |
| Scheduled limit at t | `tightest_capacity_limit` (used in `setpoints.rs`) | Reused per forecast slot |
| Plan assembly | `results::translate_to_plan`; slot skeleton duplicated in `results::fallback_plan` | Forecast assembles through `translate_to_plan` (post-W1 `SolveOutput`); the skeleton copy is extracted into one function both call |
| Forecast inputs | `milp_planner::inputs::build_milp_inputs` | Reused for slot grid, PV, base load, tariffs, caps |
| Simulator step | `SimState::tick` | The forecaster's physics, at `dt_s` = slot length |
| Adoption gate | `services::planning::evaluate_acceptance_gate` | Gains "a non-planning objective's plan always adopts" |
| Objective state | `ActiveObjective` (`boot/mod.rs`, `app_ctx.rs`) | Becomes `ObjectiveSetting { objective, source, set_at }`; one setter for route and fleet command |
| Fleet MQTT publish | `fleet_telemetry.rs` (`status`, `telemetry`, `trace`) | Adds `objective`, `plan`, and a subscription to `command/objective` |
| Fleet MQTT ingest | `bff/src/fleet.rs::apply_message`, `parse_topic` | Ingests `objective`, `plan`, `command/objective` |
| Forecast report | `report_intervals::build_forecast_intervals`, `forecast_payload` | Reused; submitted on adoption for `frequency: 0` |
| Report timing | `openadr_interface::report_timing` | Handles `frequency: 0` |
| Energy ↔ power over an interval | `entities/units.rs::energy_kwh` (VEN), used by `OadrReportPayload::energy_from_power_kw` | The conversion and its inverse (`mean_power_kw`) move to `lab_core::units`; `entities/units.rs` re-exports them, so the VEN audit's "conversions only in units.rs" rule still holds and the BFF uses the same function |
| Fleet alignment and sum | `bff/src/fleet_store.rs::group_and_resample`, `sum_over_grid` | Reused for the forecast series |
| Seeded contracts | `scripts/seed_vtn.py` `PAYLOAD_CONTRACT`, fleet-monitoring event | `USAGE_FORECAST` descriptor and its unit added |

## Goals / Non-Goals

**Goals:**
- One opportunistic decision, used by live dispatch, the no-plan window and the forecast.
- The forecast is an ordinary `Plan`, assembled by the planner's own plan-assembly code.
- The objective switch works the same from the VEN UI, the VEN route and the VTN UI, and its
  source is visible everywhere.

**Non-Goals:**
- Forecasting DISPATCH_SETPOINT windows or comms loss: they are not schedulable in advance, so
  the forecast ignores them (live dispatch still applies them).
- Export-limit enforcement or PV curtailment in opportunistic dispatch: a non-planning HEMS
  exports what it cannot use. The existing export handling is unchanged.
- A per-asset "charging strategy" setting. The EV overlay toggle stays the only per-asset
  opportunistic switch.
- A per-VEN topic ACL on `lab-mqtt` (phase 0 §6.1 still owns that).
- Persisting a VEN-UI objective change across restarts (only a retained fleet command persists).
- Changing the heater model: one parameterised heater with a temperature-only deadband (no
  minimum off-time) stays as it is, for space-heating buffers and hot-water boilers alike.
  Weather-dependent space-heating demand is out of scope.

## Decisions

### D1. The objective declares whether dispatch follows the plan
`PlannerObjective::Opportunistic` (wire `opportunistic`) is added with a declared
`dispatches_from_plan() -> bool` (false only for it) beside the variants, like
`UserRequestMode::states_a_firm_deadline`. `build_phase1_weights` is not reached for it (D6).
The tick reads it from the adopted plan, as it reads every objective today — a switch takes
effect when the new plan is adopted, a few seconds after the trigger.

*Alternative rejected:* the tick reads `ActiveObjective` directly. Faster by one solve, but then
the tick and the plan in force can disagree about the objective, and `MaxRevenue`'s discharge
refusal already reads the plan's objective.

### D2. One opportunistic pass on the shared lever loop, with generic levers
`OPPORTUNISTIC_POLICY` is a `LeverPolicy` whose candidates are two **generic** lever kinds,
built for every asset that declares a surplus-claim rank (D3), not one helper per asset kind:

- `LEVER_ABSORB` (surplus direction): capacity = what the asset would draw if offered the
  surplus — `AssetSnapshot::power_when_command_lands_kw(min(surplus, cap_max_import_kw))`, zero
  when `available_charge_kwh` is `Some(0)`. Applied by commanding
  `setpoint_for_power_at_or_below_kw` for the assigned amount.
- `LEVER_DISCHARGE` (import direction): capacity = `min(import, cap_max_export_kw)`, zero when
  `available_discharge_kwh` is `Some(0)`. Applied the same way with the export sign.

Both read only fields `AssetSnapshot` already carries for every asset (capability, response,
available energy). The asset answers through them: a full battery, an unplugged or finished EV,
a heater its thermostat holds off at `temp_max_c`, a shiftable load past its window all report
zero import capability; the response encodes stages, minimum rates and all-or-nothing starts
(R-81/R-82). No new per-asset "surplus" method is needed, and `surplus_charge_kw` (test-only, a
second answer to the same question) is deleted.

`Lever` gains the asset id it moves (`Option<&str>`, borrowed from the snapshot), so the shared
`apply_ranked_levers` loop dispatches the two generic kinds by data, alongside the existing
per-kind levers. The loop is unchanged otherwise: the achieved amount is what the asset draws
**this tick** (`power_drawn_for_setpoint_kw`), so a lagging charger passes the excess to the
next lever, as `LeverEffect` does today. The pass starts every tick from the base setpoints
(battery and EV at their default 0 kW, not their last command) and reads `projected_net_kw`
once, so it is memoryless: no integrator, no lag bridge, nothing to wind up. The direction is the
sign of the imbalance; each lever is capped at the remaining imbalance, so the battery never
charges from the grid or discharges into export.

`rank_levers` ranks by the policy's basis: `Cost` (today's passes) or `DeclaredRank`
(opportunistic), with the incumbent hysteresis kept for both.

`apply_ev_lever_opportunistic` becomes the EV case of `LEVER_ABSORB`: the pre-arbiter overlay
(`dispatcher::apply_surplus_ev_overlay`, planned objectives with deviation correction off)
builds that one lever for the EV and applies it through the same function. Heater pause, heater
emergency and PV curtailment are not opportunistic candidates. `reconcile`'s EV-only no-plan
branch is deleted; `build_tick_setpoints` calls `arbiter::opportunistic_step` (pass + limit pass)
whenever there is no plan slot or `!objective.dispatches_from_plan()`, independent of the
deviation toggle.

The per-kind `battery_lever`/`ev_lever` of the deviation and limit passes are instances of the
same generic shape plus pass-specific rules (dead-beat seeding, `MaxRevenue` refusal, lag
bridge). Folding them in is out of reach here and is filed in `TECHNICAL_DEBTS.md`.

### D3. Assets declare their surplus-claim rank
`Asset::surplus_claim_rank() -> Option<u8>` (EV 0, heater 1, shiftable load 2, battery 3; PV and
base load `None`) is carried on `AssetSnapshot`. `None` means the asset is never offered surplus
or asked to discharge. The values are W1's attribution order, so the planner's attribution and
the live dispatch order are one declaration: if W1 has landed, its
`AssetMilpContext::surplus_claim_rank` reads the asset's value; if not, W1's task 6.1 is updated
to read it. Equal ranks keep roster order.

*Alternative rejected:* battery first (several commercial HEMS default to it). It would make the
dispatch order differ from the planner's attribution order and need a second declaration.
Battery last also gives the classic profile: deferrable loads take the noon surplus, the battery
takes the rest and covers the evening.

### D4. Request modes declare their non-planning behaviour
`UserRequestMode::non_planning_dispatch() -> NonPlanningDispatch::{StartNow, SurplusOnly}`:
`Asap`, `ByDeadline` → `StartNow`; `AsapFree`, `ByDeadlineFree`, `Opportunistic`, `MaxCost` →
`SurplusOnly`. `MaxCost` is surplus-only because without a price forecast the only energy known
to fit any budget is surplus. The tick collects active requests (`current_ev_session`, the
active heater target, active shiftable loads) into `ArbiterTick.active_requests: Vec<(asset_id,
NonPlanningDispatch)>`. The request's target reaches the asset through the path
each asset already has for a target, so the asset's own answer says when it is done — the
arbiter never compares a state with a target:

- EV: under opportunistic dispatch, `ev_soc_target_override` = the active session's target when
  no sim inject sets it. Its capability then reports zero import at the target.
- Heater: under opportunistic dispatch, `heater_temp_max_override` = `min(target_temp_c,
  temp_max_c)` of the active start-now heater request when no sim inject sets it. The heater's
  own thermostat rule then cuts out at the target and stays off until it has cooled by
  `thermostat_delta_c`, so the request is held at its target with the real thermostat's deadband
  and never chatters. The bare on/off `Thermostat::thermostat_setpoint_kw` is not used: it has no
  hysteresis and would switch every tick at the target.
- Shiftable load: its own duration and window.

`StartNow` sets the base setpoint to `AssetSnapshot::cap_max_import_kw` (what
`Asset::max_effort_setpoint` returns for import) for every asset kind; with the targets injected
as above, that ceiling is zero once the asset is done (EV at target, heater latched at its
request target, shiftable load finished). A start-now asset is not also an absorb candidate. `SurplusOnly`
makes the asset an absorb candidate even when the EV overlay toggle is off — the request asks
for surplus explicitly; the toggle only governs an EV with no active request. Planned objectives
keep today's behaviour: the MILP serves requests and none of these injections happen.

### D5. Base setpoints are one function for both regimes
`dispatcher::build_setpoints` gains `follow_allocations: bool`. With `false` it returns each
asset's default command plus any thermostat setpoint (the existing "no allocation" branch). For
the heater that means its own floor: default 0 kW, with the asset forcing heat at `temp_min_c`,
and surplus lifting it toward `temp_max_c` (D3) — the common "PV boost" boiler pattern, with no
new heater rule. The
tick passes `objective.dispatches_from_plan()`; the no-plan branch in `build_tick_setpoints`
becomes a call with `false`, so the "no plan" and "opportunistic" bases are the same code.

### D6. The forecast steps a clone of the simulator with the same decision
`simulator/opportunistic_forecast.rs` takes the cycle's cloned `SimState`, the `MilpInputs` slot
series and the grid signals. Per slot it: builds the `SimSnapshot`, the `ArbiterTick` (slot-mean
PV and base load, the scheduled limit at the slot start via `tightest_capacity_limit`, alert
windows covering it, active requests at the slot start) and the base setpoints (D5); runs the
same controller function the live tick runs (`arbiter::opportunistic_step`: opportunistic pass +
limit pass); then `SimState::tick` with `dt_s` = slot length, `pv_measured_kw` and
`base_load_measured_kw` set to the slot means, `now` = the slot start (so the EV's own usage
schedule decides when it is plugged, as it does live), and the request targets of D4. It records per-asset power and end state per slot. One evaluation per slot, by
decision (proposal, "coarse").

`milp_planner` gains `plan_from_forecast(inputs, trace, ...)`: it builds a W1 `SolveOutput`
(grid series from the trace, slack zero, `assets: Vec<AssetSolution>` from per-asset power and
state) and calls `translate_to_plan` with `SolveStatus::Simulated`, `friction_eur = 0` and the
marginal values of D8. `tasks/planning/cycle.rs` holds the single guard: `dispatches_from_plan()`
→ solve, else forecast. Gate, adoption, events and history are shared.

*Alternative rejected:* a second `SolverPort` implementation. `SolveRequest` carries MILP
contexts, not a steppable roster, and `controller/` may not reach `simulator/`; the adapter
already holds the clone. *Alternative rejected:* re-implementing physics per asset with
`Asset::simulate_forward`. It steps one asset at a time, but the pass needs the whole site's
balance each slot; `SimState::tick` is the one site-level step.

### D7. Simulated plans always adopt
`SolveStatus::Simulated` (`solved()` true) is added; `evaluate_acceptance_gate` adopts any plan
whose objective `!dispatches_from_plan()`. The VEN UI shows `SIMULATED` wherever it shows the
status.

### D8. Marginal values of a forecast slot
`marginal_cost_{import,export}_eur_per_kwh` = import tariff if the slot imports or is balanced,
export tariff if it exports. Live opportunistic dispatch does not read them (D2 ranks by
declaration), so they are descriptive only.

### D9. One objective setting, one setter
`ObjectiveSetting { objective, source: Profile | VenUi | Fleet, set_at }` replaces
`ActiveObjective`'s bare value. `services::objective::set_objective(setting, objective, source,
now)` writes it and sends `PlanTrigger::UserRequest`; the route and the fleet-command consumer
both call it. New `GET /plan/objective` serves the setting plus `available_objectives` (every
variant), read by the Planner page and the Dashboard objective chip, and the same body goes out
retained on the `objective` leaf.

### D10. Fleet commands on the lab broker
VEN: the existing `fleet_telemetry` client subscribes to `<topic_root>/command/objective` after
every ConnAck (a retained command arrives at once). Payloads go through an mpsc channel to a
small consumer in `tasks/` that calls `set_objective(.., Fleet, ..)`. The consumer remembers the
`requested_at` it last applied and ignores a redelivery of the same command, which the broker
sends after every reconnect; after a restart the memory is empty, so the retained command applies
(the restart case). An empty payload reverts to the profile objective only while the active
source is `Fleet`; an unreadable payload is logged and dropped. Command body:
`{"objective": "<name>", "requested_at": "<UTC>"}` — the `PUT /plan/objective` body plus the
issue time.
BFF: the ingest client gains a publish handle; `PUT /api/fleet/vens/:ven/objective` and `PUT
/api/fleet/objective` take `{"objective": "<name>" | null}` (null clears), validate against that
VEN's `available_objectives` from its `objective` leaf, and publish retained, QoS 1. The BFF
ingests its own retained commands from the same subscription, so "commanded" and "reported" come
from the broker, and the UI shows pending while they differ.

*Alternative rejected:* the BFF calls each VEN's HTTP API. The BFF does not know VEN addresses,
Node2 VENs are not on the VTN's docker network, and it adds a second path beside the channel the
lab already runs. *Alternative rejected:* an OpenADR event carrying the objective. OpenADR has
no such field; a private payload type would be exactly the overloaded field `wire-contracts`
forbids.

### D11. Fleet forecast over OpenADR, drill-down over MQTT
`seed_vtn.py` adds to the fleet-monitoring event a `USAGE_FORECAST` descriptor (`historical:
false`, `readingType: FORECAST`, `frequency: 0`) and `PAYLOAD_CONTRACT["USAGE_FORECAST"] =
{"units": "KWH"}`. `report_timing` maps `frequency: 0` to "VEN's choice": for forecast
obligations, submit on adoption (the adoption path re-arms them as due); for measurement
obligations, the profile default cadence, logged. The BFF reads, per VEN, the newest
`USAGE_FORECAST` report from the recorder's table, converts each interval with `lab_core::units::mean_power_kw`
(declared unit and interval duration → mean kW; a declared `KW` is taken as-is), and sums with
`group_and_resample` / `sum_over_grid`. `GET /api/fleet/forecast` serves `{slots: [{start, kw,
vens, objectives}]}`. The `plan` leaf is published on adoption; the BFF keeps the latest per VEN
and serves `GET /api/fleet/vens/:ven/plan` for the Fleet page drill-down.

### D12. VTN UI
Fleet table gains Objective (value, source, pending marker) with a per-row selector and a fleet
selector above the table, plus "Release" (clear) per row and for all. `FleetPowerChart` overlays
the forecast series (dashed) with a legend listing objectives and VEN counts. A row expands to a
small chart of that VEN's plan (net and per-asset allocation).

## Risks / Trade-offs

- [Coarse slots smooth the forecast; a 15-minute thermostat or EV clamp can over- or undershoot a
  target within a slot] → Asset `step` clamps at its own limits; accepted by decision; the guard
  test pins agreement per step, not per second.
- [`SimState::tick` carries smoothing state (PV, base load) that a coarse step would misuse] →
  The forecaster sets the measured overrides every slot, which bypass smoothing; a unit test pins
  that the forecast PV equals the input series.
- [Retained `plan` messages are large (≈100 kB for a 24 h horizon)] → QoS 1, retained, one per
  adoption; mosquitto's default size limit is far above it. Measured in the BDD run.
- [Any lab-mqtt client can switch objectives] → Recorded in `TECHNICAL_DEBTS.md` against the
  phase-0 ACL item; commands only change the objective.
- [Fleet command and VEN UI both set the objective; a restart restores the fleet value] → Source
  is shown on both UIs; documented in the use case.
- [W1 has not started; D6's plan assembly needs it] → Tasks are ordered so dispatch, requests,
  the objective plumbing and fleet control ship first; the forecast group is gated on W1.
  Until then an opportunistic cycle adopts a plan with no slots and a warning
  ("opportunistic — no forecast yet"), so dispatch already works and the gap is visible.
- [The fleet forecast joins OpenADR reports (VTN `venName`) with MQTT objectives (topic
  `venName`)] → Both come from the VEN's one configured name; the BFF test pins the join, and a
  report from a VEN with no MQTT objective is counted with objective `unknown`, not dropped.
- [Changing `frequency: 0` handling alters how any event using it is reported] → No seeded event
  uses it today; the measurement case logs the applied cadence.

## Migration Plan

No data migration. `ObjectiveSetting` and `SolveStatus::Simulated` are additive on the wire
(TS types regenerated). Rollback is a revert; retained commands on the broker can be cleared
from the VTN UI or with `mosquitto_pub -r -n`.

## Open Questions

- The exact `AssetSolution` field names come from W1's implementation; D6 adapts to them.
