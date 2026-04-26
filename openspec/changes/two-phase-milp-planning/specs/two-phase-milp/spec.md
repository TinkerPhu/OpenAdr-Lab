## ADDED Requirements

### Requirement: Phase 1 minimises economic cost only
The planner SHALL solve Phase 1 with an objective containing only economic cost terms: grid import/export energy cost, battery wear (kWh-proportional), GHG cost, contractual violation penalties, and the BatEvCoexist cross-asset interaction. No switching, startup, ramp, or tier-preference penalties SHALL appear in the Phase 1 objective.

#### Scenario: Heater-only VEN produces cost-optimal schedule ignoring switching count
- **WHEN** a VEN with only a heater asset runs Phase 1
- **THEN** the returned `phase1_cost_eur` equals the minimum achievable energy cost for that horizon
- **AND** the heater schedule may contain any number of relay switches without penalty

#### Scenario: Battery wear penalised in Phase 1
- **WHEN** a VEN with a battery asset runs Phase 1
- **THEN** battery wear cost (`c_bat_wear_eur_kwh × cycled_kwh`) is included in the Phase 1 objective
- **AND** the solver limits unnecessary battery cycling as part of cost minimisation

#### Scenario: Battery startup not penalised in Phase 1
- **WHEN** a VEN with a battery asset runs Phase 1
- **THEN** battery charge/discharge mode transitions do not incur any cost in the Phase 1 objective
- **AND** the battery may freely switch between charging and discharging modes across slots

### Requirement: Phase 2 minimises operational friction within Phase 1 cost bound
The planner SHALL solve Phase 2 with the constraint `phase1_cost_expression ≤ C* + ε` (where `C*` is the Phase 1 optimal cost and `ε = phase2_epsilon_eur`) and an objective minimising all switching, startup, ramp, and tier-preference penalties across all present assets.

#### Scenario: Phase 2 cost constraint is enforced
- **WHEN** Phase 2 completes successfully
- **THEN** the energy cost of the Phase 2 schedule does not exceed `C* + phase2_epsilon_eur`

#### Scenario: Heater relay switches minimised in Phase 2
- **WHEN** two heater schedules achieve the same Phase 1 cost, one with 2 relay switches and one with 4
- **THEN** Phase 2 selects the schedule with 2 relay switches

#### Scenario: Battery startup transitions minimised in Phase 2
- **WHEN** two battery schedules achieve the same Phase 1 cost with different charge/discharge transition counts
- **THEN** Phase 2 selects the schedule with fewer transitions

#### Scenario: EV charging run starts minimised in Phase 2
- **WHEN** two EV schedules achieve the same Phase 1 cost, one starting charging once and one starting twice
- **THEN** Phase 2 selects the schedule that starts charging once

#### Scenario: VEN with no controllable assets completes without error
- **WHEN** a VEN has no heater, battery, or EV (base load and PV only)
- **THEN** Phase 2 objective evaluates to 0.0 and the plan is adopted without error

### Requirement: Two-phase solve is asset-agnostic
The two-phase solver SHALL produce correct results for any combination of present assets (heater, battery, EV, shiftable loads) without special-casing absent assets. Each phase's objective and constraint terms for a missing asset SHALL evaluate to zero.

#### Scenario: Heater-only VEN runs both phases without battery or EV terms
- **WHEN** a VEN profile contains only a heater asset
- **THEN** Phase 1 contains no battery or EV objective terms
- **AND** Phase 2 contains only heater switching terms in its objective

#### Scenario: Full asset VEN runs both phases with all terms
- **WHEN** a VEN profile contains heater, battery, and EV assets
- **THEN** Phase 1 includes battery wear and BatEvCoexist terms
- **AND** Phase 2 includes heater switching, battery startup/ramp, and EV startup/ramp terms

### Requirement: Phase 2 fallback on infeasibility
If Phase 2 returns an infeasible or error result, the planner SHALL log a warning and return the Phase 1 solution as the adopted plan. The system SHALL NOT crash or propagate the error to the caller.

#### Scenario: Phase 2 infeasibility returns Phase 1 plan
- **WHEN** Phase 2 solver returns an infeasibility error
- **THEN** `run_planner` returns the Phase 1 schedule as the plan
- **AND** a warning is logged containing `"phase2 infeasible, falling back to phase1"`

