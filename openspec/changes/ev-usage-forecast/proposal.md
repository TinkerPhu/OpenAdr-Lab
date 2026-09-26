# Proposal

## Why

`ev-usage-simulation` gave the EV asset a realistic daily leave/return pattern, but the planner
only ever learns about it after the fact (with `engage_charge_planning` off, it reacts once
`plugged` has already flipped) or through a one-sided, single-deadline proxy (`engage_charge_planning`
on, via an auto-created `EvSession`) that can never anticipate the car returning, or express more
than one future departure in a multi-day plan horizon. The car may leave under-charged not because
the planner made a bad trade-off, but because it was never told a deadline existed at all — and
even when told, the plan can't reason past the departure it knows about.

## What Changes

- New EV profile class, `usage_forecast`, an alternative to `usage_sim` (an EV picks exactly one,
  never both). It reuses `usage_sim`'s exact daily-schedule fields (weekday/weekend leave/return
  times, jitter, leave probability, SoC drop, `engage_charge_planning`) and performs the identical
  physical simulation (forced unplug during the trip window, SoC drop at return) — the only
  difference is how that same schedule reaches the planner.
- The EV asset feeds its own predicted future availability directly into the MILP as genuine
  per-slot data (not a new decision variable — the existing per-slot charging variable is
  unchanged; what's new is the bound/coefficient data constraining it), the same shape PV and
  base-load forecasts already use.
- A new modeling primitive: an exogenous, decision-independent energy adjustment at a specific
  future slot, to represent the SoC drop at return — nothing today models a one-off energy jump
  unrelated to what the planner decided that slot (the existing `forced_power_kw` concept is a
  continuous rate produced by a decision, not a discrete jump).
- When `engage_charge_planning` is enabled under `usage_forecast`, the predicted next departure's
  deadline/target is populated directly inside the EV's own MILP context — no `EvSession`
  side-channel involved for this path, so it never competes with a real user/VTN session for the
  single session slot. A real session's target/deadline always takes priority over anything
  `usage_forecast` derives; the availability data itself is never overridden by anything, since it
  is asserted fact, not a preference.
- Reuses the existing MAX_COST insufficient-budget warning pattern for the new case where a
  predicted departure leaves too little available charging time to reach a target — best-effort
  delivery plus a surfaced warning, no hard rejection.
- `usage_sim` is unchanged and remains a valid, simpler alternative — nothing about it is
  deprecated or altered by this change.

**Explicit non-goals** (deferred, not omissions): a target/urgency computed for *every* visible
future departure within one plan horizon (this change targets only the single next one, matching
`usage_sim`'s existing `engage_charge_planning` semantics exactly); a real, learned/heuristic-based
forecast for non-simulated deployments (this change's forecast is deterministic ground-truth
disclosure from the same function that drives the physics, not probabilistic prediction).

## Capabilities

### New Capabilities
- `ev-usage-forecast`: an alternative EV usage-simulation profile class that feeds the EV's
  predicted future availability (and, optionally, a predicted-departure charging target) directly
  into the MILP planner as per-slot data, instead of reacting only after the fact or via a
  one-sided session-based deadline.

### Modified Capabilities
(none — `ev-usage-simulation`'s existing behavior, `EvSessionOrigin`, and `usage_sim_plan_ahead.rs`
are unchanged by this proposal; no existing spec exists for that capability to modify, since the
prior change waved its documentation into `docs/` and deleted its own openspec change directory
per this project's workflow, rather than leaving a surviving spec file.)

## Impact

- **Profile schema** (`VEN/src/profile/schema.rs`, `VEN/src/profile/defaults.rs`): new
  `EvUsageForecastConfig`/`EvUsageForecastDayConfig` (or a shared type — an implementation-time
  structuring decision, see design.md), `EvConfig.usage_forecast: Option<...>`, and validation
  that `usage_sim` and `usage_forecast` are never both set on the same EV.
- **Params mirror** (`VEN/src/entities/asset_params.rs`): analogous `EvUsageForecastParams`.
- **EV asset** (`VEN/src/assets/ev.rs`): new `usage_forecast` field, wired through `from_params`;
  reuses (does not duplicate) `is_away_at`/`apply_return_drop`/`daily_trip` from
  `VEN/src/assets/ev_schedule.rs` for the physical simulation.
- **MILP** (`VEN/src/assets/ev_milp.rs::EvMilpContext`, `VEN/src/controller/asset_milp_port.rs`,
  `VEN/src/controller/milp_planner/`): the core of this change — per-slot availability bounds, the
  new exogenous-energy-delta primitive, and the direct (non-`EvSession`) deadline/target path for
  `engage_charge_planning`. Whether the per-slot primitives are new `AssetMilpContext` trait
  methods (wider blast radius, one shared contract) or a shared helper function EV's own
  `milp_params`/`constraints` implementation calls (smaller blast radius, same reusability) is an
  open implementation decision — see design.md.
- **Notifications** (`VEN/src/services/notify.rs`): reuse of the existing MAX_COST-shortfall
  warning path for the new infeasible-predicted-deadline case.
- **Tests**: `controller/milp_planner/tests/basic.rs`'s existing `EvSession`-deadline tests
  (`ev_mask_plugged_with_session_deadline`, `ev_mask_unplugged_all_false`,
  `ev_mode_must_run_for_firm_deadline_session`, `ev_mode_may_run_for_soft_deadline_session`) must
  stay green unchanged; new tests cover the `usage_forecast` availability array, the exogenous
  drop, the real-session-always-wins-for-target precedence, and the infeasible-deadline warning.
