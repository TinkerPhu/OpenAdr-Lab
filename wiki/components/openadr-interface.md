---
title: OpenADR Interface (VEN)
type: component
created: 2026-07-04
updated: 2026-10-21
synced_commit: 97a6abc
sources: [docs/architecture/VEN_ARCHITECTURE.md, docs/reference/WIRE_PROFILE.md, VEN/src/vtn.rs, VEN/src/controller/vtn_port.rs, VEN/src/controller/openadr_interface.rs, VEN/src/controller/rate_schedule.rs, VEN/src/controller/wire_reject.rs, VEN/src/controller/token_scopes.rs, VEN/src/controller/reporter.rs, VEN/src/controller/report_payload.rs, VEN/src/controller/report_intervals.rs, VEN/src/controller/report_accumulator.rs, VEN/src/tasks/poll_events/, VEN/src/tasks/poll_signals.rs, VEN/src/tasks/obligation.rs, VEN/src/entities/capacity.rs, VEN/src/state/obligations.rs, VEN/src/services/obligation.rs, lab-core/src/]
tags: [openadr, ven, translation, polling]
---

# OpenADR Interface (VEN)

The **only** VEN component that knows OpenADR HTTP, OAuth, and event payload formats. Transport lives in
`VEN/src/vtn.rs` behind `VtnPort` (paths, pagination capped at the VTN's 50 per page, a non-2xx answer
translated to `DomainError::VtnRejected`, 409-upsert report semantics). It never authenticates by hand:
the token is `lab_core::vtn_auth`'s (`ClientCredentials::send` attaches it and re-authenticates once),
shared with the BFF ([[wire-contract]], [[vtn-stack]]). Parsing is pure functions in
`VEN/src/controller/openadr_interface.rs` and `rate_schedule.rs` (the priority-merge and cycle-looping
core shared by tariffs and the capacity schedule); the poll loop and change detection sit in
`VEN/src/tasks/poll_events/` (`mod.rs`: the I/O loop; `detect.rs`: the pure change-detection core;
`capacity_picture.rs`: when two polls count as the same capacity) ([[ven-hexagonal-architecture]]).
Poll interval: `POLL_EVENTS_SECS`, default 30 s.

## Events are the wire crate's types

