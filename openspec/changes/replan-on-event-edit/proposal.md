# Proposal

## Why

A VEN does not replan when the VTN edits an event in place (GB-63). The poll's change detection raises an
arrival only for an event id it has not seen, and a rate change only when the *number* of tariff intervals
changes. An edit keeps the id, and an edit that rewrites prices keeps the interval count, so the VEN carries on
with the old tariff until its next periodic cycle, and a periodic cycle adopts a new plan only above the
acceptance threshold. Observed on 2026-10-29: a `PUT` of a `PRICE` event changed the whole tariff and `ven-1` and
`ven-4` logged nothing. The fleet experiments edit tariffs in place, so the fleet silently kept acting on the
previous one while the operator watched the new one on the Fleet page.

## What Changes

- The event poll remembers each known event as `(id, modificationDateTime)`, not as an id alone. A known id whose
  `modificationDateTime` differs is a new version: it is traced as `OpenAdrArrived` with the new version, written to
  the received-events history, and triggers a replan through the path every arrival takes today.
- The replan names the edited event (`trigger_causes`), as it does for a new one.
- This covers every event kind at once, because the arrival path is the one every kind already shares; no
  per-kind rule is added.
- Nothing changes for a poll that sees the same ids and versions: no trace entry, no trigger.

## Capabilities

### New Capabilities
- `event-edit-replan`: what the VEN's event poll treats as a new event, so that an event the VTN edited in place is
  acted on as promptly as one it created.

### Modified Capabilities

## Impact

- `VEN/src/tasks/poll_events/detect.rs` (the known-event set, the arrival loop, tests) and
  `VEN/src/tasks/poll_events/mod.rs` (the loop's state). The VEN image changes; the VTN, BFF and UIs do not.
- The trace and the planner's trigger list show edits as arrivals carrying the new `modification_date_time`; the
  Planner page already renders that field.
- Not changed: report obligations of an edited event (GB-67) and the fleet reaction feed's choice of the first
  arrival (GB-68), both filed.
