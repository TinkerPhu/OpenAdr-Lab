---
title: System Use Cases
type: use-case
created: 2026-07-04
updated: 2026-10-22
synced_commit: 650b626
sources: [docs/use-cases/SYSTEM-USE-CASES.md, tests/features/, docs/use-cases/SYSTEM-USE-CASE-MANUAL.md, docs/use-cases/COMFORT-PERSONAS-USE-CASE-MANUAL.md, docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md]
tags: [use-cases, dr, bdd]
---

# System Use Cases

The DR scenarios [[openadr-lab]] is built to handle, and where they are exercised.
Primary catalogue: `docs/use-cases/SYSTEM-USE-CASES.md`; the BDD features in
`tests/features/` are the executable versions ([[testing-strategy]]).

## Core DR use cases (catalogue 1–8)

| # | Use case | Typical signals | Lab mapping |
|---|---|---|---|
| 1 | Emergency load shed | kW limit, curtailment | `ALERT_GRID_EMERGENCY` / `ALERT_BLACK_START` clamp planned import to 0 over the window, `SIMPLE` levels cap it, and the arbiter enforces the limit at execution ([[openadr-interface]], [[deviation-arbiter]]) |
| 2 | Renewable export limitation | export capacity limit | `EXPORT_CAPACITY_LIMIT` → hard MILP constraint per slot; the surplus is rerouted into battery, EV and heater ([[tariffs-and-capacity]]) |
| 3 | Time-of-use / dynamic pricing | price per interval | `PRICE` events → [[milp-planner]] cost objective |
| 4 | Planned peak shaving | load/power caps | event lifecycle far → near → active; import limit as a time-aware schedule |
| 5 | EV charging management | pause, max power | session deadline and energy constraints, request modes ([[hems-planning]]) |
| 6 | Battery dispatch window | charge/discharge limits | `DISPATCH_SETPOINT` steers net site power with the storage levers while the plan runs underneath ([[deviation-arbiter]]) |
| 7 | Program enrollment / connectivity check | no-op payload | targeting and enrollment; a no-op event flows through the normal poll ([[openadr-security]]) |
| 8 | Event cancellation | event deleted | no cancel status in OpenADR 3: deletion is cancellation, detected when the event vanishes from the next poll ([[openadr-3]]) |

## Catalogue 9–14: observing and operating the system

These are operator- and reader-facing rather than grid-facing, and each names what to test:

| # | Use case | What the lab does |
|---|---|---|
| 9 | Reading a report this lab sent | Every report declares its payloads' units; `USAGE` is energy over a stated interval (kWh); a reader takes the unit from the report's own descriptors ([[wire-contract]], [[openadr-interface]]) |
| 10 | A VEN that was given nothing | A VEN with no scopes authenticates and sees an empty world; the VEN reads its own token's scopes and reports missing ones, refused objects and unreadable units on `/health` `wire_conformance` and as notifications |
| 11 | Watching the whole fleet right now | Live telemetry on the lab broker, held by the BFF, drawn on the Fleet page: one line per VEN plus the total, event reactions per VEN, and the signals each VEN was told. Telemetry is the live feed and never the record ([[fleet-monitoring]]) |
| 12 | Staggering a fleet's response | `intervalPeriod.randomizeStart`: each VEN moves a declared start by its own offset, for dispatch only, keeping it across polls and saying so in an Info notification; report cadence is not delayed |
| 13 | Editing the tariff and watching the fleet | The VTN UI Tariff page edits a tariff event (including `P9999Y` repeating days), publishes it as a higher-priority event and compares the fleet's planned import and export before and after ([[vtn-stack]]) |
| 14 | Comparing the fleet with and without planning | One switch, per VEN or for all, publishes a planner-objective command; the Fleet plan card compares the adopted plans before and after, with a release path back to each VEN's profile objective ([[opportunistic-objective]], [[ven-ui]]) |

## Executable coverage

73 feature files with about 380 scenarios. The use-case suites are `ven_uc_normal.feature`,
`ven_uc_edge_cases.feature`, `ven_uc_stress.feature` and `ven_uc_vtn_coordination.feature`;
catalogue items 9–14 are pinned by `ven_reporting_out.feature`, `ven_health.feature`,
`ven_wire_units.feature`, `fleet_telemetry.feature`, `ven_simple_levels.feature`,
`tariff_editor.feature`, `fleet_plan_feed.feature` and `fleet_objective_control.feature`, beside the
per-component features (planner, dispatcher, heater tank, user requests, comfort curve, request
modes, real measurement). Observation procedures for manual runs are
`docs/use-cases/SYSTEM-USE-CASE-MANUAL.md` (UC1–UC8),
`docs/use-cases/COMFORT-PERSONAS-USE-CASE-MANUAL.md` (comfort and personas) and
`docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` (the device-level HEMS cases).

The complementary spec-side view — use cases the OpenADR 3.1 spec *implies* a VEN should
handle, gap-checked against this code base — is in [[openadr-spec-use-cases]].
