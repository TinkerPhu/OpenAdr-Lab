---
title: HEMS Planning Concepts
type: concept
created: 2026-07-04
updated: 2026-08-09
synced_commit: 4c4f149
sources: [docs/REQUIREMENTS.md, docs/architecture/VEN_ARCHITECTURE.md, VEN/src/routes/hems/, VEN/src/entities/device_session.rs, VEN/src/services/user_request.rs]
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

A **User Request** ("charge EV to 80% by 07:00") carries one of six
`UserRequestMode`s — `ASAP`, `ASAP_FREE`, `BY_DEADLINE` (default = the pre-mode
behaviour), `BY_DEADLINE_FREE`, `MAX_COST`, `OPPORTUNISTIC` (REQUIREMENTS.md
§3.2.1). Since Phase 4 (BL-28) the mode is real on the EV path: it is stored on
the session and branches the MILP's session-intent translation — see
[[milp-planner]] for the per-mode mechanics (lateness penalty, free-energy
gating, budget cap). Heater/shiftable sessions store the mode but the planner
does not branch on it yet. The User Request Manager translates requests into
device sessions — `EvSession`, `HeaterTarget`, `ShiftableLoad` — applying
per-asset `CompletionPolicy` defaults and computing energy from SoC delta ×
capacity (VEN_ARCHITECTURE.md §2.1). Sessions enter the MILP as **constraints**
(deadline step, energy target, `MilpLoadMode`), never as iterated objects
(§2.3.1). A user may also override an asset's comfort/value curve
(WP4.2/BL-19), preferred over `default_comfort_rates()` wherever the curve is
consulted. Since BL-34, the resolved curve is no longer dropped after
resolution: `services/user_request.rs::create_ev`/`create_heater` carry it onto
`EvSession`/`HeaterTarget` as `comfort_rates`, from which the MILP actually
sources reward coefficients for `ByDeadline`/`Asap` EV sessions and heater
full-tier operation — see [[milp-planner]]'s comfort-curve section for the
solver-side mechanics.

**Session teardown closes the loop back onto the request.** Clearing a device session no
longer goes through per-device CRUD routes — BL-41 removed the direct-write `/ev-session`
(POST/DELETE), `/heater-target`, and `/shiftable-loads` routes once the UI had fully moved to
the unified `POST /user-requests` flow, which constructs the same underlying session objects
(`services::user_request::UserRequestService`). Teardown is `state.cancel_request(id)`
(`VEN/src/state/mod.rs`), called from `DELETE /user-requests/:id`
(`routes/hems/sessions.rs::delete_request`): it clears the linked `EvSession`/
`HeaterTarget`/`ShiftableLoad` and transitions the `UserRequest` itself, atomically, in one
place — the older code split this across `EvSessionService::end`/`HvacService`/route-level
logic (each now either deleted or reduced to a thin, unused sketch kept only as a decision
record, `docs/BACKLOG.md` BL-23). `GET /ev-session` is the one CRUD-era route kept, read-only:
a VTN-issued `CHARGE_STATE_SETPOINT` event still creates an `EvSession` directly
(`tasks/poll_signals.rs`) with no linked `UserRequest`, so it would otherwise be invisible to
`GET /user-requests`. `baseline_override`'s own `GET`/`POST`/`DELETE /baseline-override`
routes are *not* part of this removal — investigation for BL-41 found `BaselineOverride` has
no `/user-requests` equivalent at all (`CreateUserRequestBody`/`SessionType` don't model bulk
per-slot forecast adjustment); it's a genuinely separate capability, split off as its own
backlog item (BL-42) to give it a Devices-tab UI surface, since the backend route was already
live and tested with none.

## Accounting

The **Asset Ledger** accumulates energy/cost/CO₂ per asset each dispatcher tick;
it is in-memory only and resets on restart (persistence gap, REQUIREMENTS.md §2.3).

The glossary's **Device Session** entry (REQUIREMENTS.md §2.3) is the vocabulary for the
`EvSession`/`HeaterTarget`/`ShiftableLoad` structs above: a schedulable energy-or-equivalent
target with a deadline, represented per asset type rather than through one shared type or
status field. Whether a shared trait across these three would simplify anything is
examined in [[device-session-common-interface]] (no — the divergent parts don't unify).

Grid-boundary arithmetic underlying all of this: [[sign-convention]].
