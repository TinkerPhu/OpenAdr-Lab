# Spec Delta

## Purpose

Defines how the VEN keeps a user's EV session, heater target and EV settings with the asset the
request named, so each asset plans and is controlled with its own.

## ADDED Requirements

### Requirement: A session belongs to the asset its request named
An EV session and a heater target SHALL carry the `asset_id` of the request that created it, and
the VEN SHALL store them per asset. An EV session SHALL conflict only with sessions of the same
EV.

#### Scenario: Sessions of two EVs do not conflict
- **WHEN** two EV sessions with overlapping windows are queued for two different EVs
- **THEN** both are queued, each in its own EV's queue

#### Scenario: Cancelling a request removes its session from its own asset
- **WHEN** a request for an EV is cancelled
- **THEN** its session leaves that EV's queue and no other queue changes

### Requirement: Each asset plans with its own sessions
The planner SHALL hand an EV context only the sessions of that EV and a heater context only the
target of that heater.

#### Scenario: A second EV does not see the first EV's session
- **WHEN** the roster holds two EVs and one session is queued for the first
- **THEN** the first EV's context has that session and the second EV's context has none

### Requirement: Simulated usage is installed per EV
Every EV that predicts a usage schedule SHALL have its predicted sessions installed into its own
queue, whatever its id.

#### Scenario: An EV with another id gets its simulated sessions
- **WHEN** the EV is named `car` and predicts a usage schedule
- **THEN** its predicted sessions appear in the queue for `car`

### Requirement: EV settings per EV
The VEN SHALL serve and accept EV settings per EV at `/assets/{asset_id}/ev-settings` and SHALL
refuse an asset that is not an EV. The VEN UI's EV card SHALL use its own asset's settings.

#### Scenario: Reading and changing one EV's settings
- **WHEN** a client turns opportunistic charging off at `PUT /assets/ev/ev-settings`
- **THEN** `GET /assets/ev/ev-settings` reports it off

#### Scenario: Settings of an asset that is not an EV
- **WHEN** a client requests `/assets/heater/ev-settings`
- **THEN** the VEN answers 404 naming the asset
