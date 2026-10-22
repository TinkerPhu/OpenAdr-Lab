---
title: OpenADR-Spec-Implied Use Cases — Gap Analysis
type: use-case
created: 2026-07-04
updated: 2026-10-22
synced_commit: 650b626
sources: [docs/openadr_3_1_specs/, docs/BACKLOG_OpenADR_Cert.md, docs/architecture/VEN_ARCHITECTURE.md, docs/reference/WIRE_PROFILE.md, tests/features/, VEN/src/entities/capacity.rs, VEN/src/controller/openadr_interface.rs, VEN/src/controller/reporter.rs, lab-core/src/event_timing.rs]
tags: [use-cases, openadr, gap-analysis, spec]
---

# OpenADR-Spec-Implied Use Cases — Gap Analysis

What the OpenADR 3.1 spec *expects* a VEN-side system to do (User Guide §5 user stories,
§6 scenarios, §7 feature examples, §8 formal Use Cases), and whether [[openadr-lab]] can do
it today. Requirement-level detail and the certification percentages live in
`docs/BACKLOG_OpenADR_Cert.md`; the operational DR catalogue is [[system-use-cases]].
Legend: ✅ supported · 🟡 partial · ❌ missing.

## Actor roles: BL (VTN-side) vs CL (VEN-side)

The spec splits responsibilities by actor (User Guide §5.1), and this lab implements one side
of that split in depth:

| Actor | Role | User stories (§5.2–§5.4) | This lab |
|---|---|---|---|
| **Business Logic (BL)** | Client of the VTN acting for the energy/service provider | CRUD programs and events; request/read/delete reports; subscribe to report-creation callbacks | The VTN UI ([[vtn-stack]]) is a thin operator console over the BFF's single business credential (`bl-client`) — it covers BL's CRUD needs but not autonomous BL logic (no automated program/event generation, no DR dispatch strategy on the utility side) |
| **Customer** | Person doing one-time VEN setup | Receive program description, VTN base URL, client ID, API credentials, resource IDs from BL (out-of-band) | Scripted, not delivered by a BL-driven enrollment flow: `scripts/seed_vtn.py` provisions every VEN's user, scopes and credential before it starts, and the VEN profile carries the rest ([[fleet-tooling]]); VEN self-registration is closed by decision (GB-51) |
| **Customer Logic (CL)** | Functional logic inside the VEN | Read programs/events, subscribe to change notifications, create/update reports | The lab's actual focus — [[openadr-interface]] + [[hems-planning]] implement the CL role (poll-based, not subscription-based; see the push-notification gap below) |

