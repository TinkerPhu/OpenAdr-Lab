# Spec Delta

## Purpose

Defines how the VEN UI shows every asset of the roster, each under its own id.

## ADDED Requirements

### Requirement: One card per asset
The Devices page SHALL render one card per EV and one per heater, in roster order, titled with
the asset id, each showing that asset's own settings, usage schedule, capability and requests.

#### Scenario: Two EVs
- **WHEN** the roster holds the EVs `car-a` and `car-b`
- **THEN** the Devices page shows two EV cards titled `car-a` and `car-b`, each with its own
  opportunistic-charging switch

### Requirement: A card requests for its own asset
A request created from an asset's card SHALL name that asset.

#### Scenario: A session from the second EV's card
- **WHEN** the user creates a charge session on the card of `car-b`
- **THEN** the request is sent with `asset_id` `car-b`

### Requirement: The Controller shows every asset
The Controller page SHALL show one summary cell per asset of the roster, each reading its own
snapshot.

#### Scenario: Two batteries
- **WHEN** the roster holds the batteries `bat-a` and `bat-b`
- **THEN** the Controller shows a cell for each with its own power and state of charge
