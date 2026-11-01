# Design

## Context

See proposal.md for why. What exists today, found by searching the concept ("when does the VEN count an event as
new, and what makes it replan"):

| Concept | Where it lives today | Verdict |
|---|---|---|
| The set of events the previous poll saw | `spawn_event_poll` (`tasks/poll_events/mod.rs`): `prev_event_ids: HashSet<String>`, passed to `detect_event_changes` as `prev_ids` and to `retire_obligations_not_in` | Ids only. This is the gap. Becomes the one definition of "known". |
| An event is new | `detect_event_changes` (`detect.rs`): `prev_ids.contains(evt.id)` skips the arrival; the same loop builds `OpenAdrArrived` and the received-events record | The one place an arrival is decided. Reused. |
| An event disappeared | `detect_event_changes`: ids in `prev_ids` and not in the poll give `OpenAdrExpired` | Unchanged. |
| A rate changed | `detect_event_changes`: `rates.len() != prev_tariff_count` | Counts intervals, so it cannot see an edit. Left as it is: with edits counted as arrivals it adds no new case, and removing it is not this change. |
| A capacity changed | `CapacityPicture` (`capacity_picture.rs`): equality of the in-force state and the whole schedule | By value; already sees an edit. Unchanged. |
| An alert, SIMPLE or dispatch window changed | `poll_signals::apply_signal_changes`: `SignalPrevs`, equality of the parsed windows | By value; already sees an edit. Unchanged. |
| Which events caused a replan | `spawn_event_poll`: `trigger_causes` from the `OpenAdrArrived` and `OpenAdrExpired` trace entries | Reused: an edited event is named because it is traced as an arrival. |
| The version of an event the VEN saw | `OpenAdrArrived.modification_date_time`, verbatim from the VTN | Already on the trace entry (and on the `fleet_reactions` row), never compared. |
| Report obligations of an event | `extract_report_obligations`: skips an (event id, payload type) it already tracks | Does not see an edited descriptor. Out of scope, filed as GB-67. |

The VTN sets `modification_date_time = now()` on every update
(`openleadr-rs/openleadr-vtn/src/data_source/postgres/event.rs`), as the OpenADR object model requires of a
server, so the version is a reliable key against this VTN.

## Goals / Non-Goals

**Goals:**
- An event edited in place reaches the plan within one poll interval (plus the existing GB-55 hold for rate
  replans), for every event kind.
- One definition of "this event is new", used by the arrival trace, the history record and the replan trigger.

**Non-Goals:**
- Report obligations of an edited event (GB-67) and the fleet reaction feed's pick of the first arrival (GB-68).
- Removing the interval-count test for a rate change.
- Acting on a VTN that does not bump `modificationDateTime` on update (a spec violation): such an edit stays unseen
  until the next periodic cycle, as it is today.

## Decisions

**D1. Key the known events on `(id, modificationDateTime)`; a new version is an arrival.**
`detect_event_changes` takes the previous poll's known events as a map from id to version instead of a set of ids;
the arrival loop skips an event only when both match. An edited event is then traced as `OpenAdrArrived` with its
new version, written to `events_received`, and named in `trigger_causes`, through the code every new event already
takes. The decision the backlog row left open (rate events only, or every kind) is answered by construction: the
arrival path knows no kind.
Alternatives: (a) compare the parsed rates by value, as capacity is compared. It fixes tariffs only, leaves the
replan unattributed (no event id) and keeps "has something changed" decided in a second way next to the arrival.
(b) A new `OpenAdrModified` trace variant. It would need generated UI types, the Planner page's two renderers and
the BFF's trace reader for what the existing variant already carries (`modification_date_time` exists for this
reason, per its doc comment). Both rejected under one-concept-one-function.

**D2. The known events become one small type, `KnownEvents`, in its own file.**
`tasks/poll_events/known_events.rs` (split out like `capacity_picture.rs`, because `detect.rs` is near the `tasks/`
200-production-line cap) owns the map and two questions: `is_new(&event)` (id absent or version different) and
`ids()` (what `retire_obligations_not_in` and the expiry loop read). `EventChanges.current_ids` becomes
`EventChanges.known: KnownEvents` and the loop keeps `prev_known`. The expiry loop and `retire_obligations_not_in`
use `ids()`, so no caller builds an id set of its own.

**D3. Versions compare as instants; the trace keeps the VTN's text.**
The map holds the `DateTime<Utc>` from the wire object; any difference, earlier or later, is a new version (a VTN
restored from a backup may move it backwards). The trace entry keeps `to_rfc3339()` of the same value, as today.

**D4. An edit that changes nothing the VEN acts on still replans once.**
A new event of any kind replans today whether or not it changes a plan; an edit is treated alike. The alternative,
suppressing the trigger when the resolved rates, limits and windows are equal, is a second change detector, and the
cheap case it saves (an operator renaming an event) is rare. A hard trigger always adopts the new plan (the
acceptance gate only judges periodic plans), so a rate edit takes effect on adoption.

## Risks / Trade-offs

- [A capacity or alert edit now traces an arrival as well as the change entry it already produced] -> The trigger
  stays `CapacityChange` (it wins in `plan_trigger`) or `Alert`; the only visible difference is one more trace row
  and the event named as cause.
- [The first poll after a restart treats every event as arrived] -> Unchanged: the known map starts empty, as the id
  set did.
- [A flapping `modificationDateTime` replans every poll] -> It would be a VTN fault, and the trace shows the
  versions. No guard is added.
- [The fleet reaction feed shows the first arrival of an edited event per VEN] -> GB-68; the arrival rows are
  correct and the feed's choice of the first is the limit.
