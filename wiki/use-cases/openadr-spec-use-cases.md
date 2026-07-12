---
title: OpenADR-Spec-Implied Use Cases — Gap Analysis
type: use-case
created: 2026-07-04
updated: 2026-07-11
synced_commit: c5a1d03
sources: [docs/openadr_3_1_specs/, docs/BACKLOG_OpenADR_Cert.md, docs/architecture/VEN_ARCHITECTURE.md, tests/features/, VEN/src/entities/capacity.rs]
tags: [use-cases, openadr, gap-analysis, spec]
---

# OpenADR-Spec-Implied Use Cases — Gap Analysis

What the OpenADR 3.1 spec *expects* a VEN-side system to do (User Guide §5 user stories,
§6 scenarios, §7 feature examples), and whether [[openadr-lab]] can do it today.
Requirement-level detail lives in `docs/BACKLOG_OpenADR_Cert.md`; the operational DR
catalogue is [[system-use-cases]]. Legend: ✅ supported · 🟡 partial · ❌ missing.

## Actor roles: BL (VTN-side) vs CL (VEN-side)

The spec splits responsibilities by actor (User Guide §5.1), and this lab only
implements one side of that split in depth:

| Actor | Role | User stories (§5.2–§5.4) | This lab |
|---|---|---|---|
| **Business Logic (BL)** | Client of the VTN acting for the energy/service provider | CRUD programs and events; request/read/delete reports; subscribe to report-creation callbacks | The VTN UI ([[vtn-stack]]) is a thin operator console over the BFF's `any-business` credential — it covers BL's CRUD needs but not autonomous BL logic (no automated program/event generation, no DR dispatch strategy on the utility side) |
| **Customer** | Person doing one-time VEN setup | Receive program description, VTN base URL, client ID, API credentials, resource IDs from BL (out-of-band) | Manual: VEN profiles are hand-authored YAML with `clientID`/`clientSecret` baked in, not delivered via a BL-driven enrollment flow |
| **Customer Logic (CL)** | Functional logic inside the VEN | Read programs/events, subscribe to change notifications, create/update reports | This is the lab's actual focus — [[openadr-interface]] + [[hems-planning]] implement the CL role fully (poll-based, not subscription-based; see the push-notification gap below) |

BL-side automation (the provider's actual DR program logic, forecasting, and dispatch
decisions) is out of scope for this lab — the VTN UI exists to drive events manually for
testing, not to model a utility's business logic.

## From the CL/VEN user stories (User Guide §5.4) and scenarios (§6)

| Spec-implied use case | Spec ref | Status | Lab reality / suggested feature |
|---|---|---|---|
| Discover programs, select the applicable one | §5.4, §6.4 | ✅ | VEN polls `GET /programs` at startup and every 300 s ([[openadr-interface]]) |
| Read events, respond within a program | §5.4, §6.5 | ✅ | 30 s poll → typed translation → `PlanTrigger` → [[milp-planner]] replan |
| **Push notification** on program/event/report change (webhook subscription) | §5.4, §6.3.1 | ❌ | Polling only. Feature: subscription object + webhook receiver endpoint; would cut the 30 s reaction latency |
| Notification via additional protocols (MQTT) | §6.3.2 | ❌ | No MQTT client (cert backlog §2). Feature: optional MQTT listener beside the poller |
| Create/update reports fulfilling report requests | §5.4, §6.6, §7.5 | 🟡 | `USAGE`, `SIMPLE`, `STORAGE_CHARGE_LEVEL`, `OPERATING_STATE`, capacity reservations are built (`reporter.rs`); `DEMAND`/`USAGE_FORECAST` are not; obligations now recur correctly (a `frequency` descriptor yields one report per interval, re-armed rather than one-shot — see [[openadr-interface]]); rolling/ad-hoc/`report-only` management (§7.5) not implemented ([[ven-code-vs-docs-audit]]) |
| Hourly prices + usage response | §6.6 | ✅ | The lab's core loop: `PRICE` events → MILP cost objective → usage reports ([[hems-planning]]) |
| Load shed on command | §6.6 | 🟡 | `ALERT_GRID_EMERGENCY`/`ALERT_BLACK_START` → planner shed is BL-04, not yet implemented; `SIMPLE` levels 0–3 unmapped ([[openadr-interface]]) |
| Device status reporting | §6.6 | 🟡 | `OPERATING_STATE` payload is sent but hardcoded to `"ACTIVE"` (`reporter.rs`); the `DeviceResponsiveness` enum it should derive from is unreferenced vocabulary |
| VEN + resource registration & management | §6.7 | 🟡 | venName/resources registered at startup; runtime credential/URL reconfiguration needs a container restart (cert backlog §1) |
| Enrollment / connectivity check events | §6.2 | ✅ | `enrollment.feature` covers targeting; no-op events flow through the normal poll path |
| Event priority between overlapping events | §7.1 | ❌ | No priority resolution — overlapping events are not arbitrated. Feature: honour `priority` when two active events target the same resource |
| Variable-duration intervals in one event | §7.4 | 🟡 | Interval parsing follows the wire format, but MILP tariff sampling assumes the plan-grid resolution ([[three-tier-plan-grid]]); irregular interval edges inside one Zone-A step blur |
| Baseline reporting for M&V | §7.5, `BASELINE` payload | ❌ | `BASELINE` is typed but no baseline computation exists — see the Baseline/Forecast distinction in [[demand-response]] |

