---
title: Dispatcher
type: component
created: 2026-07-04
updated: 2026-07-05
synced_commit: e138861
sources: [VEN/src/controller/dispatcher.rs, VEN/src/tasks/sim_tick/, VEN/src/controller/monitor.rs, docs/architecture/VEN_ARCHITECTURE.md]
tags: [dispatcher, realtime, ledger]
---

# Dispatcher

The fast half of the VEN's two-speed loop. The dispatcher itself is a **pure-function
module** (`VEN/src/controller/dispatcher.rs`); the 1-second tick that drives it lives in
`VEN/src/tasks/sim_tick/` (`tick.rs::tick_once`), which snapshots plan/capacity/tariffs,
calls the dispatcher, ticks the [[simulator]] physics, then publishes results.

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
   (`EvSettings.opportunistic_charging_enabled`, `tasks/sim_tick/tick.rs:44`).

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

> **DRIFT** `docs/architecture/VEN_ARCHITECTURE.md` §2.1 describes the dispatcher as
> distributing `NetDeviation` across "auto-follow assets" and accumulating the ledger
> itself. Neither exists: there is no auto-follow concept, and the battery deviation
> correction (`apply_battery_correction_overlay`, a dead-beat P-controller on grid
> deviation) is `#[allow(dead_code)]` at `dispatcher.rs:188` — implemented and
> unit-tested but deliberately **not wired** into `build_setpoints`. The only live
> reactive layer is the surplus-EV overlay. See [[ven-code-vs-docs-audit]].
