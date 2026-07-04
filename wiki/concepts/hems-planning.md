---
title: HEMS Planning Concepts
type: concept
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/REQUIREMENTS.md, docs/architecture/VEN_ARCHITECTURE.md]
tags: [hems, planning, sessions, domain]
---

# HEMS Planning Concepts

The vocabulary of the VEN's Home Energy Management System — how user intent and grid
signals become a schedule (docs/REQUIREMENTS.md §2.3).

## Two-speed loop

The controller runs at two timescales (docs/architecture/VEN_ARCHITECTURE.md §2.2):
a **slow loop** (planner: 20 s periodic + `PlanTrigger` watch channel — any component can
request a replan, each trigger yields exactly one plan) and a **fast loop** (dispatcher +
monitor at 1 s). Implementations: [[milp-planner]], [[dispatcher]].

## Slot semantics

- **FIRM slot** — must execute; driven by hard user requests or minimum-SoC constraints.
- **FLEXIBLE slot** — may shift or cancel if constraints change; typically price-driven
  charging windows.
- Classification is time-based: slots within `now + NearHorizonDuration` are FIRM, beyond
  are FLEXIBLE (VEN_ARCHITECTURE.md §2.3). Reported upstream as point forecasts vs
  `[0, MaxPower]` ranges ([[openadr-interface]]).

## User intent

A **User Request** ("charge EV to 80% by 07:00") supports modes `ASAP`, `BY_DEADLINE`,
`MAX_COST`, `OPPORTUNISTIC` (REQUIREMENTS.md §2.3). The User Request Manager translates it
into device sessions — `EvSession`, `HeaterTarget`, `ShiftableLoad` — applying per-asset
`CompletionPolicy` defaults and computing energy from SoC delta × capacity
(VEN_ARCHITECTURE.md §2.1). Sessions enter the MILP as **constraints** (deadline step,
energy target, `MilpLoadMode`), never as iterated objects (§2.3.1).

## Accounting

The **Asset Ledger** accumulates energy/cost/CO₂ per asset each dispatcher tick;
it is in-memory only and resets on restart (persistence gap, REQUIREMENTS.md §2.3).

> **DRIFT** The glossary's **Energy Packet** (schedulable kWh unit,
> `PENDING → ACTIVE → COMPLETED/ABANDONED`, REQUIREMENTS.md §2.3) describes a
> scheduling model that was superseded by device sessions: the packet-seeding config and
> `packet_id` fields were removed from the code on 2026-07-04 (`VEN/src/profile.rs`,
> `entities/site_meter.rs` — "Packet-based scheduling — not yet implemented"), and
> sessions carry the lifecycle now. Residual "packet" vocabulary remains in comments and
> `PacketTransition` trace events. Glossary update tracked in `wiki/review.md`.

Grid-boundary arithmetic underlying all of this: [[sign-convention]].
