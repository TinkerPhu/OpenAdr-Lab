## ADDED Requirements

### Requirement: Resolved comfort curve reaches the session's MILP context
The system SHALL carry the `ComfortRate` curve resolved at session-creation time
(`effective_comfort_rates()`) through `UserRequest`, `EvSession`/`HeaterTarget`, and into
`EvMilpContext`/`HeaterMilpContext`, instead of discarding it in
`controller/user_request.rs::create_from_body`.

#### Scenario: Session created with a user-overridden curve carries it into the plan
- **WHEN** an EV or heater session is created for an asset with an active comfort-curve
  override (`services/comfort.rs::effective_comfort_rates` resolves to the override, not the
  default)
- **THEN** the resulting `EvSession`/`HeaterTarget` stores that resolved curve, and the next
  planning cycle's `EvMilpContext`/`HeaterMilpContext` for that asset is built from the same
  curve values

#### Scenario: Session created with no override falls back to the asset default unchanged
- **WHEN** an EV or heater session is created for an asset with no comfort-curve override set
- **THEN** the resulting session stores `cfg.default_comfort_rates()` exactly as
  `effective_comfort_rates()` already resolves it today, and planner behavior is unchanged
  from before this capability existed

### Requirement: Comfort curve shapes the MILP reward for reaching higher fill levels
The system SHALL derive the EV's `v_core_eur`/`v_extra_eur_kwh` reward coefficients and the
heater's full-tier reward coefficient from the session's resolved `ComfortRate` curve
(evaluated via linear interpolation at the fill levels the asset's existing MILP structure
already distinguishes — 0.0 and 1.0), rather than from global planner-wide constants.

#### Scenario: Two identical EV sessions with different curves produce different allocations
- **GIVEN** two EV charging sessions with identical plugged state, SoC, deadline, and tariff
  inputs, differing only in their resolved `ComfortRate` curve
- **WHEN** each is planned independently through the MILP solver
- **THEN** the two resulting allocations (charged energy per slot, or total extra/core energy)
  differ in a direction consistent with the higher-price curve valuing charging more

#### Scenario: Two identical heater sessions with different curves produce different allocations
- **GIVEN** two heater sessions with identical initial tank energy, target, deadline, and
  tariff inputs, differing only in their resolved `ComfortRate` curve's price at `fill=1.0`
- **WHEN** each is planned independently through the MILP solver
- **THEN** the session with the higher `fill=1.0` price allocates at least as much full-tier
  (`z_heat_full`) operation as the lower-price session, all else equal

#### Scenario: MustRun sessions remain feasible regardless of curve values
- **WHEN** a session has a hard energy requirement (`MustRun`) and an arbitrarily low-value
  comfort curve
- **THEN** the solve still satisfies the hard deadline/energy constraint — the curve only
  affects objective rewards, never feasibility

### Requirement: Curve-to-reward interpolation is well-defined at and beyond its breakpoints
The system SHALL provide a `value_at_fill` interpolation function over a `ComfortRate` curve
that returns the exact stored price at each breakpoint, linearly interpolates between two
breakpoints, and clamps for `fill` values outside the curve's stored range.

#### Scenario: Exact breakpoint lookup returns the stored price
- **WHEN** `value_at_fill` is queried at exactly a stored breakpoint's `fill` value
- **THEN** it returns that breakpoint's `max_marginal_price` exactly

#### Scenario: Mid-curve query interpolates linearly
- **WHEN** `value_at_fill` is queried at a `fill` strictly between two adjacent breakpoints
- **THEN** it returns the linearly interpolated price between those two breakpoints

#### Scenario: Out-of-range query clamps to the nearest breakpoint
- **WHEN** `value_at_fill` is queried at a `fill` below the lowest or above the highest stored
  breakpoint
- **THEN** it returns the nearest boundary breakpoint's price rather than extrapolating
