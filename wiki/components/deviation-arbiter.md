---
title: Deviation Arbiter
type: component
created: 2026-07-28
updated: 2026-10-21
synced_commit: 97a6abc
sources: [VEN/src/controller/arbiter.rs, VEN/src/controller/arbiter/, VEN/src/entities/arbiter_residual.rs, VEN/src/tasks/sim_tick/arbiter_glue.rs, VEN/src/tasks/sim_tick/setpoints.rs, VEN/src/tasks/sim_tick/tick.rs, VEN/src/state/arbiter.rs, VEN/src/routes/hems/arbiter.rs, VEN/ui/src/components/devices/ArbiterSettingsCard.tsx, docs/architecture/VEN_ARCHITECTURE.md, docs/architecture/ven_milp_planner.md, docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md, docs/reference/KEY_LEARNINGS.md, docs/history/project_journal.md]
tags: [arbiter, deviation, real-time, marginal-cost, ven]
---

# Deviation Arbiter

The single owner of every reactive (non-plan, non-VTN-override) actuator adjustment: it reconciles
the [[milp-planner]]'s plan against what the [[simulator]] actually measures each tick, and keeps a
hard import limit. Two passes run through **one shared lever machinery**
(`controller/arbiter/ranked.rs::apply_ranked_levers`): candidate levers, ranked by current marginal
€/kWh with preemption-margin hysteresis, applied greedily cheapest first, each consuming what it
actually achieved and respecting its remaining capacity (an EV at target SoC or a battery at its SoC
limit contributes zero, not merely low priority). What differs between the passes is data
(`LeverPolicy`: `DEVIATION_POLICY`, `DISPATCH_POLICY`, the limit pass's own), never a second code path
(`docs/architecture/VEN_ARCHITECTURE.md` §2.1, "Arbiter").

- **Deviation correction** (`reconcile`, `deviation_arbiter_enabled`, default off): corrects the
  difference between a live projection of net site power and the plan's signed net.
- **Limit enforcement** (`limit::enforce_import_limit`, `limit_enforcement_enabled`, default on): the
  second pass (GB-47). It is memoryless and recomputed each tick from the tick's setpoint map, and it may
  cut a planned EV allocation or discharge the battery whatever the objective, so a timed-out MILP
  incumbent that put a heater stage inside a cap, or unforecast load, is met from the next tick while the
  planner catches up. Excess nothing can shed (forced power, the heater's comfort floor) is reported as
  `unresolved_kw`, never hidden.

A lever belongs to the asset that declares it (`Asset::arbiter_lever`,
`entities/asset.rs::ArbiterLeverKind`: `Storage`, `SurplusCharge`, `PausableLoad`), not to a fixed asset
id: two batteries are two levers, ranked like any others, with a cost tie broken by lever-kind
precedence and then roster order (`controller/arbiter/ranked.rs`). Diagnostics name a lever
`<kind>:<asset_id>`, EVs each have their own overlay, and the [[simulator]] receives the heater
emergency mode and the EV departure per asset. PV and base load are not levers; the arbiter counts their
**previewed** power for this tick (`LiveKwByAsset`, `SimState::peek_pv_kw`/`peek_base_load_kw`), never a
one-tick-stale snapshot. See [[asset-layer]].

## Tick order

`tasks/sim_tick/setpoints.rs::build_tick_setpoints`: the plan's base setpoints
(`dispatcher::build_setpoints`) → deviation correction (`reconcile`, when enabled; otherwise the
pre-arbiter `dispatcher::apply_surplus_ev_overlay`) → `dispatch_stages` (`controller/arbiter/stages.rs`):
a VTN DISPATCH_SETPOINT window steers the storage levers to its setpoint, then the comms-loss clamp, then
the limit pass last. So a hard import limit also bounds a dispatch setpoint above it, while staying inside
the comms-loss per-asset bounds. Both passes steer to `min(plan signed net, hard limit − LIMIT_MARGIN_KW)`:
the hard limit is 0 during an alert window, else the VTN import limit in force at `now` from the
time-aware capacity schedule ([[tariffs-and-capacity]]), else a sim-injected limit. While a switching
lever (a heater stage, the EV's minimum-charge floor) led the limit pass last tick its target sits
`LIMIT_RELEASE_HYSTERESIS_KW` lower, so a shed stage is restored only with room to spare.

Under the opportunistic objective the arm is different: no plan allocation is followed and
`opportunistic_step` decides from the tick's balance, then goes through the same `dispatch_stages`
([[opportunistic-objective]]).

## Why a single arbiter, not another reactive loop

A real-time correction layer (feature 017, `absorber.rs`, `PlanTrigger::DeviceDeviation`) was
built and removed twice for oscillating against the pre-existing opportunistic EV-surplus
overlay — two independent writers reacting to a one-tick-stale snapshot, with no arbitration
order between them (`docs/reference/KEY_LEARNINGS.md`'s Deviation Absorber section). The arbiter
is not a revival of that mechanism: it **subsumes** the overlay (folded into `arbiter_levers.rs`
rather than left as a second loop) and reads the current tick's actual PV/base-load, not a lagged
one — the two specific failure modes that killed feature 017. `LEVER_PREEMPTION_MARGIN_EUR_PER_KWH`
adds hysteresis so a lever near a cost tie doesn't chatter between ticks.

## The carried command is the baseline

Battery and EV deviation is projected from `AssetSnapshot.setpoint_kw` — the arbiter's own last-applied
command, the same field `apply_battery_lever`/`apply_ev_lever` use as their integrator state — and
`reconcile`'s `setpoints` baseline is seeded from it, not from the plan's static allocation
(`projected_net_kw`, `controller/arbiter/live.rs`). Reading the plan instead would hide a correction
applied on tick N from tick N+1's deviation calc, so it would either stack a fresh correction on the last
one (runaway) or be silently reverted. The regression test
`reconcile_battery_converges_under_stationary_disturbance_not_runaway_to_clamp`
(`controller/tests/arbiter_tests.rs`) drives the real `reconcile` entry point, because a convergence test
that bypasses it misses this (`docs/history/project_journal.md`, "Deviation arbiter battery/EV runaway
fix").

## A held correction ends by one rule (R-88)

Because battery and EV carry their last command forward, a correction is state the arbiter holds, and it
needs an explicit end (`controller/arbiter/release.rs`). Each tick it also computes the **deviation if
released**: battery and EV back at their plan values, counted at what those values draw once the command
has landed, and PV under the generation limit that applies without the arbiter's own tightening
(`ArbiterTick::live_released_kw`). Inside the dead band (`DEAD_BAND_KW`) the cause is gone and the whole
correction returns to plan in one tick; settled but still needed, it is *held* (nothing moves, the lever
stays reported active); otherwise the levers act and a shrinking disturbance is tracked down from the
carried value. PV curtailment is the arbiter's third held correction, carried through the inverter's
limit rather than a setpoint, and judged on the curtailed preview so it is not released and re-engaged
every few seconds. The VEN UI shows "Deviation if released" next to the deviation. Pinned by
`reconcile_releases_*`, `reconcile_keeps_*` and `a_held_correction_feeds_the_replan_backstop_as_energy`.

## A lagging lever is bridged, not corrected (R-82, R-104, R-151)

The EV charger applies a command a tick late (`response_delay_s`). The lever loop therefore hands what the
charger has not delivered yet to the next lever, usually the battery, for that tick, so import never
crosses a hard limit while a cut lands (`controller/arbiter/lag_bridge.rs`). The bridge is accounted per
pass and taken out of the carried battery command afterwards; otherwise the battery would read it as a
deviation of its own and the arbiter would undo the EV's landed change (the EV/battery hunt of R-104).
After the limit pass `limit::hold_where_commands_land`, in `stages::dispatch_stages` and so for every
objective, holds a lagging lever where its command lands, so a planning VEN does not release a cut EV
into a limit breach a tick later.

## Marginal-cost signal

The ranking isn't a hand-written priority table (cost vs. self-consumption vs. comfort vs. DR
obligation) — it's the plan's own shadow price. See [[milp-planner]]'s marginal-cost extension
(§5.2) for how `SolverPort` produces `marginal_cost_import/export_eur_per_kwh` per slot; a DR
obligation's breach penalty and a routine tariff both collapse into the same number, so an active
event doesn't need a special-cased rule to win — it just has a higher shadow price. §5.5 notes
this is a **within-tick tie-breaker, not a substitute for replanning**: greedy absorption doesn't
know a resource's later obligations, which is what the residual-based replan trigger below is for.

## Residual escalation → replan

Absorbed kWh on SoC-coupled assets (battery, EV) accumulates in `entities::arbiter_residual::AssetResidual`.
Once `breach_fraction` crosses `RESIDUAL_THRESHOLD_FRACTION` (0.2) past a 900 s cooldown,
`arbiter_glue::apply_residual_escalation` fires `PlanTrigger::ResidualThreshold` for a fresh MILP
replan — the mechanism feature 017 was evolving toward before its removal
(`docs/reference/KEY_LEARNINGS.md`'s Deviation Absorber section, "Residual vs. raw deviation for
Tier 2 triggers"): a replan recomputes future obligations against real SoC instead of trusting
stale marginal-cost numbers.

## Levers

| Lever | Mechanism | Capacity check |
|---|---|---|
| Battery | Charge/discharge adjustment within power limits | SoC bounds |
| EV | Adjust opportunistic/session charging | Session target SoC |
| Heater pause | Drop a stage while the comfort band allows it; levers command only exact stages (`AssetSnapshot.power_steps_kw`) | Zero-cost only when mid-cycle toward `temp_max_c`; none while the thermostat forces power |
| Heater emergency curtail/absorb | Drift below `temp_min_c` / heat above `temp_max_c` toward `temp_safety_max_c` | [[asset-layer]]'s `HeaterEmergencyMode`; curtail only during an alert, in either pass |
| PV curtailment | Tighten the generation limit (`PvCurtailmentSource::Arbiter`) below the plan's `pv_used_kw` | Only relieves an active export cap; last resort |

## Runtime toggles and diagnostics surface

`GET/PUT /arbiter-settings` (`routes/hems/arbiter.rs`) carries both toggles, each optional in the PUT body
so one switch changes without restating the other: `deviation_arbiter_enabled` (default `false` in every
profile) and `limit_enforcement_enabled` (default `true`, switched off only to measure the planner alone).
They are surfaced on the VEN UI Devices page's `ArbiterSettingsCard`. `GET /arbiter-diagnostics` (backed
by `AppState.arbiter_diagnostics`, updated every tick) reports both passes: projected net site power,
deviation, active lever, and the limit pass's target, excess, adjustments and unresolved excess, plus the
measured net power the tick came to; the card renders it while a pass is enabled (ui-transparency: no
reactive-lever decision without an inspectable surface). `ControllerEvent::ArbiterDecision` in
`GET /trace/events` records decisions, not ticks — emitted only when a pass's leading lever or unresolved
state changes (`controller/arbiter/decision.rs`). Observation walkthrough:
`docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` UC-15; the limit pass is pinned by
`tests/features/ven_import_limit_enforcement.feature`.

## Relationship to the dispatcher

The arbiter and [[dispatcher]]'s `apply_surplus_ev_overlay` overlap in intent (both react to live PV
surplus for EV charging) but are not both active: with `deviation_arbiter_enabled` the arbiter's EV
lever is the one live reactive layer for a tick; the dispatcher's overlay runs only when the deviation
pass is off (`setpoints.rs`), and the limit pass and `dispatch_stages` apply either way.
