---
title: OpenADR-Spec-Implied Use Cases — Gap Analysis
type: use-case
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/openadr_3_1_specs/, docs/BACKLOG_OpenADR_Cert.md, docs/architecture/VEN_ARCHITECTURE.md, tests/features/]
tags: [use-cases, openadr, gap-analysis, spec]
---

# OpenADR-Spec-Implied Use Cases — Gap Analysis

What the OpenADR 3.1 spec *expects* a VEN-side system to do (User Guide §5 user stories,
§6 scenarios, §7 feature examples), and whether [[openadr-lab]] can do it today.
Requirement-level detail lives in `docs/BACKLOG_OpenADR_Cert.md`; the operational DR
catalogue is [[system-use-cases]]. Legend: ✅ supported · 🟡 partial · ❌ missing.

## From the CL/VEN user stories (User Guide §5.4) and scenarios (§6)

| Spec-implied use case | Spec ref | Status | Lab reality / suggested feature |
|---|---|---|---|
| Discover programs, select the applicable one | §5.4, §6.4 | ✅ | VEN polls `GET /programs` at startup and every 300 s ([[openadr-interface]]) |
| Read events, respond within a program | §5.4, §6.5 | ✅ | 30 s poll → typed translation → `PlanTrigger` → [[milp-planner]] replan |
| **Push notification** on program/event/report change (webhook subscription) | §5.4, §6.3.1 | ❌ | Polling only. Feature: subscription object + webhook receiver endpoint; would cut the 30 s reaction latency |
| Notification via additional protocols (MQTT) | §6.3.2 | ❌ | No MQTT client (cert backlog §2). Feature: optional MQTT listener beside the poller |
| Create/update reports fulfilling report requests | §5.4, §6.6, §7.5 | 🟡 | `USAGE`, `DEMAND`, `STORAGE_CHARGE_LEVEL`, `OPERATING_STATE`, `USAGE_FORECAST`, capacity reservations work; advanced report management (rolling reports, ad-hoc, sub-intervals, `report-only` events, §7.5) not implemented |
| Hourly prices + usage response | §6.6 | ✅ | The lab's core loop: `PRICE` events → MILP cost objective → usage reports ([[hems-planning]]) |
| Load shed on command | §6.6 | 🟡 | `ALERT_GRID_EMERGENCY`/`ALERT_BLACK_START` → planner shed is BL-04, not yet implemented; `SIMPLE` levels 0–3 unmapped ([[openadr-interface]]) |
| Device status reporting | §6.6 | ✅ | `OPERATING_STATE` from `DeviceResponsiveness` |
| VEN + resource registration & management | §6.7 | 🟡 | venName/resources registered at startup; runtime credential/URL reconfiguration needs a container restart (cert backlog §1) |
| Enrollment / connectivity check events | §6.2 | ✅ | `enrollment.feature` covers targeting; no-op events flow through the normal poll path |
| Event priority between overlapping events | §7.1 | ❌ | No priority resolution — overlapping events are not arbitrated. Feature: honour `priority` when two active events target the same resource |
| Variable-duration intervals in one event | §7.4 | 🟡 | Interval parsing follows the wire format, but MILP tariff sampling assumes the plan-grid resolution ([[three-tier-plan-grid]]); irregular interval edges inside one Zone-A step blur |
| Baseline reporting for M&V | §7.5, `BASELINE` payload | ❌ | `BASELINE` is typed but no baseline computation exists — see the Baseline/Forecast distinction in [[demand-response]] |

## Reading of the gaps

Three clusters emerge: **transport modernisation** (webhooks, MQTT, TLS — all cert
blockers, see [[vision-and-roadmap]]), **report management depth** (§7.5 machinery),
and **event arbitration** (priority, `SIMPLE` mapping, emergency shed). The first cluster
is infrastructure; the last two are planner/domain features that would fit the existing
`PlanTrigger` and obligation models.

> **OPEN QUESTION** Which cluster first? Certification pressure says transport;
> lab-learning value says event arbitration (it exercises the MILP under conflicting
> constraints). Owner call — belongs in `docs/BACKLOG.md` prioritisation.
