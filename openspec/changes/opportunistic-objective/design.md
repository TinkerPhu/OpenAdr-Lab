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
`apply_surplus_ev_overlay`, which calls the same function. It offers surplus to every EV in
roster order, each under its own overlay (`EvOverlays`, one per EV since W4). Batteries and
heaters do nothing.

**Shared lever machinery** (`controller/arbiter/ranked.rs`, W5a). `lever_candidates` builds one
lever per asset that declares an `ArbiterLeverKind` (`Storage`, `SurplusCharge`, `PausableLoad`,
via `Asset::arbiter_lever` on `AssetSnapshot`), plus the heater emergency and PV curtailment
levers; each `Lever` carries a `LeverId { kind, asset_id }`. `rank_levers` sorts by marginal cost
(stable: kind precedence, then roster order) with incumbent hysteresis; `apply_ranked_levers`
applies greedily. What differs between passes is a `LeverPolicy` (`DEVIATION_POLICY`,
`DISPATCH_POLICY`, `limit::LIMIT_POLICY`), including the lever kinds it `admits`.

**Objective switch.** `PUT /plan/objective` (`routes/hems/misc.rs`) writes `ActiveObjective`
(`Arc<RwLock<PlannerObjective>>`, seeded from the profile in `boot/mod.rs`, not persisted) and
sends `PlanTrigger::UserRequest`. The VEN UI Planner page sends it and reads the objective back
from `plan.objective`; there is no GET for the objective itself.

**Planning cycle.** `tasks/planning/cycle.rs` clones the whole `SimState`
(`simulator::plan_context::clone_sim_snapshot`), assembles a `SolveRequest`
(`tasks/planning/assemble.rs`), solves through `SolverPort` (`milp_planner::MilpSolver` →
`run_planner`), then gates (`services::planning::evaluate_acceptance_gate`: any non-periodic
trigger adopts) and adopts. Plan assembly is `milp_planner::results::translate_to_plan` (from a
`SolveOutput`); `fallback_plan` builds the same slot skeleton a second time. Since W1,
`SolveOutput` carries `assets: Vec<AssetSolution>` (net power, planned state, storage legs,
warnings, a per-kind `detail`), and `translate_to_plan(sol, inputs, asset_contexts, ...)` orders
allocations by `AssetMilpContext::surplus_claim_rank`, declared per kind in
`assets/*_declared.rs` (EV 0, heater 1, shiftable load 2, battery 3, PV and base load as forecast
participants `u8::MAX`). `controller/milp_planner/tests/plans_golden.rs` pins whole plans.

