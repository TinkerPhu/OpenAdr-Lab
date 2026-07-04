---
title: Dispatcher
type: component
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [VEN/src/controller/dispatcher.rs, docs/architecture/VEN_ARCHITECTURE.md]
tags: [dispatcher, realtime, ledger]
---

# Dispatcher

The fast half of the VEN's two-speed loop: a **1-second tick** that turns the current
`PlanTimeSlot` into live device setpoints (docs/architecture/VEN_ARCHITECTURE.md §2.1).

Per tick (`VEN/src/controller/dispatcher.rs`):

1. Read the current `PlanTimeSlot` from the active Plan (produced by the [[milp-planner]]).
2. For each `AssetAllocation`: compute a `DispatchCommand` for the target asset.
3. For auto-follow assets: distribute `NetDeviation = Σ(ActualPower) − Σ(PlannedPower)`
   across them.
4. Write commands to the [[simulator]].
5. Accumulate cost/CO₂ in the asset ledger.

## Notable ownership facts

- **Deviation handling lives here, not in the Monitor**: 
  `apply_battery_deviation_correction()` and `apply_ev_surplus_overlay()` are dispatcher
  functions; the Monitor only maintains the `AssetLedger` (VEN_ARCHITECTURE.md §2.1).
- The **asset ledger** (cumulative energy/cost/CO₂ per asset) is in-memory only and resets
  on restart — flagged in the glossary as a persistence gap
  (docs/REQUIREMENTS.md §2.3 "Asset Ledger").
- A first plan slot may start up to one Zone-A step in the past; the dispatcher treats it
  as the currently executing slot and applies its setpoints immediately on adoption
  ([[three-tier-plan-grid]], first-slot convention).
- `DISPATCH_SETPOINT` events from the VTN bypass the planner and override the dispatcher
  directly ([[openadr-interface]]).
