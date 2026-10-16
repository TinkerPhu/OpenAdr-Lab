# Spec Delta

## Purpose

Defines how an asset's type is identified, and how the SoC report covers several storage assets.

## ADDED Requirements

### Requirement: One closed asset kind
Every asset SHALL declare exactly one `AssetKind`, and no component SHALL derive an asset's kind
from its id or compare a kind as a free string.

#### Scenario: A kind whatever the id
- **WHEN** a profile declares an EV with id `car-a`
- **THEN** `/sim` lists it with `asset_type` `ev`, and every type-based view treats it as an EV

#### Scenario: An unknown kind does not compile
- **WHEN** UI code compares `asset_type` with a value that is not an `AssetKind`
- **THEN** the TypeScript build fails

### Requirement: The SoC report follows the request's aggregation
A `STORAGE_CHARGE_LEVEL` report SHALL cover every asset that declares a state of charge, as one
resource per asset when the descriptor's `aggregate` is false and as one energy-weighted resource
when it is true.

#### Scenario: Not aggregated
- **WHEN** the VTN requests `STORAGE_CHARGE_LEVEL` with `aggregate: false` from a VEN with EVs
  `car-a`, `car-b` and a battery
- **THEN** the report has one resource per storage asset, each named `<ven>-<asset_id>`

#### Scenario: Aggregated
- **WHEN** the same request has `aggregate: true`
- **THEN** the report has one resource whose SoC is the stored energy of the connected storage
  assets divided by their capacity
