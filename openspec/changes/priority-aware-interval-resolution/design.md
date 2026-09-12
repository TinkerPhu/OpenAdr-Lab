## Context

`tasks/poll_events/detect.rs` turns the VEN's active OpenADR events into two schedules via one
shared core, `controller/rate_schedule.rs::collect_interval_groups`:
`parse_rate_snapshots` (PRICE / EXPORT_PRICE / GHG → `TariffSnapshot`) and
`parse_capacity_schedule` (IMPORT/EXPORT_CAPACITY_LIMIT → `CapacitySnapshot`). The core expands
looping events, orders events "wins last" (BL-02: higher priority processed last, equal priority
→ newer `createdDateTime` last) and writes each interval into a map keyed by its exact
`(start, end)`. Priority therefore only arbitrates intervals with *identical* boundaries.
Partially overlapping intervals all survive into the output.

Each consumer then resolves the overlap its own way:

| Consumer | Rule applied to overlaps | Effect |
|---|---|---|
| `controller/monitor.rs` (tick cost/ledger) | first interval (by start) covering `now` | earliest start wins |
| `tasks/history_sampler/accumulator.rs` (`grid_samples` tariff and limit columns) | same `.find` | earliest start wins |
| `entities/tariff_snapshot.rs::TariffTimeSeries::from_snapshots` (planner) | step series keyed by interval start | latest start wins, and its value holds past its own end until the next start |
| `set_planned_capacity_limits` / `GET /capacity/schedule` | raw list | overlaps passed through |

Observed on the 2026-08-31 fleet run (GB-45): an hourly broadcast TOU event (no scenario
priority advantage, created earlier) beat the scenario's 10-minute price intervals in the
recorded tariff for S-1/S-2 for the whole window, while the planner's step series saw the
scenario prices. OpenADR 3.1 User Guide §7.1: priority applies to events that overlap in time
and conflict; lower number is higher priority, zero highest; equal-priority conflicts are
unspecified.

## Goals / Non-Goals

**Goals:**
- One resolution rule, applied once, at the source: the schedules `detect.rs` publishes are
  non-overlapping and already priority-resolved, so every consumer agrees by construction.
- Spec-conformant: priority decides any time overlap, not just identical intervals.
- A short high-priority interval affects exactly its own window; the lower-priority value
  resumes after it.
- Zero behavior change for non-overlapping input (every existing test keeps passing unchanged).

**Non-Goals:**
- `parse_capacity_state` (strictest-limit collapse for live enforcement) — different, deliberate
  semantics; untouched.
- Step-series behavior across genuine gaps (no event covers an instant) and the stale-rate policy
  (GB-42) — unchanged.
- Merging adjacent segments that happen to carry equal values.
- Any change to how events are fetched, targeted, or looped.

## Decisions

**D1 — Resolve at the source, not in the consumers.** Make `collect_interval_groups` emit a
non-overlapping, resolved segment list. *Alternative:* keep overlaps and give every consumer a
shared priority-aware lookup. Rejected: priority/created metadata would have to flow through
`TariffSnapshot`/`CapacitySnapshot` into four consumers (one of them the planner's series
builder), and any future consumer could reintroduce its own rule — the exact failure mode here
(generic-over-bespoke: fix it in the shared primitive).

**D2 — Atomic segments from the union of boundaries.** After looping expansion, collect every
candidate interval `(start, end, rank, payloads)`; the segment boundaries are the sorted union of
all starts and ends. For each `[b_i, b_{i+1})` covered by at least one candidate, emit one
segment. With non-overlapping input the union equals the original boundaries, so output is
identical to today's. *Alternative:* sweep-line with an active set — same result, more code; the
candidate count is small (at most ~11 loop cycles × intervals per event), so an O(boundaries ×
candidates) scan is well within a poll cycle. Revisit only if a profile ever shows it.

**D3 — Resolve per payload type, per segment.** For each payload type requested by the caller,
the segment takes the value from the highest-ranked candidate that covers the segment *and
carries that payload type*. A priority-1 PRICE-only event over a priority-5 PRICE+GHG event
yields PRICE from the first and GHG from the second — the spec's own example of events that
overlap without conflicting. This preserves today's per-type merge for identical intervals.

**D4 — Rank = (priority, createdDateTime, input order).** Lower `priority` wins; absent priority
ranks below every explicit value (unchanged from BL-02); equal priority → newer
`createdDateTime` wins (unparseable/absent = oldest, unchanged); still tied → later in the input
wins, so the result is deterministic. Reuse the existing BL-02 comparator rather than a second
one.

**D5 — Segments with no covering candidate are not emitted** (unchanged: gaps stay gaps; the
planner's stale-rate policy handles them as today).

## Risks / Trade-offs

- [More, shorter snapshots when events overlap] → consumers already handle arbitrary interval
  lists; `/capacity/schedule` and `/tariffs` simply show the resolved segments, which is the
  truthful view. Covered by a test asserting non-overlap and full coverage.
- [Capacity schedule now follows priority across partial overlaps, where a stricter
  lower-priority limit previously could survive as a separate overlapping interval] →
  spec-conformant and consistent with the identical-interval case today; live enforcement still
  uses `parse_capacity_state` (strictest). Called out in docs.
- [Equal-priority conflicts are unspecified by OpenADR] → our tie-break (newest wins, then input
  order) is documented and tested, not left to iteration order.
- [Planner prices change for overlapping cases] → intended: the planner loses the "short interval
  leaks to the next start" artifact and plans against what the monitor bills.

## Migration Plan

Pure VEN logic change; no data migration. Deploy with the normal VEN rebuild on Node1 (ven-1..3)
and Node2 (ven-4..20). Rollback = revert the commit and redeploy. Verified in the next fleet run
by the harness pre-flight (recorded tariff equals scenario price) and GB-45's S-1/S-2 re-run.

## Open Questions

None blocking.
