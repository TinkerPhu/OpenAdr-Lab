---
title: OpenADR Interface (VEN)
type: component
created: 2026-07-04
updated: 2026-07-05
synced_commit: e138861
sources: [docs/architecture/VEN_ARCHITECTURE.md, VEN/src/vtn.rs, VEN/src/controller/openadr_interface.rs, VEN/src/controller/reporter.rs, VEN/src/tasks/poll_events.rs, VEN/src/entities/capacity.rs]
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
| `IMPORT_CAPACITY_SUBSCRIPTION` / `IMPORT_CAPACITY_RESERVATION` | `OadrCapacityState` scalar fields (min wins) |
| `reportDescriptors` | `OadrReportObligation` per (event, payloadType), due after `frequency` seconds |

**Looping events**: when `event.intervalPeriod.duration` exceeds the intervals' total
span (the spec's persistent-daily-prices pattern, `P9999Y`), the interval set is repeated
to cover one cycle back through 3 days ahead (`parse_rate_snapshots`). Multiple events
writing the same interval merge **last-write-wins**; the `priority` field is parsed but
not used in ordering (limitation documented at `openadr_interface.rs:102`).

Change detection compares poll results against the previous tick (new/expired event ids,
tariff count, import limit) and pushes trace events; any change fires a single
`PlanTrigger::RateChange` waking the [[milp-planner]] — the `CapacityChange` and `Alert`
trigger variants are never sent. Event *removal* on a poll means cancellation
([[openadr-3]]).

> **DRIFT** `docs/architecture/VEN_ARCHITECTURE.md` §2.1's translation table also lists
> `ALERT_*`, `DISPATCH_SETPOINT`, `CHARGE_STATE_SETPOINT`, and the export-side
> subscription/reservation payloads as inbound targets. None are handled anywhere —
> they survive only as fields of the dead `OadrEventCache` vocabulary struct
> (`entities/capacity.rs:42`), and `OadrCapacityState` has no export-subscription field.
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
- Obligations are **one-shot**: fulfilled once and never re-armed, so
  `frequency: 900` produces one report, not one per 15 min — a certification-relevant
  gap ([[openadr-spec-use-cases]]).
- The plan-cycle TELEMETRY_STATUS report is dead code: `tasks/planning.rs:338` passes
  `program_id = None` and `build_status_report` returns `None` without it.

> **DRIFT** `docs/architecture/VEN_ARCHITECTURE.md` §2.1 additionally lists
> `USAGE_FORECAST` (FIRM slots as point forecasts, FLEXIBLE slots as `[0, MaxPower]`
> ranges) as an outbound obligation — but no code path in `reporter.rs` builds this
> payload type. The MILP planner already computes exactly this per-slot forecast
> internally (`planned_state_by_asset`, exposed to `/timeline` for the UI) — it's just
> never turned into a report. See [[openadr-spec-use-cases]] (§8.7/§8.8) for what the
> spec expects (the VEN doesn't parse `reportDescriptor.historical` at all).

The tariff/capacity values captured per poll tick form the `TariffSnapshot` described in
[[tariffs-and-capacity]].
