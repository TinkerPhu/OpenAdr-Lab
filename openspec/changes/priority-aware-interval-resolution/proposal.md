## Why

When two OpenADR events overlap in time with different interval boundaries (a day-ahead
hourly TOU price plus a 10-minute DR price, or a day-long envelope plus a short capacity
limit), the VEN ignores event `priority`: the BL-02 merge only arbitrates intervals with
identical `(start, end)`, and every consumer then resolves the remaining overlap with its own
ad-hoc rule. The tick-time monitor and the history sampler take the earliest-starting
interval; the planner's `TariffTimeSeries` takes the latest-starting one and lets a short
interval's value leak until the next interval starts. So the same VEN plans against one
price and bills/records another. The 2026-08-31 fleet run hit exactly this (GB-45,
`docs/BACKLOG.md`): a broadcast TOU demo event overrode the scenario price in every recorded
tariff for S-1 and S-2, invalidating their cost KPIs while the planner saw the scenario
price. OpenADR 3.1 User Guide §7.1 is explicit that priority governs events that *overlap
in time and conflict*, not only identical intervals.

## What Changes

- Resolve overlapping event intervals per **atomic time segment**: split at every interval
  boundary of every active event, and for each segment and each payload type pick the value
  from the highest-priority event covering it (lower `priority` number wins; absent priority
  is lowest; equal priority → newer `createdDateTime` wins; still tied → later in the input
  wins, deterministic).
- The resolved schedule is **non-overlapping**, so every consumer (monitor, history sampler,
  planner tariff series, capacity schedule for the planner and `/capacity/schedule`) sees the
  same value for the same instant by construction — no consumer-side resolution rule remains.
- A short high-priority interval ends where it ends: the underlying lower-priority value
  resumes for the rest of the longer interval instead of the short value leaking forward.
- Non-overlapping inputs produce exactly today's output (same intervals, same values) — no
  behavior change for the common single-event case.
- Applies to both parsers built on the shared core: `parse_rate_snapshots` (PRICE,
  EXPORT_PRICE, GHG) and `parse_capacity_schedule` (IMPORT/EXPORT_CAPACITY_LIMIT).
  `parse_capacity_state` (strictest-limit collapse used for live enforcement) is unchanged.

## Capabilities

### New Capabilities
- `event-interval-resolution`: how the VEN turns a set of possibly-overlapping OpenADR event
  intervals into one unambiguous per-instant value per payload type (priority, tie-breaks,
  segment boundaries, looping events), and the guarantee that all consumers read that one
  resolved schedule.

### Modified Capabilities
<!-- none: openspec/specs/ holds no current capability specs (they are folded into docs/ after
     implementation) -->

## Impact

- Code: `VEN/src/controller/rate_schedule.rs` (`collect_interval_groups`); tests in
  `VEN/src/controller/openadr_interface.rs` and `VEN/src/entities/tariff_snapshot.rs`.
  Consumers (`controller/monitor.rs`, `tasks/history_sampler/accumulator.rs`,
  `entities/tariff_snapshot.rs::from_snapshots`, `tasks/poll_events`) need no logic change
  once their input is non-overlapping — verified by tests, not assumed.
- Behavior: recorded tariffs, ledger costs and the planner now agree; KPIs derived from
  `grid_samples.import_tariff_eur_kwh` become trustworthy under overlapping events.
- BDD: a use-case scenario (overlapping day-ahead + intra-hour price) in `tests/features/`.
- Docs: `docs/architecture/` (tariff/event handling section), BACKLOG GB-45 closed.
- No API, dependency, or UI changes.
