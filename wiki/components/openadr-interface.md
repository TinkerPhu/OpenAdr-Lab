---
title: OpenADR Interface (VEN)
type: component
created: 2026-07-04
updated: 2026-07-11
synced_commit: b1aba12
sources: [docs/architecture/VEN_ARCHITECTURE.md, VEN/src/vtn.rs, VEN/src/controller/openadr_interface.rs, VEN/src/controller/reporter.rs, VEN/src/tasks/poll_events.rs, VEN/src/entities/capacity.rs, VEN/src/state.rs, VEN/src/services/obligation.rs]
tags: [openadr, ven, translation, polling]
---

# OpenADR Interface (VEN)

The **only** VEN component that knows OpenADR HTTP, OAuth, and event payload formats.
Transport lives in `VEN/src/vtn.rs` behind `VtnPort` (OAuth2 client-credentials with
60 s expiry margin, one automatic 401/403 token-refresh retry, 409-upsert report
semantics); parsing is pure functions in `VEN/src/controller/openadr_interface.rs`;
the poll loop and change detection sit in `VEN/src/tasks/poll_events.rs`
([[ven-hexagonal-architecture]]). Poll interval: `POLL_EVENTS_SECS`, default 30 s.

## Inbound: what is actually parsed

| OpenADR payload type | Internal target |
|---|---|
| `PRICE` / `EXPORT_PRICE` / `GHG` | `TariffSnapshot` per interval (`parse_rate_snapshots`) |
| `IMPORT_/EXPORT_CAPACITY_LIMIT` | `OadrCapacityState.import_/export_limit_kw` — strictest wins, source event id kept |
| `IMPORT_/EXPORT_CAPACITY_SUBSCRIPTION` / `_RESERVATION` | `OadrCapacityState` scalar fields (min wins); subscription+reservation form a contracted allowance that binds the solver when tighter than the limit (Phase 3, WP3.3) |
| `ALERT_GRID_EMERGENCY` / `ALERT_BLACK_START` | `AlertWindow` (window from interval- or event-level `intervalPeriod`; both types clamp planned import to 0 — Phase 3, WP3.1) |
| `SIMPLE` (levels 0–3) | `SimpleWindow` — L1 caps import at a configurable % of contract, L2 at baseline, L3 at 0; highest level wins, alerts override (Phase 3, WP3.2) |
| `DISPATCH_SETPOINT` | `DispatchWindow` — dispatcher steers the battery to the commanded net site power during the window, plan running underneath; alert wins precedence (Phase 3, WP3.4) |
| `CHARGE_STATE_SETPOINT` | creates/updates an `EvSession` targeting the given SoC (fraction or percent); event deletion cancels the event-created session (Phase 3, WP3.4) |
| `reportDescriptors` | `OadrReportObligation` per (event, payloadType), due after `frequency` seconds; `USAGE_FORECAST` and `IMPORT_/EXPORT_CAPACITY_RESERVATION` payload types serve plan-slot forecasts / envelope values (Phase 3, WP3.6) |

**Looping events**: when `event.intervalPeriod.duration` exceeds the intervals' total
span (the spec's persistent-daily-prices pattern, `P9999Y`), the interval set is repeated
to cover one cycle back through 3 days ahead (`parse_rate_snapshots`). Multiple events
writing the same interval merge **last-write-wins**, with events pre-sorted so the
highest-priority one is processed last (BL-02, Phase 0 — `priority` ascending, newer
`createdDateTime` breaking ties).

Change detection compares poll results against the previous tick and pushes trace
events. Signal application lives in `tasks/poll_signals.rs` (Phase 3): alert changes
fire `PlanTrigger::Alert`, SIMPLE changes `CapacityChange`, charge-state changes
`UserRequest`, everything else a single `RateChange` — with the watch-channel caveat
that only the latest trigger survives, so RateChange is suppressed when a more
specific trigger was just sent. Event *removal* on a poll means cancellation
([[openadr-3]]) — including cancelling the EvSession a CHARGE_STATE_SETPOINT event
created.

All of `ALERT_*`, `DISPATCH_SETPOINT`, `CHARGE_STATE_SETPOINT`, and the export-side
subscription/reservation payloads are now genuinely handled (Phase 3) — the long-standing
drift against `VEN_ARCHITECTURE.md` §2.1's translation table is closed. Only the
`OadrEventCache` vocabulary struct remains an unwired sketch (its anticipated
DISPATCH_SETPOINT consumer landed as typed `DispatchWindow` state instead; removal
flagged in `docs/BACKLOG.md` BL-24).
> BL-04/BL-06 markers cover the alerts and charge setpoints; the export-side capacity
> payloads and priority-ordered merge are unmarked gaps. See [[ven-code-vs-docs-audit]].

## Outbound: reports (`controller/reporter.rs`)

- **Timer-driven measurement reports** (every `report_interval_s`, default 60 s): one
  TELEMETRY_USAGE-style report per active event *without* reportDescriptors — net site
  import (W), `OPERATING_STATE`, EV `STORAGE_CHARGE_LEVEL` when available.
- **Obligation-driven reports** (checked every 5 s): for events *with*
  reportDescriptors, a **multi-interval** report resampled onto the obligation's
  interval grid via `TimeSeries::resample_uniform` — `USAGE`-family payloads as
  time-weighted-mean net site power, `STORAGE_CHARGE_LEVEL` as point-in-time SoC at
  interval ends, `IMPORT_/EXPORT_CAPACITY_RESERVATION` from the live
  `SiteFlexibilityEnvelope` (up/down kW).
- Obligations **recur**: each due obligation is re-armed to its next `due_at`
  (`interval_duration_s` later) after reporting instead of being permanently fulfilled,
  so `frequency: 900` produces one report per 15 min for the event's lifetime
  (`state.rs::rearm_obligation`, called from `services/obligation.rs`). An obligation is
  retired once its source event drops out of the active poll set
  (`state.rs::retire_obligations_not_in`, called from `tasks/poll_events.rs`). The stable
  per-`(ven, event, payload_type)` report name means each cycle upserts the same VTN
  report resource with the latest trailing window, rather than creating a new report.
- There is no plan-cycle status report — `PlanCycle` events are visible via
  `/trace/events` and `/plan/events` (SSE) only; no VTN report is built from them (the
  dead `TELEMETRY_STATUS`-on-`PlanCycle` code path was removed, not fixed, since it never
  had a real program ID to report against).

> **DRIFT** `docs/architecture/VEN_ARCHITECTURE.md` §2.1 additionally lists
> `USAGE_FORECAST` (FIRM slots as point forecasts, FLEXIBLE slots as `[0, MaxPower]`
> ranges) as an outbound obligation — but no code path in `reporter.rs` builds this
> payload type. The MILP planner already computes exactly this per-slot forecast
> internally (`planned_state_by_asset`, exposed to `/timeline` for the UI) — it's just
> never turned into a report. See [[openadr-spec-use-cases]] (§8.7/§8.8) for what the
> spec expects (the VEN doesn't parse `reportDescriptor.historical` at all).

The tariff/capacity values captured per poll tick form the `TariffSnapshot` described in
[[tariffs-and-capacity]].