### Requirement: `objective_eur` on Plan reflects Phase 1 cost
The `Plan.objective_eur` field SHALL contain the Phase 1 economic cost (pure EUR). The Phase 2 friction objective value SHALL be stored in a separate `Plan.friction_eur` field. The `plan_adoption_threshold_eur` comparison SHALL use `objective_eur` (Phase 1 cost) exclusively.

#### Scenario: Plan objective_eur is comparable across consecutive planning cycles
- **WHEN** two consecutive periodic planning cycles produce plans with nearly identical initial conditions
- **THEN** the difference in `objective_eur` between the two plans reflects only genuine economic improvement, not switching schedule variation

#### Scenario: Adoption threshold suppresses economically equivalent replans
- **WHEN** `plan_adoption_threshold_eur` is set to 0.20 and a periodic replan produces a plan with `objective_eur` less than 0.20 EUR below the current plan
- **THEN** the new plan is rejected and the current plan remains active

### Requirement: Adoption threshold decays linearly with plan age
`PlannerConfig` SHALL include a `plan_adoption_decay_s: f64` field (default `0.0` = no decay). When `plan_adoption_decay_s > 0`, the effective threshold at the periodic-replan adoption gate SHALL be `plan_adoption_threshold_eur × clamp(1.0 − elapsed_s / plan_adoption_decay_s, 0.0, 1.0)` where `elapsed_s` is the number of seconds since `current_plan.created_at`. Once `elapsed_s ≥ plan_adoption_decay_s`, the effective threshold is 0.0 and any new plan SHALL be adopted.

#### Scenario: Threshold decays to zero after decay_s seconds
- **WHEN** the current plan is older than `plan_adoption_decay_s` seconds
- **AND** a periodic replan produces any valid plan
- **THEN** the new plan is adopted regardless of the improvement in `objective_eur`

#### Scenario: Threshold is at full strength immediately after adoption
- **WHEN** a new plan was just adopted (elapsed_s ≈ 0)
- **AND** `plan_adoption_threshold_eur` is 0.20 EUR
- **THEN** the effective threshold is 0.20 EUR (full strength)

#### Scenario: No decay when plan_adoption_decay_s is zero
- **WHEN** `plan_adoption_decay_s` is 0.0
- **THEN** the effective threshold equals `plan_adoption_threshold_eur` regardless of plan age

### Requirement: `phase2_epsilon_eur` profile field controls cost tolerance
`PlannerConfig` SHALL include a `phase2_epsilon_eur` field (default 0.02 EUR). Setting it to 0.0 SHALL disable Phase 2 entirely and restore single-pass behaviour using only Phase 1 weights.

#### Scenario: Default epsilon prevents Phase 2 infeasibility from numerical precision
- **WHEN** Phase 1 terminates at the MIP gap boundary and `C*` is slightly optimistic
- **THEN** the default epsilon of 0.02 EUR provides sufficient slack for Phase 2 to find a feasible solution

#### Scenario: Zero epsilon disables Phase 2
- **WHEN** `phase2_epsilon_eur` is set to 0.0 in the profile
- **THEN** only Phase 1 is executed and the plan is built from the Phase 1 solution directly

### Requirement: Initial heater mode is pinned at planning slot 0
The MILP SHALL constrain `z_heat_mid[0]` and `z_heat_full[0]` to match the actual live heater power state at the time of planning. The switch variable `sw[0]` SHALL reflect any transition from the pinned initial mode to the mode at slot 1.

#### Scenario: Heater on at planning time forces z_heat_full[0] = 1
- **WHEN** the live heater is running at full power when planning starts
- **THEN** `z_heat_full[0]` is fixed to 1 in both phases
- **AND** a switch from full to off at slot 1 incurs the relay switching penalty in Phase 2

#### Scenario: Heater off at planning time forces z_heat_mid[0] = z_heat_full[0] = 0
- **WHEN** the live heater is off when planning starts
- **THEN** both `z_heat_mid[0]` and `z_heat_full[0]` are fixed to 0 in both phases
