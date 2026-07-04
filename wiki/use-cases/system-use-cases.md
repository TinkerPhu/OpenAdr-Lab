---
title: System Use Cases
type: use-case
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/use-cases/SYSTEM-USE-CASES.md, tests/features/, docs/use-cases/SYSTEM-USE-CASE-MANUAL.md]
tags: [use-cases, dr, bdd]
---

# System Use Cases

The DR scenarios [[openadr-lab]] is built to handle, and where they are exercised.
Primary catalogue: docs/use-cases/SYSTEM-USE-CASES.md; the BDD features in
`tests/features/` are the executable versions ([[testing-strategy]]).

## Core DR use cases (from the catalogue)

| # | Use case | Typical signals | Lab mapping |
|---|---|---|---|
| 1 | Emergency load shed | kW limit, curtailment | `ALERT_GRID_EMERGENCY` → planner shed (BL-04, pending — [[openadr-interface]]) |
| 2 | Renewable export limitation | export capacity limit | `EXPORT_CAPACITY_LIMIT` → MILP constraint ([[tariffs-and-capacity]]) |
| 3 | Time-of-use / dynamic pricing | price per interval | `PRICE` events → [[milp-planner]] cost objective |
| 4 | Planned peak shaving | load/power caps | event lifecycle far → near → active |
| 5 | EV charging management | pause, max power | `EvSession` deadline/energy constraints ([[hems-planning]]) |

(The catalogue continues beyond these five in the source document — battery dispatch,
capacity subscription scenarios, etc.)

## Executable coverage

BDD suites `ven_uc_normal.feature`, `ven_uc_edge_cases.feature`, `ven_uc_stress.feature`,
`ven_uc_vtn_coordination.feature`, plus per-component features (planner, dispatcher,
heater tank, user requests, reports) — ~49 scenarios total. Observation procedures for
manual runs: docs/use-cases/SYSTEM-USE-CASE-MANUAL.md.

The complementary spec-side view — use cases the OpenADR 3.1 spec *implies* a VEN should
handle, gap-checked against this code base — is in [[openadr-spec-use-cases]].
