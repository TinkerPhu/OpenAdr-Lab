---
title: Dispatcher
type: component
created: 2026-07-04
updated: 2026-08-09
synced_commit: 329444a
sources: [VEN/src/controller/dispatcher.rs, VEN/src/tasks/sim_tick/, VEN/src/controller/monitor.rs, docs/architecture/VEN_ARCHITECTURE.md]
tags: [dispatcher, realtime, ledger]
---

# Dispatcher

The fast half of the VEN's two-speed loop. The dispatcher itself is a **pure-function
module** (`VEN/src/controller/dispatcher.rs`); the 1-second tick that drives it lives in
`VEN/src/tasks/sim_tick/` (`tick.rs::tick_once`), which snapshots plan/capacity/tariffs,
calls the dispatcher, ticks the [[simulator]] physics, then publishes results. `tick_once`'s
pre-lock snapshot phase (weather/[[real-measurement-mqtt]] resolution, inject/plan/capacity/
dispatch/tariff reads, arbiter gates) and its post-lock housekeeping (inject-field clearing,
periodic report/persist counters) were split into `tasks/sim_tick/context.rs` and
`tasks/sim_tick/post_lock.rs` respectively (file-size cap) when the two new measured-kw
parameters were threaded through.

Per tick, `build_setpoints(plan, sim, capacity, heater_setpoint_c, now, overlay_enabled)`:

1. Seed every asset with its `default_setpoint_kw` from the snapshot.
2. Find the plan slot covering `now` (produced by the [[milp-planner]]) and overwrite
   setpoints for each `AssetAllocation` in it.
3. Heater override: when an injected `heater_setpoint_c` is set and the plan has no
   heater allocation, compute a thermostat ON/OFF setpoint.
4. Cap PV at the export capacity limit (sign convention: PV negative, [[sign-convention]]).
5. Apply the **opportunistic surplus-EV overlay** (`apply_surplus_ev_overlay`): when no
   plan-level EV allocation exists and the overlay is enabled, live PV surplus (after
   all other loads *and* any planned battery charging) is routed to the EV up to
   `max_charge_kw`. Auto-paused while an `EvSession` is active
   (`EvSettings.opportunistic_charging_enabled`, `tasks/sim_tick/tick.rs:44`). **Only
   runs when [[deviation-arbiter]]'s `deviation_arbiter_enabled` is false** — the
   overlay call is pinned to the disabled-arbiter rollout-gate path
   (`dispatcher.rs:217-219`); when the arbiter is enabled, `controller::arbiter::reconcile`
   owns the EV-surplus decision instead, as one of its ranked levers, called right after
   `build_setpoints` in the tick loop.
6. Apply the **dispatch override** (Phase 3, WP3.4 — `apply_dispatch_override` in
   `tasks/sim_tick/helpers.rs`, composed in `build_tick_setpoints`): while a
   `DISPATCH_SETPOINT` window is active and no alert window is (alert wins — safety
   over instruction), the battery is set so net site power hits the commanded kW,
   clamped to live capability; non-finite sentinel setpoints (PV's `f64::MAX`
   default) fall back to live power. The plan keeps running underneath and resumes
   when the window ends. See [[openadr-interface]] for the parsing side.

## Ownership facts

- **Ledger accounting is the Monitor's, wired from the tick task**:
  `monitor::record_tick` (called in `sim_tick/publish.rs`) accumulates per-asset
  energy/cost/CO₂ using the LOCF tariff at `now`; only importing assets accrue cost/CO₂
  (export revenue is not credited). In-memory only, resets on restart
  (docs/REQUIREMENTS.md §2.3 "Asset Ledger").
- A first plan slot may start up to one Zone-A step in the past; the covering-slot
  lookup (`s.start <= now < s.end`) executes it immediately on adoption
  ([[three-tier-plan-grid]], first-slot convention).
- Shiftable loads have no physics asset: the tick task detects a plan allocation for
  them, starts a countdown `ShiftableLoadRuntime`, augments the sim snapshot so they
  appear in `GET /sim` and the ledger, and fires a replan when they complete
  (`sim_tick/publish.rs`).

There is no "auto-follow" concept and no `NetDeviation` distribution across assets driven from
inside `build_setpoints` itself — real-time deviation correction across all assets is now
[[deviation-arbiter]]'s job, running after `build_setpoints` on the same tick when enabled. The
battery deviation correction (`apply_battery_correction_overlay`, a dead-beat P-controller on grid
deviation) is implemented and unit-tested but deliberately **not wired** into `build_setpoints`
(`dispatcher.rs:188`) — kept intentionally rather than deleted; `docs/BACKLOG.md` BL-22 tracks
wiring it behind a profile flag.
