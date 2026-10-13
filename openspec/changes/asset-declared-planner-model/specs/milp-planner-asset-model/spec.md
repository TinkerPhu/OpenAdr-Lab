# Spec Delta

## Purpose

Defines how the VEN's MILP planner builds its optimisation model and its plan from what each
schedulable asset declares about itself. No asset kind is known by name outside that asset, so
any number of assets of any kind can take part in one plan.

## ADDED Requirements

### Requirement: Existing profiles plan identically
For every profile with at most one asset of each kind, the planner SHALL hand the solver the same
model, and SHALL produce the same plan, as before this capability existed. "Same model" means the
same variables, objective terms and constraints. "Same plan" means every slot field, allocation,
planned state, session-flexibility entry, warning, cost figure and marginal cost.

#### Scenario: Recorded scenarios are unchanged
- **WHEN** each recorded planner scenario is solved (each asset alone, all assets together, both
  cross-asset interactions, shiftable loads, a penalty rule, import and export slack, phase 2
  skipped)
- **THEN** phase 1, phase 2 and the marginal-cost pass each declare the same variables, objective
  terms and constraints as recorded (compared independently of term order within an expression)
- **AND** each solve returns the same solution as recorded
- **AND** the translated plan matches the recorded plan golden field by field

#### Scenario: Wire format unchanged
- **WHEN** a plan is served to the BFF or the VEN UI after this change
- **THEN** it carries the same fields with the same meaning as before, including the site-level
  battery charge and discharge, PV forecast and PV used, and the stored-energy trajectory

### Requirement: Several assets of one kind can be planned together
Given several schedulable assets of the same kind with distinct ids, the planner SHALL plan each
one with its own variables, constraints, costs and results. No asset's parameters or results may
overwrite another's. This applies to the planner alone: profile validation still limits a VEN to
one asset per kind until that limit is deliberately lifted.

#### Scenario: Two batteries are planned independently
- **WHEN** the planner is given two battery contexts with distinct ids and different capacities
  and states of charge
- **THEN** the plan has a separate allocation and a separate planned state of charge for each
  battery id
- **AND** each battery's planned energy stays within its own capacity bounds

#### Scenario: Two EVs keep their own obligations
- **WHEN** the planner is given two EV contexts with distinct ids, each with its own departure and
  firm target
- **THEN** each EV's allocation charges toward its own target
- **AND** a shortfall warning, if one is raised, names the EV that falls short

#### Scenario: Duplicate ids are rejected
- **WHEN** two contexts handed to the planner report the same asset id
- **THEN** a debug build fails its assertion, and the condition is reported as a planner
  contract violation rather than silently merged

### Requirement: Cross-asset interactions apply by role
A cross-asset interaction SHALL apply to every combination of assets that offers the roles it
couples, whatever their kinds or count. A role-based interaction SHALL NOT apply when no asset
offers a role it needs.

#### Scenario: Discharge–charge coexistence couples every pair
- **WHEN** the coexistence penalty is enabled, and the site has one asset that can discharge and
  one load with an on/off indicator
- **THEN** the penalty applies to that pair exactly as it did to the battery–EV pair before

#### Scenario: No pair, no interaction
- **WHEN** the coexistence penalty is enabled but no asset offers a discharge leg
- **THEN** the interaction adds no variables, constraints or cost

### Requirement: Results come from the asset that produced them
Everything per asset in a plan SHALL be derived from what that asset declares about its own
solution: its allocation power and pricing basis, its planned state, its session-flexibility
window, and its warnings. The order in which assets claim PV surplus SHALL be declared by each
asset. Ties SHALL be broken by planner input order.

#### Scenario: Surplus is claimed in declared order
- **WHEN** PV surplus is smaller than the combined planned consumption of the EV, the heater, a
  shiftable load and the battery in a slot
- **THEN** the surplus is credited first to the EV, then the heater, then shiftable loads, then the
  battery, as before

#### Scenario: Session flexibility is computed by one rule
- **WHEN** an EV session, a heater target and a shiftable load are active at once
- **THEN** each one's session-flexibility entry is computed by the same rule from that asset's
  declared window, energy and power range
- **AND** each entry equals what was produced before this change

#### Scenario: A fallback plan still reports session flexibility
- **WHEN** the solver fails and a fallback plan is returned
- **THEN** each asset's session-flexibility entry falls back to energy × average tariff over its
  window, as before
