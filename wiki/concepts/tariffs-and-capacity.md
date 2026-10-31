---
title: Tariffs and Capacity State
type: concept
created: 2026-07-04
updated: 2026-10-31
synced_commit: 102f7fa
sources: [docs/REQUIREMENTS.md, VEN/src/entities/tariff_snapshot.rs, lab-core/src/time_series.rs, lab-core/src/event_priority.rs, VEN/src/controller/milp_planner/stale_rates.rs, VEN/src/controller/milp_planner/input_stages.rs, VEN/src/entities/capacity.rs, VEN/src/entities/design_vocabulary.rs, VEN/src/controller/rate_schedule.rs, VEN/src/history_store/schema.rs]
tags: [tariff, capacity, domain, envelope]
---

# Tariffs and Capacity State

How time-varying grid signals are captured inside the VEN
(docs/REQUIREMENTS.md §2.3).

## TariffSnapshot

One interval's co-valid rate signals in one row: import tariff, export tariff (€/kWh),
and CO₂ intensity (g/kWh), each optional (`VEN/src/entities/tariff_snapshot.rs`). Fields
originate from `PRICE`/`EXPORT_PRICE`/`GHG` payloads merged per interval; capacity limits
are **not** part of this struct — they are a separate priority-resolved schedule (`CapacitySnapshot`, below;
inbound mapping in [[openadr-interface]]).

## TariffTimeSeries — Step/LOCF by construction

At the planning boundary, snapshots become a `TariffTimeSeries`: three independent
`TimeSeries` (import, export, CO₂) with `Interpolation::Step`
(`entities/tariff_snapshot.rs`). The shared `TimeSeries` type (`lab-core/src/time_series.rs`) provides
`interpolate_at` (LOCF for Step), `time_weighted_mean`, min/max buckets, and grid
resampling — one abstraction serving the planner, the reporter, and the timeline. The
planner prices a slot covered by tariff data at its **time-weighted mean** over the slot, so a slot straddling a
boundary blends both rates; only a slot with no defined mean falls back to the sample at its start
(`milp_planner/input_stages.rs`, R-16).

## Two kinds of capacity — don't conflate

- **Per-interval capacity limits**: hard kW caps from `*_CAPACITY_LIMIT` events; the strictest active limit
  wins and its source event id is retained (`entities/capacity.rs::tightest_capacity_limit`).
- **Capacity State** (`OadrCapacityState`): the contractual picture — subscription and reservation, import and
  export side (`IMPORT_/EXPORT_CAPACITY_SUBSCRIPTION`, `IMPORT_/EXPORT_CAPACITY_RESERVATION`), plus the limits
  in force. It is the API view (`GET /capacity`, `GET /signals`) built by `OadrCapacityState::at` when asked,
  never stored.

Both bound the [[milp-planner]]'s feasible region; reservations also flow back out as
`IMPORT_/EXPORT_RESERVATION_CAPACITY` report payloads built from the live site envelope
([[openadr-interface]]).

## Capacity-limit *schedule*, not just current state

`controller/rate_schedule.rs::parse_capacity_schedule` keeps the full per-interval schedule, so "what limit
applied at 14:32 yesterday" and "what will it be in 2 hours" are the same lookup. Tariffs and capacity are
resolved by one ranking, `lab_core::event_priority::resolve` (lower `priority` number wins, an absent priority
ranks last, equal priority goes to the newer `createdDateTime`); `parse_rate_snapshots` and
`parse_capacity_schedule` differ only in which OpenADR payload types they collect, and the BFF's Fleet signal
bands use the same function, so a UI curve and a VEN's plan read the same winning event. The schedule is served live via
`GET /capacity/schedule` and rendered on the Controller/History tabs as the "Tariff & Envelope" diagram. Past
values are persisted into `grid_samples` (`import_limit_kw`/`export_limit_kw`) using tightest-value-wins per
1-minute window, not a mean — a capacity limit is usually absent, and averaging it against "unlimited" would be
meaningless (same non-averaging pattern as PV curtailment's `generation_limit_kw` in `tick_samples`). The VEN
only ever *transports* this schedule — it has no opinion on how a VTN should compute it. What a real DNSP's
envelope *algorithm* is actually based on, and whether this lab's own VTN should someday generate a plausible
one rather than only relaying one, is an open external-research thread — see [[power-envelope-forecast-basis]].

## One time-aware capacity schedule (GB-48)

Every capacity quantity — import/export limit, subscription, reservation — is a segment of one
priority-resolved `CapacitySnapshot` (`parse_capacity_schedule`); nothing "in force now" is stored.
`tightest_over(span)` answers which value applies over a span (an instant is a zero-width span), so the
planner caps each slot at the tightest overlapping limit (never above the contracted allowance), the tick
and arbiter ask at the tick's `now` (a limit takes effect at its interval boundary, not at the next poll),
and the API view is built when asked. `OadrCapacityState` is a derived view of this schedule; interval timing itself is the shared `lab_core::event_timing` rule ([[wire-contract]]).
Source: VEN_ARCHITECTURE §2.1 "Capacity schedule".

## Stale data

Slots beyond the last published rate (`coverage_end`) are priced by `StaleRatePolicy`
(`milp_planner/stale_rates.rs`, applied to the import tariff and the CO₂ intensity alike; export keeps its
step-hold). The profile default is `HEURISTIC_FORECAST`: a stale slot takes the rate of the same clock time 24 h
earlier — from the cycle's own known series when that lies inside it, else from the `grid_samples` history, and
168 h back when the slot and its 24 h reference are different day types (a weekday's shape is a bad estimate for
a weekend day). It degrades to `LAST_KNOWN` when neither source has data (a fresh VEN). The other policies are
`LAST_KNOWN` (repeat the last rate), `SAFE_AVERAGE` (a configurable percentile, default 0.8) and
`DEFER_TO_FLEXIBLE` (the maximum known rate, so discretionary load moves into covered slots). Built-in fallbacks
when no data exists at all are 0.25 €/kWh import, 0.08 €/kWh export and 300 g/kWh CO₂. Estimated slots are
flagged `rate_estimated` on the plan and the planner's warning text names the data source.

Tariff (€/kWh) vs rate (€/h) terminology: see [[sign-convention]].