## Formal Use Case catalogue (User Guide §8)

The spec's own named Use Cases, each a BL-authored program/event pattern the VEN must
recognise and respond to:

| Use Case | Spec ref | Status | Lab reality |
|---|---|---|---|
| Alert (grid emergency, non-financial) | §8.1 | 🟡 | `ALERT_*` typed but shed logic not implemented (BL-04) |
| Load Shed — CPP / direct load control | §8.2 | 🟡 | Same BL-04 gap; `SIMPLE` levels 0–3 unmapped |
| Day-Ahead Prices with Usage Report | §8.3 | ✅ | Core loop: `PRICE`/`GHG` → [[milp-planner]] → `USAGE` reports |
| Inverter Management | §8.4 | 🟡 | PV asset exists ([[asset-layer]]) but no inverter-specific dispatch (curtailment/power-factor setpoints) beyond capacity limits |
| Load Control | §8.5 | ❌ | `DISPATCH_SETPOINT` is not handled anywhere — it survives only as a field of the unused `OadrEventCache` struct (`entities/capacity.rs`); no parser, no dispatcher override path ([[ven-code-vs-docs-audit]]) |
| State of Charge Reporting | §8.6 | ✅ | `STORAGE_CHARGE_LEVEL` from battery/EV SoC |
| Capability Forecast Reporting | §8.7 | ❌ | `OadrReportObligation` (`entities/capacity.rs`) never parses `reportDescriptor.historical`, so the VEN can't distinguish a forecast request from a historical one; no `LOAD_SHED_DELTA_AVAILABLE`/`GENERATION_DELTA_AVAILABLE` payload exists. See [[openadr-interface]] |
| Operational Forecast Reporting | §8.8 | ❌ | Same root cause — the MILP already computes the per-slot forecast internally (`planned_state_by_asset`) but it's never turned into a report; see the DRIFT in [[openadr-interface]] |
| Capacity Management (Dynamic Operating Envelopes, Dynamic Capacity Mgmt) | §8.10 | 🟡 | `*_CAPACITY_LIMIT` → MILP contractual-limit constraints; `IMPORT_CAPACITY_SUBSCRIPTION`/`_RESERVATION` are parsed into `OadrCapacityState` (shown at `GET /capacity`) but never constrain the solver; export-side subscription/reservation payloads unhandled ([[tariffs-and-capacity]]) |
| Custom Dispatch Instructions | §8.12 | ❌ | No dispatch-instruction payload path exists (same `DISPATCH_SETPOINT` gap as §8.5) |
| Dynamic Targeting | §8.13 | ✅ | `enrollment.feature` exercises BL-granted targets; see [[openadr-security]] for the targeting/object-privacy mechanism and its 3.1 field-shape drift |

## Reading of the gaps

Three clusters emerge: **transport modernisation** (webhooks, MQTT, TLS — all cert
blockers, see [[vision-and-roadmap]]), **report management depth** (§7.5 machinery),
and **event arbitration** (priority, `SIMPLE` mapping, emergency shed). The first cluster
is infrastructure; the last two are planner/domain features that would fit the existing
`PlanTrigger` and obligation models.

The question of which cluster to tackle first was resolved by execution: Phase 3
("Control-Method Lab", 2026-07-11) took **event arbitration** — SIMPLE levels,
grid-emergency alerts, capacity reservations, and direct dispatch/charge setpoints
all now constrain or steer the planner/dispatcher, each with its precedence rule
(alert > dispatch; highest SIMPLE level wins; all converge on the per-slot import
cap). Transport modernisation (webhooks/MQTT/TLS) remains the open cert-blocker
cluster; report-management depth partially closed via USAGE_FORECAST + envelope
reports (`docs/BACKLOG_OpenADR_Cert.md` §5 ~70%, §6 ~80%).