`OadrEvent`, `OadrInterval` and `OadrPayload` are aliases for `openleadr-wire`'s `Event`,
`EventInterval` and `EventValuesMap` (`controller/vtn_port.rs`): a type from the wire crate cannot forget
a field, which is how `event.duration`, `targets`, `payloadDescriptors` and `randomizeStart` once went
missing together. The rule: migrate a type when we construct it or when its parse must be strict; keep a
DTO when we only pass it through (`OadrReport` keeps unknown fields via `#[serde(flatten)]`, so
`GET /reports` serves the VTN's reports verbatim). Two consequences ([[dto-pass-through]]):

- **An incoming number has a unit.** It is read only through `lab_core::wire_contract::PayloadReader`,
  which resolves the unit from the event's `payloadDescriptors`, else its program's, else the profile
  default, and returns the value or a `Refusal` (`SIMPLE`'s declared kind `Integer` is read too). The poll
  builds one reader from the stored programs (`vtn_port::payload_reader`); its audit becomes a
  `events:units` wire rejection (`/health` `wire_conformance` degraded, one notification) or, for an
  assumed default, `/health` `wire_assumptions` and a line on the Dashboard's Health card
  ([[wire-contract]]). The architecture audit refuses raw `.numeric()` reads.
- **A missing required field is refused, not defaulted.** `controller::wire_reject` is the one rule for
  "the VTN sent an object we cannot parse": each object is parsed independently, every good one is kept,
  and each bad one is recorded and shown on `/health` — one bad event no longer blinds the VEN to every
  good one, and none is dropped silently. The only place the VEN is stricter than the schema is
  `intervalPeriod.start` (R-87, `docs/reference/TECHNICAL_DEBTS.md`).

**Granted scopes (GB-49).** A VEN whose user carries no VEN scopes authenticates fine and then polls an
empty world. `controller/token_scopes.rs` reads the scopes off its own access token (signature
deliberately not verified: this is advice to the operator about our own grant, not access control) and
says so when the VTN granted this VEN nothing.

## Inbound: what is actually parsed

| OpenADR payload type | Internal target |
|---|---|
| `PRICE` / `EXPORT_PRICE` / `GHG` | `TariffSnapshot` per interval (`parse_rate_snapshots`) |
| `IMPORT_/EXPORT_CAPACITY_LIMIT` | `CapacitySnapshot` schedule, served at `GET /capacity/schedule` |
| `IMPORT_/EXPORT_CAPACITY_SUBSCRIPTION` / `_RESERVATION` | the same schedule (per-segment subscription and reservation; a reservation applies only inside its own window); subscription + reservation form a contracted allowance per slot that binds the planner when tighter than the limit, and the default once the limit schedule is exhausted (User Guide §8.10.1) |
| `ALERT_GRID_EMERGENCY` / `ALERT_BLACK_START` | `AlertWindow`; both types clamp planned import to 0 over the window (soft, never infeasible) |
| `SIMPLE` (levels 0–3) | `SimpleWindow` — L1 caps import at a configurable % of contract, L2 at baseline, L3 at 0; highest level wins, alerts override |
| `DISPATCH_SETPOINT` | `DispatchWindow` — the storage levers steer net site power to the commanded setpoint, plan running underneath; alert wins ([[deviation-arbiter]]) |
| `CHARGE_STATE_SETPOINT` | **Received, not applied** (R-100): announced once per event as an Info notification naming why (`NOT_APPLIED_PAYLOADS`, the one table); never an `EvSession`, never a planner input |
| `reportDescriptors` | `OadrReportObligation` per (event, payloadType) |

**There is one capacity schedule.** The poll stores the VTN's schedule, not an "in force now" value;
every reader asks it at the time it cares about, and the planner caps each slot by it
([[tariffs-and-capacity]]). Change detection compares a `CapacityPicture` — what is in force at the poll's
`now`, plus the schedule — so a limit that starts or ends between two polls of an unchanged event list is
still a change.

**Interval timing is one rule** (`lab_core::event_timing`, GB-48): when interval *i* of an event runs,
including `event.duration` looping (an event with `intervalPeriod.duration` longer than its intervals
repeats them to cover the cycle, the persistent-daily-prices pattern `P9999Y`) and `randomizeStart`
(R-86): each VEN moves a declared start by its own offset, for dispatch only, applied at one call site
(`events_this_ven_acts_on`) — so the declared events, the report obligations and the trace still say what
the VTN asked. Overlapping events resolve **by priority per segment** (GB-45), and each resolved segment keeps the event it came from.

## Change detection and replan triggers

`detect.rs` compares each poll with the previous one and returns trace events plus parsed rates, capacity,
signals, `EventReceived` history rows ([[history-store]]) and the unit audit. The poll applies signals in
`tasks/poll_signals.rs`: alert changes fire `PlanTrigger::Alert`, SIMPLE changes `CapacityChange`, and a
poll that changed capacity asks for `CapacityChange` even beside a rate change, because a limit is a
contractual constraint that must be acted on now, whereas a plain `RateChange` is held for a jittered
interval ([[milp-planner]]). A trigger carries the ids of the events it was caused by
(`PlanTriggerSignal::caused_by`, including arrived and expired together), so a replan can name the event
behind it. Event *removal* on a poll means cancellation ([[openadr-3]]). Event polling is unconditional;
only the history write is skipped when persistent history is not configured.

## Outbound: reports

Reports are **obligation-driven only**: the old timer-driven measurement report is gone, and a report
exists because an event's `reportDescriptors` asked for it. Obligations are checked every 5 s
(`tasks/obligation.rs` → `services/obligation.rs`); each builds a multi-interval report on the obligation's
own grid (`controller/reporter.rs::build_measurement_report_for_obligation`, intervals in
`controller/report_intervals.rs`) and submits it with the same stable per-`(ven, event, payload_type)`
report name, so each cycle upserts one VTN report resource (the report id is remembered rather than
re-derived).

- **Every value declares itself.** `controller/report_payload.rs` is the single authority: a payload is
  built by naming its quantity (`energy_kwh`, `energy_from_power_kw`, `power_kw`, `percent`, `level`,
  `state`), so there is no constructor taking a bare number, and `descriptors_for` derives the report's
  `payloadDescriptors` from the payloads actually present. Power is kW and energy kWh throughout, because
  OpenADR's `Unit` enum has no watt ([[wire-contract]]).
- **A request is a guarantee.** The report answers the payload type asked for; width and cadence are
  separate (`frequency` counts intervals), and an interval still being measured is not reported.
- **Accumulation.** Each submission builds only the intervals that closed since the last one, but the
  VTN's `PUT` replaces the object, so `controller/report_accumulator.rs` keeps the VEN's recent intervals
  and sends the whole remembered window, bounded by a count (`MAX_REPORT_INTERVALS`, 120 — a request-size
  bound, since a 60 s grid over 24 h would be megabytes a minute across the fleet) and declaring what it
  still carries. The fleet monitoring series lives at full resolution in the BFF's own telemetry store, so
  the report is not its archive ([[fleet-monitoring]]).
- **Obligations recur.** Each due obligation is re-armed to its next `due_at`
  (`state/obligations.rs::rearm_obligation`) instead of being fulfilled once, so `frequency: 900` yields
  one report per 15 minutes for the event's lifetime; an obligation is retired when its source event
  leaves the active poll set (`retire_obligations_not_in`). A VTN 404 means the source event or program is
  gone (GB-23).

| Report obligation | Source |
|---|---|
| `USAGE` | **Energy** over the interval (kWh): the time-weighted mean of net site import power, resampled with `TimeSeries::resample_uniform`. A report built from a single sample omits `USAGE` rather than send power as energy. Export keeps its sign. |
| `STORAGE_CHARGE_LEVEL` | Point-in-time SoC at each interval end, for every asset that has one: one resource per asset (`<ven>-<asset_id>`), or with `aggregate: true` one resource (`<ven>-meter`) with the energy-weighted SoC (`build_soc_resources`) |
| `OPERATING_STATE` | From sample freshness (fresh ≤ 120 s → ACTIVE, stale → UNAVAILABLE) |
| `IMPORT_/EXPORT_RESERVATION_CAPACITY` | Capacity this VEN is **requesting** beyond its contracted allowance (`entities::reservation_request`), declared `KW`, `0` when no allowance is declared; not live headroom (R-76) |
| `USAGE_FORECAST` | Planned net site power per plan slot at the plan's native boundaries; `reportDescriptor.historical: false` on any usage-family payload takes the same forecast path (§8.7/§8.8, [[openadr-spec-use-cases]]) |
| `BASELINE` | The event-blind heuristic forecast summed across assets, sampled on the obligation's grid, each interval tagged `DATA_QUALITY` `"HEURISTIC"`, so `experiments/kpi.py` can compute an event's impact |
| `DEMAND` | not built |

There is no plan-cycle status report: `PlanCycle` events are visible via `/trace/events` and `/plan/events`
only, and a `TELEMETRY_STATUS` report would need a program id they don't carry
(`docs/BACKLOG_OpenADR_Cert.md`). Report submissions are tracked beyond the aggregate counter (WP-T5) and
each is recorded as a `ReportSent` row ([[history-store]]).

The tariff and capacity values captured per poll tick are described in [[tariffs-and-capacity]].
