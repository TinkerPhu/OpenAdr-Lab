---
title: OpenADR Interface (VEN)
type: component
created: 2026-07-04
updated: 2026-07-04
synced_commit: 5a9a304
sources: [docs/architecture/VEN_ARCHITECTURE.md, VEN/src/vtn.rs, VEN/src/controller/, VEN/src/entities/capacity.rs]
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
`STORAGE_CHARGE_LEVEL` (SoC), `OPERATING_STATE`, `TELEMETRY_STATUS`, and import/export
capacity reservations from the flexibility envelope (`VEN/src/controller/reporter.rs`).
These are the only payload types any code path actually builds.

> **DRIFT** `docs/architecture/VEN_ARCHITECTURE.md` §2.1 additionally lists
> `USAGE_FORECAST` (FIRM slots as point forecasts, FLEXIBLE slots as `[0, MaxPower]`
> ranges) as an outbound obligation — but no code path in `reporter.rs` builds this
> payload type; it appears nowhere outside a comment in `entities/capacity.rs`. The MILP
> planner already computes exactly this per-slot forecast internally
> (`planned_state_by_asset`, exposed to `/timeline` for the UI) — it's just never turned
> into a report. See [[openadr-spec-use-cases]] (§8.7/§8.8 Capability/Operational Forecast
> Reporting) for what the OpenADR spec expects here and the concrete gap (the VEN doesn't
> parse `reportDescriptor.historical` at all, so it can't distinguish a forecast request
> from a historical one).

The tariff/capacity values captured per poll tick form the `TariffSnapshot` described in
[[tariffs-and-capacity]].
