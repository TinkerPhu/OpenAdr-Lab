# Spec Delta

## Purpose

Defines how the VEN's real-time control (the deviation arbiter, its limit pass, the VTN dispatch
override and the comms-loss fail-safe) acts on every asset that declares a lever, whatever its id.

## ADDED Requirements

### Requirement: An asset declares its lever
Each asset SHALL declare which arbiter lever it offers, if any, and the arbiter SHALL build its
levers from those declarations, one per asset, never from a fixed asset id.

#### Scenario: A battery with another id is a lever
- **WHEN** the roster's battery is named `home-battery` and the site imports above plan
- **THEN** the arbiter discharges `home-battery` and reports `battery:home-battery`

### Requirement: Levers of the same kind are ranked like any other
The arbiter SHALL rank every lever by its marginal cost and SHALL break ties by roster order.

#### Scenario: Two batteries with equal cost
- **WHEN** two batteries can each absorb the deviation
- **THEN** the first in the roster absorbs it, and the second takes only what the first could not

### Requirement: Opportunistic charging per EV
Each EV SHALL be offered surplus only under its own settings and only while the plan allocates it
nothing. EVs SHALL be served in roster order.

#### Scenario: Two EVs share a surplus
- **WHEN** two plugged-in EVs have opportunistic charging on and the surplus exceeds what the
  first can take
- **THEN** the first charges at what it can take and the second takes the rest

#### Scenario: One EV's setting does not affect another
- **WHEN** opportunistic charging is off for the first EV and on for the second
- **THEN** only the second EV is offered the surplus

### Requirement: Per-asset emergency mode and departure
The heater emergency mode the arbiter chooses and an EV session's departure time SHALL reach only
the asset they belong to.

#### Scenario: Two heaters, one curtailed
- **WHEN** during an alert only the first heater's emergency heat is curtailed
- **THEN** the second heater keeps its thermostat behaviour

### Requirement: The dispatch override uses the ranked storage levers
A VTN dispatch window SHALL be met by the storage levers, ranked like the arbiter's, ignoring the
planner objective.

#### Scenario: Dispatch with a battery named otherwise
- **WHEN** a dispatch window is active and the roster's battery is named `home-battery`
- **THEN** `home-battery` is steered to meet the window's setpoint

### Requirement: The comms-loss fail-safe clamps every lever asset
On confirmed comms loss, the VEN SHALL clamp every asset that declares a lever to the configured
percentage of its own rated import and export.

#### Scenario: A second EV is clamped too
- **WHEN** comms loss is active and the roster holds two EVs
- **THEN** both EVs' setpoints are capped at the configured share of their own rated charge power
