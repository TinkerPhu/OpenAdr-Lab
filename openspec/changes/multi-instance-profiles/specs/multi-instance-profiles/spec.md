# Spec Delta

## Purpose

Defines that a VEN profile may declare several assets of one kind, each under its own id.

## ADDED Requirements

### Requirement: Several assets of one kind
Profile validation SHALL accept several assets of one kind and SHALL refuse an asset id used by
more than one asset.

#### Scenario: Two EVs and two arrays
- **WHEN** a profile declares the EVs `car-a` and `car-b` and the arrays `pv-east` and `pv-west`
- **THEN** it validates and the VEN lists, plans and forecasts each under its own id

#### Scenario: An id used twice
- **WHEN** two EVs omit their ids (both default to `ev`)
- **THEN** validation fails naming the id `ev` once
