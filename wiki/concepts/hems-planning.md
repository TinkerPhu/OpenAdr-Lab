---
title: HEMS Planning Concepts
type: concept
created: 2026-07-04
updated: 2026-07-06
synced_commit: ae4a1ed
sources: [docs/REQUIREMENTS.md, docs/architecture/VEN_ARCHITECTURE.md, VEN/src/routes/hems/, openspec/specs/ev-session-request-completion/spec.md, VEN/src/entities/device_session.rs]
tags: [hems, planning, sessions, domain]
---

# HEMS Planning Concepts

The vocabulary of the VEN's Home Energy Management System — how user intent and grid
signals become a schedule (docs/REQUIREMENTS.md §2.3).

## Two-speed loop

The controller runs at two timescales: a **slow loop** (planner:
`replan_interval_s` periodic, default 300 s, plus a `PlanTrigger` watch channel — any
component can request a replan, each trigger yields exactly one plan;
`VEN/src/tasks/planning.rs`) and a **fast loop** (dispatcher + monitor at 1 s).
`docs/architecture/VEN_ARCHITECTURE.md` §2.2 still quotes "20 s periodic" — stale.
Trigger senders in code: routes (`UserRequest`), sim inject / `POST /plan/trigger`
(`AssetStateChange`), event poll (`RateChange` for *any* detected change), shiftable-load
completion (`UserRequest`); `Alert` and `CapacityChange` are defined but never sent.
Implementations: [[milp-planner]], [[dispatcher]].

## Slot semantics

- **FIRM slot** — must execute; driven by hard user requests or minimum-SoC constraints.
- **FLEXIBLE slot** — may shift or cancel if constraints change; typically price-driven
  charging windows.
- Classification is time-based: slots within `now + NearHorizonDuration` are FIRM, beyond
  are FLEXIBLE (VEN_ARCHITECTURE.md §2.3). The architecture design intends this
  distinction to shape a forecast report (FIRM as points, FLEXIBLE as `[0, MaxPower]`
  ranges) — that report is not actually built yet; see the DRIFT in [[openadr-interface]].

## User intent

A **User Request** ("charge EV to 80% by 07:00") supports modes `ASAP`, `BY_DEADLINE`,
`MAX_COST`, `OPPORTUNISTIC` (REQUIREMENTS.md §2.3). The User Request Manager translates it
into device sessions — `EvSession`, `HeaterTarget`, `ShiftableLoad` — applying per-asset
`CompletionPolicy` defaults and computing energy from SoC delta × capacity
(VEN_ARCHITECTURE.md §2.1). Sessions enter the MILP as **constraints** (deadline step,
energy target, `MilpLoadMode`), never as iterated objects (§2.3.1).

**Session teardown closes the loop back onto the request.** Deleting an `EvSession`
(`DELETE /ev-session`, `VEN/src/routes/hems/ev.rs`) does not just clear session state — it
walks `UserRequest`s by `session_id` and transitions any still `Active` to `Completed`
before the session is cleared (`openspec/specs/ev-session-request-completion/spec.md`).
Only `Active` requests are touched; `Cancelled` ones and requests tied to a different
session are left alone. Without this, a completed or manually-ended charge would leave
its originating request stuck `Active` forever — a UI-visible dangling state.

## Accounting

The **Asset Ledger** accumulates energy/cost/CO₂ per asset each dispatcher tick;
it is in-memory only and resets on restart (persistence gap, REQUIREMENTS.md §2.3).

The glossary's **Device Session** entry (REQUIREMENTS.md §2.3) is the vocabulary for the
`EvSession`/`HeaterTarget`/`ShiftableLoad` structs above: a schedulable energy-or-equivalent
target with a deadline, represented per asset type rather than through one shared type or
status field. Whether a shared trait across these three would simplify anything is
examined in [[device-session-common-interface]] (no — the divergent parts don't unify).

Grid-boundary arithmetic underlying all of this: [[sign-convention]].
