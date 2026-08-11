---
title: Tariffs and Capacity State
type: concept
created: 2026-07-04
updated: 2026-08-11
synced_commit: 17b702a
sources: [docs/REQUIREMENTS.md, VEN/src/entities/tariff_snapshot.rs, VEN/src/common/mod.rs, VEN/src/entities/capacity.rs, VEN/src/entities/design_vocabulary.rs, VEN/src/controller/rate_schedule.rs, VEN/src/history_store/schema.rs]
tags: [tariff, capacity, domain, envelope]
---

# Tariffs and Capacity State

How time-varying grid signals are captured inside the VEN
(docs/REQUIREMENTS.md §2.3).

## TariffSnapshot

One interval's co-valid rate signals in one row: import tariff, export tariff (€/kWh),
and CO₂ intensity (g/kWh), each optional (`VEN/src/entities/tariff_snapshot.rs`). Fields
originate from `PRICE`/`EXPORT_PRICE`/`GHG` payloads merged per interval; capacity limits
are **not** part of this struct — they are flattened into the scalar `OadrCapacityState`
(inbound mapping in [[openadr-interface]]).

## TariffTimeSeries — Step/LOCF by construction

At the planning boundary, snapshots become a `TariffTimeSeries`: three independent
`TimeSeries` (import, export, CO₂) with `Interpolation::Step`
(`entities/tariff_snapshot.rs`). The shared `TimeSeries` type (`common/mod.rs`) provides
`interpolate_at` (LOCF for Step), `time_weighted_mean`, min/max buckets, and grid
resampling — one abstraction serving the planner, the reporter, and the timeline. The
planner currently samples each slot at its **start** timestamp; time-weighted averaging
across boundary-straddling slots is available but not yet used there
([[ven-code-vs-docs-audit]]).

## Two kinds of capacity — don't conflate

- **Per-interval capacity limits**: hard kW caps from `*_CAPACITY_LIMIT` events;
  strictest active limit wins, source event id retained
  (`controller/openadr_interface.rs::parse_capacity_state`).
- **Capacity State** (`OadrCapacityState`): the contractual picture. Import-side only in
  code — `import_subscription_kw` and `import_reservation_kw` are parsed;
  `EXPORT_CAPACITY_SUBSCRIPTION`/`EXPORT_CAPACITY_RESERVATION` have no inbound handling
  and no struct fields (REQUIREMENTS.md §2.3 describes both sides).

## Capacity-limit *schedule*, not just current state

`parse_capacity_state` (above) collapses every active event into a single current-value
scalar — it cannot answer "what limit applied at 14:32 yesterday" or "what will it be in
2 hours." `controller/rate_schedule.rs::parse_capacity_schedule` keeps the full
per-interval schedule instead (same priority-merge/cycle-looping core as
`parse_rate_snapshots`, factored into a shared `collect_interval_groups` helper — the two
parsers differ only in which OpenADR payload types they collect), served live via `GET
/capacity/schedule` and rendered on the Controller/History tabs as the "Tariff &
Envelope" diagram. Past values are persisted into `grid_samples`
(`import_limit_kw`/`export_limit_kw`, schema v9) using **tightest-value-wins per
1-minute window**, not a mean — a capacity limit is usually absent, and averaging it
against "unlimited" would be meaningless (same non-averaging pattern as PV curtailment's
`generation_limit_kw` in `tick_samples`, schema v5). What a real DNSP's envelope
*algorithm* is actually based on — and whether this lab's own VTN should someday
generate one rather than only transporting one — is an open research thread: see
[[power-envelope-forecast-basis]].

Both bound the [[milp-planner]]'s feasible region; reservations also flow back out as
`IMPORT_/EXPORT_CAPACITY_RESERVATION` report payloads built from the live site envelope
([[openadr-interface]]).

## Stale data

When the VTN is unreachable, Step/LOCF extrapolation carries the **last known rate**
forward for all future slots, and hardcoded defaults (0.25 €/kWh import, 0.08 €/kWh
export, 300 g/kWh) cover slots with no data at all (`milp_planner/inputs.rs`). The
`StaleRatePolicy` enum (`entities/design_vocabulary.rs` — `LAST_KNOWN`,
`HEURISTIC_FORECAST`, `DEFER_TO_FLEXIBLE`, `SAFE_AVERAGE`) is unreferenced roadmap
vocabulary, quarantined rather than wired: only its `LAST_KNOWN` behaviour exists today,
as a hardwired consequence of Step interpolation, and plans never mark slots
`rate_estimated` — `docs/BACKLOG.md` BL-07 tracks formalising `LastKnown` and
`rate_estimated` as a real feature ([[ven-code-vs-docs-audit]]).

Tariff (€/kWh) vs rate (€/h) terminology: see [[sign-convention]].