**Simulator.** `SimState::tick(TickInputs)` steps every asset with `Asset::step(state, setpoint,
dt)` and integrates the grid meter; `dt_s` is a parameter. Inputs come in two shapes: the sim
inject's site-wide scalars (`heater_temp_max_override`, `ev_soc_target_override`, ...) and, since
W4/W5b, per-asset maps (`ev_departure_by_asset`, `heater_emergency_by_asset`, and the measured
readings and weather feeds in `AssetTickFeeds`).

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
| EV surplus decision | `arbiter_levers::apply_ev_lever_opportunistic` (its surplus excludes battery discharge and counts the battery's planned charge) | **Untouched** (D0). It and the generic `LEVER_ABSORB` answer the same question under different rules; consolidation filed as debt with an equivalence test as its first step |
| Pre-arbiter EV overlay | `dispatcher::apply_surplus_ev_overlay`; `reconcile`'s EV-only no-plan branch | **Untouched** (D0): both stay the planning objectives' no-plan and overlay behaviour |
| Asset surplus capability | `RequestResolvable::surplus_charge_kw` (test-only) **and** the EV lever's `power_when_command_lands_kw` read | The capability/response/available-energy fields already on `AssetSnapshot` are the answer for every asset (D2); `surplus_charge_kw` deleted |
| Surplus-claim order | `AssetMilpContext::surplus_claim_rank` in `assets/{ev,heater,shiftable,battery,forecast}_declared.rs`, read by `results::surplus_claim_order` | The per-kind value becomes one constant per kind, returned by both the declared context and a new `Asset::surplus_claim_rank()`, and carried on `AssetSnapshot` (D3) |
| Lever identity and kinds | `LeverId { kind, asset_id }`, `ArbiterLeverKind`, `SimSnapshot::{lever_asset_ids, assets_in_roster_order}`, `LeverPolicy::admits` | The two generic kinds are new `LeverId` kinds, admitted only by `OPPORTUNISTIC_POLICY` (D2) |
| Asset response to a command | `AssetSnapshot::{power_when_command_lands_kw, power_drawn_for_setpoint_kw, setpoint_for_power_at_or_below_kw}` | The generic levers' capacity and apply (D2) |
| Battery lever | `battery_lever`, `apply_battery_lever` (dead-beat seeding, `MaxRevenue` refusal, lag bridge) | Unchanged for the deviation and limit passes; the opportunistic pass moves the battery through the generic levers (D2). Consolidation filed as debt |
| Heater above comfort | `heater_emergency_lever` Absorb (past `temp_max_c`, needs a plan slot) | Not used by the opportunistic pass; a heater surplus candidate stops at `temp_max_c` |
| Max effort of an asset | `Asset::max_effort_setpoint` (import = `capability.max_import_kw`), on the snapshot as `cap_max_import_kw` | Start-now requests (D4) |
| Request semantics | `UserRequestMode::{states_a_firm_deadline, charges_until_departure}` | Gains `non_planning_dispatch()` beside them |
| Heater thermostat rule | `assets/heater_thermostat.rs` (cut-out at `temp_max_c`, latched off until `temp_max_c − thermostat_delta_c`; emergency at `temp_min_c`); per-tick `TickInputs::heater_temp_max_override` (snaps back to the profile value when `None`) | A start-now heater request regulates at its target through this rule (D4) |
| Bare on/off thermostat | `Thermostat::thermostat_setpoint_kw` (`assets/heater_capabilities.rs`: full power below target, off at or above, no hysteresis) | **Not used** by this change: at a 1 s tick it would switch every tick around the target. R-144 (`TECHNICAL_DEBTS.md`) moves the sim's comfort-target inject onto the same ceiling override and deletes it; whichever lands second keeps one precedence: a sim inject beats a request target |
| Per-asset tick inputs | `TickInputs::{ev_departure_by_asset, heater_emergency_by_asset}` and their `TickOverrides` twins; the scalar sim injects `ev_soc_target_override`, `heater_temp_max_override`; `EvCharger::apply_usage_sim_tick` (plugging follows the EV's own usage schedule at `now`) | Gains `ev_soc_target_by_asset` and `heater_temp_max_by_asset` in the same shape, filled only under opportunistic dispatch; a scalar sim inject, when set, still wins (D4). The forecaster's slot `now` drives plugging (D6) |
| Requests per asset | `AppState::current_ev_session(asset_id, now)`, `heater_targets()`, `shiftable_loads()` (W4) | Read only in the opportunistic arm (D4) |
| Base setpoints | `dispatcher::build_setpoints`; the no-plan base in `build_tick_setpoints` (defaults only, no thermostat setpoints) | `build_setpoints` gains `follow_allocations`, `true` for every planning objective; the no-plan base is untouched (D0, D5) |
| Limit pass | `limit::enforce_import_limit`, `hard_import_limit_kw`, `limit_target_kw` | Reused unchanged, live and in the forecast |
| Scheduled limit at t | `tightest_capacity_limit` (used in `setpoints.rs`) | Reused per forecast slot |
| Plan assembly | `results::translate_to_plan`; slot skeleton duplicated in `results::fallback_plan` | Forecast assembles through `translate_to_plan` (post-W1 `SolveOutput`); the skeleton copy is extracted into one function both call |
| Forecast inputs | `milp_planner::inputs::build_milp_inputs` | Reused for slot grid, PV, base load, tariffs, caps |
| Simulator step | `SimState::tick` | The forecaster's physics, at `dt_s` = slot length |
| Adoption gate | `services::planning::evaluate_acceptance_gate` | Gains one leading rule: a change of `dispatches_from_plan()` between the plan in force and the new plan, or a new opportunistic plan, always adopts; between two planning plans the gate is unchanged (D7) |
| Arbiter memory | `state/arbiter.rs` (`arbiter_active_lever`, `limit_active_lever`), residual tracker (`ArbiterOutcome::residual_kwh_by_asset` -> `apply_residual_escalation`) | The opportunistic pass writes none of it (D0) |
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
- Planning objectives decide exactly as before (D0).
- One opportunistic decision, used by live dispatch and the forecast.
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

### D0. Planning objectives are frozen
The first rule of this change: under every planning objective the VEN decides exactly as before.
The opportunistic code is reached through exactly two guards, both on
`objective.dispatches_from_plan()`: one in `build_tick_setpoints`, one in `tasks/planning/cycle.rs`.
Every planning-path function either stays byte-for-byte as it is, or changes only by gaining a
parameter whose planning value reproduces today's behaviour. The table is the review of every
touchpoint:

| Touchpoint | Planning path after this change | How it is proven |
|---|---|---|
| `build_tick_setpoints` | Same calls in the same order; the opportunistic branch is a new arm taken only when the adopted plan's objective is `opportunistic` | Tick golden (task 0.1) |
| No-plan window (startup, expired plan) | Unchanged: defaults-only base, then `reconcile`'s EV-only branch or `apply_surplus_ev_overlay` | Tick golden, no-plan cases |
| `apply_ev_lever_opportunistic`, `apply_surplus_ev_overlay` | Untouched | Existing tests, tick golden |
| `dispatcher::build_setpoints` | `follow_allocations = true` is today's body | Existing tests, tick golden |
| `rank_levers` | The `Cost` basis is today's sort, kept verbatim as its own branch | Existing tests, plus a randomized test against a frozen copy of today's function |
| `Lever`, `apply_ranked_levers` | New field `None`; two new match arms existing levers never reach | Tick golden |
| `reconcile`, deviation and limit levers, `LeverPolicy` constants | Untouched | Tick golden |
| Arbiter memory (active levers, residual tracker) | The opportunistic pass returns no `displaced_kw_by_asset` and writes its leading lever into its own diagnostic field, never into `arbiter_active_lever` or the residual tracker. The limit pass is the same pass under both regimes and keeps `limit_active_lever` across a switch exactly as it does across any plan change today (a shed stage stays shed until it fits) | Switch-back test (task 3.4) |
| Target injections (EV SoC target, heater ceiling) | Only in the opportunistic arm; a planning tick passes the sim inject values exactly as today | Tick golden |
| Active-request collection | Only in the opportunistic arm; a planning tick does no extra state reads | Tick golden, code review |
| Planning cycle | Same solve call; the guard is on the objective | `plans_golden.rs` |
| Adoption gate | One new leading rule that fires only when `dispatches_from_plan()` differs between the two plans or the new plan is opportunistic | Existing gate tests plus new ones |
| `PlannerObjective`, `SolveStatus`, `WarningKind` | New variants; no existing arm changes | Exhaustive matches, spelling tests |
| `ObjectiveSetting` | Same value and same trigger on `PUT /plan/objective`; the profile value at boot when no fleet command is retained | Route tests |
| `AssetSnapshot` | One additive field | Snapshot tests |
| `lab_core::units` | The same arithmetic, moved | Round-trip test, plans golden |
| Plan-assembly skeleton (7.1) | Pure extraction | Plans golden |
| Reports (`frequency: 0`) | No existing event uses `frequency: 0` (repo-wide grep); other frequencies unchanged | `report_timing` tests |
| Fleet MQTT publishes | Never awaited by a tick or a planning cycle (D10) | Unit test with a full queue |

**Accepted transient.** With deviation correction enabled, `reconcile` seeds battery and EV from
their last command. The first ticks after switching back start from the opportunistic pass's last
command, exactly as they start from the previous plan's command after any plan change today; the
dead-beat step and the release rule (R-88) bring them onto the new plan within a tick or two. This
is today's plan-change behaviour, not a new one, so nothing is reset.

**What changes in planning mode on purpose:** nothing in this change. R-144 (the sim's
comfort-target chatter) is a separate fix on its own branch.

**Load added in planning mode:** the VEN publishes its objective and plan on the lab broker
(non-blocking) and, once seeded, submits a `USAGE_FORECAST` report, throttled to once per 15
minutes unless the objective changed (D11). Neither runs inside the tick or the solve.

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

Both are new `LeverId` kinds, one lever per asset whose snapshot carries a surplus-claim rank,
in the same shape as W5a's per-asset levers. `lever_candidates` builds them only when the policy
admits them, so the planning passes (`ALL_LEVERS`, `DISPATCH_POLICY`) never build or see them,
and `apply_ranked_levers` gains two match arms for them. The loop is unchanged otherwise: the achieved amount is what the asset draws
**this tick** (`power_drawn_for_setpoint_kw`), so a lagging charger passes the excess to the
next lever, as `LeverEffect` does today. The pass starts every tick from the base setpoints
(storage and EVs at their default 0 kW, not their last command) and reads `projected_net_kw`
once, so it is memoryless: no integrator, no lag bridge, nothing to wind up. The direction is the
sign of the imbalance; each lever is capped at the remaining imbalance, so the battery never
charges from the grid or discharges into export.

`rank_levers` ranks by the policy's basis: `Cost` (today's passes; today's code kept verbatim as
that branch) or `DeclaredRank` (opportunistic), with the incumbent hysteresis kept for both.

`apply_ev_lever_opportunistic`, `apply_surplus_ev_overlay` and `reconcile`'s EV-only no-plan
branch stay exactly as they are (D0). They compute surplus differently (battery discharge
excluded, the battery's planned charge counted as a consumer), so routing them through
`LEVER_ABSORB` would change planned dispatch. Consolidating the two answers is filed as debt; its
first step is an equivalence test that shows where they differ. Heater pause, heater emergency
and PV curtailment are not opportunistic candidates. `build_tick_setpoints` calls
`arbiter::opportunistic_step` (pass + limit pass) only when the adopted plan's objective has
`!dispatches_from_plan()`, independent of the deviation toggle.

The per-kind `battery_lever`/`ev_lever` of the deviation and limit passes are instances of the
same generic shape plus pass-specific rules (dead-beat seeding, `MaxRevenue` refusal, lag
bridge). Folding them in would change planned dispatch and is out of scope (D0); filed in
`TECHNICAL_DEBTS.md`.

### D3. One surplus-claim rank per asset kind, read by planner and arbiter
The rank exists today only on the MILP side (`AssetMilpContext::surplus_claim_rank` in each
`*_declared.rs`). Each kind's value moves to one constant beside the kind (EV 0, heater 1,
shiftable load 2, battery 3); the declared context returns it, and a new
`Asset::surplus_claim_rank() -> Option<u8>` returns the same constant (`None` for PV and base
load, whose context keeps `u8::MAX`). `AssetSnapshot` carries it. `None` means the asset is never
offered surplus or asked to discharge. The planner's attribution and the live dispatch order are
then one declaration, and `plans_golden.rs` proves the planner side unchanged. Equal ranks keep
roster order, so two EVs or two heaters take surplus in roster order.

*Alternative rejected:* battery first (several commercial HEMS default to it). It would make the
dispatch order differ from the planner's attribution order and need a second declaration.
Battery last also gives the classic profile: deferrable loads take the noon surplus, the battery
takes the rest and covers the evening.

### D4. Request modes declare their non-planning behaviour
`UserRequestMode::non_planning_dispatch() -> NonPlanningDispatch::{StartNow, SurplusOnly}`:
`Asap`, `ByDeadline` → `StartNow`; `AsapFree`, `ByDeadlineFree`, `Opportunistic`, `MaxCost` →
`SurplusOnly`. `MaxCost` is surplus-only because without a price forecast the only energy known
to fit any budget is surplus. In the opportunistic arm only, the tick collects each asset's
active request (`current_ev_session(asset_id, now)` per EV, `heater_targets()`, active
`shiftable_loads()`) into `ArbiterTick.active_requests: Vec<(asset_id, NonPlanningDispatch)>`. The request's target reaches the asset through the path
each asset already has for a target, so the asset's own answer says when it is done — the
arbiter never compares a state with a target:

- EV: under opportunistic dispatch, `ev_soc_target_by_asset[id]` = the active session's target;
  the scalar `ev_soc_target_override` sim inject, when set, still wins. Its capability then
  reports zero import at the target.
- Heater: under opportunistic dispatch, `heater_temp_max_by_asset[id]` = `min(target_temp_c,
  temp_max_c)` of that heater's active start-now request; the scalar `heater_temp_max_override`
  sim inject, when set, still wins. The heater's
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
opportunistic arm passes `false`; every planning call passes `true`. The planning objectives'
no-plan base (defaults only, no thermostat setpoints) stays as it is: making it a call with
`false` would add thermostat setpoints to today's no-plan window (D0).

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

`milp_planner` gains `plan_from_forecast(inputs, asset_contexts, trace, ...)`: it builds a
`SolveOutput` (grid series from the trace, slack zero) whose `assets` come from each asset's own
context: a new `AssetMilpContext::solution_from_trace(net_kw, states)` returns that kind's
`AssetSolution` (planned state, storage legs, its `detail`), so no kind's readback is written
outside the kind. It then calls `translate_to_plan` with the same `asset_contexts`,
`SolveStatus::Simulated`, `friction_eur = 0` and the marginal values of D8. PV and base load
contribute their forecast series as they do in a solve. `tasks/planning/cycle.rs` holds the single guard: `dispatches_from_plan()`
→ solve, else forecast. Gate, adoption, events and history are shared.

*Alternative rejected:* a second `SolverPort` implementation. `SolveRequest` carries MILP
contexts, not a steppable roster, and `controller/` may not reach `simulator/`; the adapter
already holds the clone. *Alternative rejected:* re-implementing physics per asset with
`Asset::simulate_forward`. It steps one asset at a time, but the pass needs the whole site's
balance each slot; `SimState::tick` is the one site-level step.

### D7. Simulated plans always adopt
`SolveStatus::Simulated` (`solved()` true) is added. `evaluate_acceptance_gate` gains one rule,
checked first: if `dispatches_from_plan()` differs between the plan in force and the new plan, or
the new plan's objective is opportunistic, adopt. Without the first half, a failed first solve
after switching back would lose to the opportunistic plan in force (`(true, false) => reject`),
and the VEN would keep dispatching opportunistically under a planning objective. With it, the
failed plan is adopted and the VEN behaves as a planning VEN with a failed plan does today. Two
planning plans never reach the new rule. The VEN UI shows `SIMULATED` wherever it shows the
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
Every new VEN publish (`objective`, `plan`) uses `try_publish` from a spawned task, so it never
waits on the broker and never holds a planning cycle or a tick. (The existing telemetry publish
awaits `AsyncClient::publish`, which waits when rumqttc's 32-message request queue is full;
whether that can stall the tick while the broker is down is unverified. Recorded as debt with
needs-evidence, not changed here.)
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
obligations, submit on the first adoption after an objective change and otherwise at most once
per 15 minutes (the adoption path re-arms them as due from a spawned task, after the plan is in
force); for measurement obligations, the profile default cadence, logged. Once per 15 minutes
keeps the VTN's report table at about 4 reports per VEN per hour (each up to 288 intervals)
instead of 12. The BFF reads, per VEN, the newest
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
- [The fleet switch makes many VENs replan at once (each solve 18-60 s on a Node host), which can
  push solves into their time limit] -> The BFF publishes a fleet-wide switch one VEN at a time
  with a configurable spacing (default 10 s); a planning VEN's plan quality is then what it is
  after any other replan trigger.
- [Rolling back after opportunistic plans were stored] -> Older code cannot read
  `"objective": "opportunistic"` or `SIMULATED` in the plan history. Clear those rows (or switch
  every VEN back and wait out the history retention) before a rollback; noted in the migration
  plan.
- [Dispatch lands before the forecast within the branch] → Group 4 adopts a plan with no slots
  and a warning ("opportunistic, no forecast yet") so dispatch is testable on its own; group 7
  replaces it before merge.
- [The fleet forecast joins OpenADR reports (VTN `venName`) with MQTT objectives (topic
  `venName`)] → Both come from the VEN's one configured name; the BFF test pins the join, and a
  report from a VEN with no MQTT objective is counted with objective `unknown`, not dropped.
- [Changing `frequency: 0` handling alters how any event using it is reported] → No seeded event
  uses it today; the measurement case logs the applied cadence.

## Migration Plan

No data migration. `ObjectiveSetting` and `SolveStatus::Simulated` are additive on the wire
(TS types regenerated). Rollback: clear every fleet command (VTN UI "Release all", or
`mosquitto_pub -r -n` per topic), switch every VEN to a planning objective, delete stored plans
made under `opportunistic` from each VEN's plan history, then revert.

