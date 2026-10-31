---
title: Dispatcher
type: component
created: 2026-07-04
updated: 2026-10-31
synced_commit: 102f7fa
sources: [VEN/src/controller/dispatcher.rs, VEN/src/tasks/sim_tick/, VEN/src/controller/arbiter/stages.rs, VEN/src/controller/monitor.rs, docs/architecture/VEN_ARCHITECTURE.md]
tags: [dispatcher, realtime, ledger]
---

# Dispatcher

The fast half of the VEN's two-speed loop. The dispatcher itself is a small **pure-function module**
(`VEN/src/controller/dispatcher.rs`) that turns the plan into a *base* setpoint per asset; the 1-second tick
that drives it lives in `VEN/src/tasks/sim_tick/` (`tick.rs::tick_once`). Each tick resolves its context
(`context.rs`: weather/[[real-measurement-mqtt]] feeds, injects, plan, capacity schedule, windows), builds the
setpoints (`setpoints.rs::build_tick_setpoints`), ticks the [[simulator]] physics with them, then records the
arbiter outcome and publishes results (`publish.rs`).

## What `build_setpoints` decides

`build_setpoints(plan, sim, thermostat_setpoints_kw, now)`:

1. Seed every asset with its `default_setpoint_kw` from the snapshot.
2. Find the plan slot covering `now` (produced by the [[milp-planner]]) and overwrite the setpoint of each
   `AssetAllocation` in it. A first plan slot may start up to one Zone-A step in the past; the
   covering-slot lookup (`s.start <= now < s.end`) executes it immediately on adoption
   ([[three-tier-plan-grid]], first-slot convention).
3. For each thermostat asset **without** a plan allocation this slot, use the asset's own comfort-target
   setpoint (`thermostat_setpoints_kw`, computed by the asset).

`build_setpoints_without_plan` is the same rule with no slot, the base of the opportunistic objective. The
plan is the sole authority for what the dispatcher outputs; it knows no asset ids (R-128) and no asset kind.
PV generation limiting is not a setpoint: `resolve_pv_generation_limit_kw` (and, with comms-loss,
`controller::comms_loss::pv_generation_limit`) takes the tightest of plan, export capacity limit, arbiter
curtailment, manual inject and comms-loss limits and is applied to the inverter's generation limit directly,
tagged with the source that won (planned / unplanned / arbiter / manual) for the curtailment history
(sign convention: PV negative, [[sign-convention]]).

## What happens after the base

`build_tick_setpoints` (`tasks/sim_tick/setpoints.rs`) then runs the reactive layers on that base, in order:

- the pass that proposes a change: `controller::arbiter::reconcile` (deviation correction, when
  `deviation_arbiter_enabled`), else the pre-arbiter surplus-EV overlay (`apply_surplus_ev_overlay`, which only
  delegates to the arbiter's EV lever), or, under a non-planning objective, `opportunistic_step` with no plan
  allocation followed at all ([[opportunistic-objective]]);
- `dispatch_stages` (`controller/arbiter/stages.rs`), one function shared by the live tick and the
  opportunistic forecast: the `DISPATCH_SETPOINT` override (steers the storage levers so net site power hits
  the commanded kW, clamped to live capability; an alert window beats it — safety over instruction), the
  comms-loss clamp, and last the import-limit enforcement pass (GB-47), which therefore also bounds a
  dispatch setpoint above a hard limit while staying inside the comms-loss bounds, and holds a lagging lever
  where its command lands (R-151).

The plan keeps running underneath a dispatch window and resumes when it ends. Parsing of the windows:
[[openadr-interface]]; the reactive layer: [[deviation-arbiter]]. The battery dead-beat correction overlay
named in older docs is gone; battery correction is one of the arbiter's ranked levers.

## Ownership facts

- **Ledger accounting is the Monitor's, wired from the tick task**: `monitor::record_tick`, called through
  `AppState::account_tick` in `sim_tick/publish.rs`, accumulates per-asset energy/cost/CO₂ using the LOCF tariff
  at `now`, and attributes each asset's import cost to its active `UserRequest` (BL-39). Only importing assets
  accrue cost/CO₂ (export revenue is not credited). Closed monthly periods are archived
  (`GET /ledger?asset_id=`); the live ledger is in memory (docs/REQUIREMENTS.md §2.3 "Asset Ledger").
- A shiftable load has an ordinary physics asset that latches on the plan's first nonzero setpoint; `publish.rs`
  only notices when a still-open request's asset has left the snapshot and closes the request out
  (`complete_shiftable_and_announce`), which replans.
- The tick reads its clock once and passes `now` down (the architecture audit forbids `Utc::now()` in the inner
  rings); an asset that follows no setpoint publishes none (R-153).
