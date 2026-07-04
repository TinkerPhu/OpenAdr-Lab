---
title: Tariffs and Capacity State
type: concept
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/REQUIREMENTS.md, VEN/src/entities/tariff_snapshot.rs]
tags: [tariff, capacity, domain]
---

# Tariffs and Capacity State

How time-varying grid signals are captured inside the VEN
(docs/REQUIREMENTS.md §2.3).

## TariffSnapshot

A point-in-time capture of **all** time-varying OpenADR events at one poll tick: import
tariff, export tariff (€/kWh), CO₂ intensity, and capacity limits — unified in one row so
every field is valid at the same timestamp (temporal correlation). Price fields originate
from `PRICE`/`EXPORT_PRICE` events, capacity fields from `IMPORT_/EXPORT_CAPACITY_LIMIT`
(`VEN/src/entities/tariff_snapshot.rs`; inbound mapping in [[openadr-interface]]).

## Two kinds of capacity — don't conflate

- **Per-interval capacity limits** (in `OadrEventSnapshot`): hard kW caps per event
  interval, from `*_CAPACITY_LIMIT` events.
- **Capacity State** (`OadrCapacityState`): the contractual picture — subscribed import/
  export kW and reserved import/export kW, from `*_SUBSCRIPTION`/`*_RESERVATION` events
  (REQUIREMENTS.md §2.3).

Both bound the [[milp-planner]]'s feasible region; reservations also flow back out as
`IMPORT_/EXPORT_CAPACITY_RESERVATION` report payloads.

## Stale data

When the VTN is unreachable, tariff slots beyond the last known data follow the
configured `StaleRatePolicy` (`LAST_KNOWN`, `HEURISTIC_FORECAST`, `DEFER_TO_FLEXIBLE`,
`SAFE_AVERAGE`) rather than silently assuming zero ([[milp-planner]]).

Tariff (€/kWh) vs rate (€/h) terminology: see [[sign-convention]].
