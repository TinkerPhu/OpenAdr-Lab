---
title: OpenADR Interface (VEN)
type: component
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/architecture/VEN_ARCHITECTURE.md, VEN/src/vtn.rs, VEN/src/controller/]
tags: [openadr, ven, translation, polling]
---

# OpenADR Interface (VEN)

The **only** VEN component that knows OpenADR HTTP, OAuth, and event payload formats
(docs/architecture/VEN_ARCHITECTURE.md §2.1). It polls the VTN every 30 s and translates
both ways between spec JSON and the internal domain model. Transport lives in
`VEN/src/vtn.rs` behind `VtnPort` ([[ven-hexagonal-architecture]]).

## Inbound: event type → internal signal

| OpenADR EventType | Internal target |
|---|---|
| `PRICE` / `EXPORT_PRICE` | `OadrEventSnapshot.ImportPrice` / `.ExportPrice` |
| `GHG` | `OadrEventSnapshot.ImportCO2` |
| `IMPORT_/EXPORT_CAPACITY_LIMIT` | per-interval capacity limits |
| `*_CAPACITY_SUBSCRIPTION` / `*_RESERVATION` | `OadrCapacityState` fields (kW) |
| `ALERT_*` | `PlanTrigger::Alert` (grid-emergency handling: BL-04, not yet implemented) |
| `DISPATCH_SETPOINT` | direct [[dispatcher]] override, bypasses the planner |
| `CHARGE_STATE_SETPOINT` | creates/modifies `EvSession` (BL-06, not yet implemented) |

Changed tariff/capacity data emits `PlanTrigger::RATE_CHANGE`/`CAPACITY_CHANGE`, waking
the [[milp-planner]]. Event *removal* on a poll means cancellation ([[openadr-3]]).

## Outbound: report obligations

`USAGE` (time-weighted mean net site import), `DEMAND` (per-resource actual power),
`STORAGE_CHARGE_LEVEL` (SoC), `OPERATING_STATE`, `USAGE_FORECAST` (FIRM slots as points,
FLEXIBLE slots as `[0, MaxPower]` ranges), and import/export capacity reservations from
the flexibility envelopes (VEN_ARCHITECTURE.md §2.1).

The tariff/capacity values captured per poll tick form the `TariffSnapshot` described in
[[tariffs-and-capacity]].
