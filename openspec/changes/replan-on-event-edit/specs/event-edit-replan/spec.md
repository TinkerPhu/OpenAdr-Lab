# Spec Delta

## Purpose
When the VEN's event poll counts an event as new, so that a tariff, limit or signal the VTN edited in place reaches
the plan within a poll interval, whatever the event kind.

## ADDED Requirements

### Requirement: An event is new when its id or its version is new
The VEN SHALL identify a polled event by its `id` together with its `modificationDateTime`. An event whose id was
not in the previous poll, or whose id was but whose `modificationDateTime` differs from the previous poll's, SHALL
count as newly arrived.

#### Scenario: A price edited in place with the same number of intervals
- **WHEN** the VTN updates a `PRICE` event so that every interval carries a different price and the event keeps
  its id and its interval count
- **THEN** the next poll traces `OpenAdrArrived` for that event with the new `modificationDateTime` and requests a
  replan

#### Scenario: A capacity limit edited in place
- **WHEN** the VTN updates an `IMPORT_CAPACITY_LIMIT` event to a different limit
- **THEN** the next poll requests a replan, and the replan names the edited event

#### Scenario: An alert window edited in place
- **WHEN** the VTN extends the interval of an alert event it already published
- **THEN** the next poll counts the event as newly arrived

### Requirement: A poll that sees nothing new does nothing
A poll in which every polled event has the id and `modificationDateTime` of the previous poll, and no event
disappeared, SHALL NOT trace an arrival and SHALL NOT request a replan for events.

#### Scenario: The same events twice
- **WHEN** two consecutive polls return the same events with the same versions
- **THEN** the second poll adds no trace entry and requests no replan

### Requirement: The replan names the version it acted on
The trace entry for a newly arrived event SHALL carry the event's id and the `modificationDateTime` as the VTN
wrote it, and the plan cycle that follows SHALL list the event id among its causes. The received-events history
SHALL record each version.

#### Scenario: Two versions of one event
- **WHEN** an event is published, then edited, and the VEN polls after each
- **THEN** the trace shows two arrivals of the same id with different `modification_date_time`, and the history
  holds two received records for it

### Requirement: A deleted event still expires
An event that is absent from the poll after it was present SHALL be traced as expired, whether or not it was
edited before.

#### Scenario: Edited, then deleted
- **WHEN** an event is edited and later deleted
- **THEN** the poll after the deletion traces `OpenAdrExpired` for its id