BL-side automation (the provider's actual DR program logic, forecasting and dispatch decisions)
is out of scope: the VTN UI and the experiment harness drive events for testing, not to model a
utility's business logic ([[experiment-harness]]).

## From the CL/VEN user stories (User Guide §5.4) and scenarios (§6)

| Spec-implied use case | Spec ref | Status | Lab reality / suggested feature |
|---|---|---|---|
| Discover programs, select the applicable one | §5.4, §6.4 | ✅ | VEN polls `GET /programs` at startup and on its programs cadence ([[openadr-interface]]) |
| Read events, respond within a program | §5.4, §6.5 | ✅ | Event poll → typed translation → `PlanTrigger` → [[milp-planner]] replan |
| **Push notification** on program/event/report change (webhook subscription) | §5.4, §6.3.1 | ❌ | Polling only (certification §7, 0 %). Needs a subscription object, a callback endpoint and the echo challenge; would cut the poll latency |
| Notification via additional protocols (MQTT) | §6.3.2 | ❌ | The VEN has MQTT clients for the weather, measurement and fleet-telemetry feeds ([[weather-forecast]], [[real-measurement-mqtt]], [[fleet-monitoring]]), but none for OpenADR notifications; the VTN's own MQTT notifier binding does not conform yet |
| Create/update reports fulfilling report requests | §5.4, §6.6, §7.5 | ✅/🟡 | Obligation-driven and recurring: `USAGE`, `USAGE_FORECAST`, `BASELINE`, `STORAGE_CHARGE_LEVEL`, `OPERATING_STATE` and the reservation-capacity payloads are built, each declaring its unit; `DEMAND` is not. `startInterval`, `repeat` and `aggregate` descriptor fields are ignored, rolling reports and `DELETE /reports/{id}` (a certification MUST) are missing ([[openadr-interface]]) |
| Hourly prices + usage response | §6.6 | ✅ | The lab's core loop: `PRICE` events → MILP cost objective → usage reports ([[hems-planning]]) |
| Load shed on command | §6.6 | ✅ | `ALERT_GRID_EMERGENCY` / `ALERT_BLACK_START` clamp planned import to 0 over the window, `SIMPLE` levels 0–3 cap import, and the arbiter enforces the limit at execution ([[deviation-arbiter]]) |
| Device status reporting | §6.6 | 🟡 | `OPERATING_STATE` is derived from sample freshness (fresh → `ACTIVE`, stale → `UNAVAILABLE`), at site level only; no per-device state |
| VEN + resource registration & management | §6.7 | 🟡 | Registered by the seed script, not by the VEN; no resource objects; runtime credential/URL change needs a restart. A decision, not an obstacle (GB-51) |
| Enrollment / connectivity check events | §6.2 | ✅ | `enrollment.feature` covers targeting; no-op events flow through the normal poll path |
| Event priority between overlapping events | §7.1 | ✅ | Resolved **per segment** by ascending `priority`, newer `createdDateTime` breaking ties, each segment keeping the event it came from (GB-45, `lab_core::event_timing`; [[openadr-interface]]) |
| Variable-duration intervals in one event | §7.4 | ✅ | ISO 8601 durations and `P9999Y` looping events follow one interval-timing rule for every parser (GB-48); the planner samples the tariff on its own grid ([[three-tier-plan-grid]]) |
| Baseline reporting for M&V | §7.5, `BASELINE` payload | ✅ | The event-blind heuristic forecast, as energy per interval, tagged `DATA_QUALITY` `"HEURISTIC"` ([[heuristics-pipeline]]); no confidence figure. See the Baseline/Forecast distinction in [[demand-response]] |

## Formal Use Case catalogue (User Guide §8)

The spec's own named Use Cases, each a BL-authored program/event pattern the VEN must
recognise and respond to:

| Use Case | Spec ref | Status | Lab reality |
|---|---|---|---|
| Alert (grid emergency, non-financial) | §8.1 | 🟡 | `GRID_EMERGENCY` and `BLACK_START` are handled; `OUTAGE`, `FLEX_ALERT`, `FIRE`, `FREEZING` and `WIND` are not |
| Load Shed — CPP / direct load control | §8.2 | ✅ | Alerts and `SIMPLE` levels 0–3 shed through the per-slot import cap; a price spike does the CPP part |
| Day-Ahead Prices with Usage Report | §8.3 | ✅ | Core loop: `PRICE`/`GHG` → [[milp-planner]] → `USAGE` reports |
| Inverter Management | §8.4 | 🟡 | The PV asset is curtailed by whichever limit is tightest — plan, capacity event or a manual inject — and the winner is recorded as the curtailment source ([[asset-layer]]); no power-factor or reactive setpoints |
| Load Control | §8.5 | 🟡 | `DISPATCH_SETPOINT` steers net site power during its window with the storage levers, the plan running underneath, an alert taking precedence ([[deviation-arbiter]]); `DISPATCH_INSTRUCTION`, `CONTROL_SETPOINT` and `CONTROL_LEVEL_OFFSET` are not handled; `CHARGE_STATE_SETPOINT` is received and deliberately not applied (R-100) |
| State of Charge Reporting | §8.6 | ✅ | `STORAGE_CHARGE_LEVEL` from battery/EV SoC, per asset or aggregated |
| Capability Forecast Reporting | §8.7 | ❌ | `reportDescriptor.historical: false` is honoured, but no `LOAD_SHED_DELTA_AVAILABLE` / `GENERATION_DELTA_AVAILABLE` payload exists |
| Operational Forecast Reporting | §8.8 | ✅ | `USAGE_FORECAST` (and any usage-family payload with `historical: false`) reports the plan's net site power per slot at the plan's native boundaries |
| Capacity Management (Dynamic Operating Envelopes, Dynamic Capacity Mgmt) | §8.10 | ✅ | `*_CAPACITY_LIMIT` are time-aware hard constraints on each plan slot; import and export subscription and reservation form a contracted allowance that binds when tighter than the limit and is the default once the limit schedule is exhausted (§8.10.1); the VEN reports the capacity it requests ([[tariffs-and-capacity]]). Grid-power-outage handling is not covered |
| Custom Dispatch Instructions | §8.12 | 🟡 | Setpoint dispatch works as under §8.5; a free-form dispatch-instruction payload does not exist |
| Dynamic Targeting | §8.13 | ✅ | `enrollment.feature` exercises BL-granted targets; see [[openadr-security]] for the targeting/object-privacy mechanism |

## Reading of the gaps

Three clusters remain: **transport and trust** (webhooks with the echo challenge, OpenADR MQTT
notifications, TLS, token-endpoint discovery — the certification MUSTs, see [[vision-and-roadmap]]),
**payload families not handled** (`CONTROL_*`, `DISPATCH_INSTRUCTION`, `CURVE`/`OLS`, five alert
types, capability-delta forecasts) and **report management depth** (rolling and status reports,
`DELETE /reports/{id}`, three descriptor fields).

**Event arbitration** was the cluster executed first (Phase 3, "Control-Method Lab"): SIMPLE levels,
grid-emergency alerts, capacity reservations and dispatch setpoints constrain or steer the planner and
arbiter, each with a precedence rule (alert > dispatch; highest SIMPLE level wins; all converge on the
per-slot import cap; overlapping events resolved by priority). The certification analysis reports
Event Handling at about 55 % and Report Submission at about 75 %, with four MUSTs open (TLS, `/auth/server`
discovery, the webhook callback, `DELETE /reports/{id}`), so the VEN is not yet certifiable
(`docs/BACKLOG_OpenADR_Cert.md`).
