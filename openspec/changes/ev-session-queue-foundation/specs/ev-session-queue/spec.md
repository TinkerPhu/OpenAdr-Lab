# Spec Delta

## Purpose

An EV asset holds an ordered queue of non-overlapping charging sessions rather
than a single current obligation, so that a sequence of upcoming departures —
each with its own deadline and target state of charge — can be stated, kept
consistent, expired and cancelled independently of one another.

## ADDED Requirements

### Requirement: An EV holds an ordered queue of charging sessions

The system SHALL hold, per EV asset, zero or more charging sessions ordered by
charging-window start. Each session SHALL declare the instant its charging
window opens (when the vehicle becomes available for that session), the instant
it closes (the departure the session must be ready for), and the target state of
charge to reach by that departure.

#### Scenario: Multiple upcoming sessions are retained

- **WHEN** two sessions with non-overlapping charging windows are stored for one EV
- **THEN** both are retained, ordered by window start, and both are readable

#### Scenario: The current session is identifiable

- **WHEN** the queue holds sessions and the present instant falls inside the first session's charging window
- **THEN** that session is reported as the EV's current session

#### Scenario: A session whose window has not opened yet is still queued

- **WHEN** the queue holds a session whose charging window opens in the future
- **THEN** it is readable as a queued session and is not reported as the current session

### Requirement: Queued sessions never overlap

The queue SHALL never contain two sessions whose charging windows overlap in
time. Two sessions overlap when one's window start falls strictly before the
other's departure and that other's window start falls strictly before the
first's departure. Sessions that merely touch — one's departure exactly equal to
the next one's window start — SHALL NOT be treated as overlapping.

#### Scenario: Touching sessions are both accepted

- **WHEN** a session is inserted whose charging window starts exactly at an existing session's departure
- **THEN** both sessions are present in the queue

#### Scenario: Overlapping insertion is reported as a conflict

- **WHEN** an insertion is attempted whose charging window overlaps an existing session's window
- **THEN** the conflict is reported to the caller, naming every session it overlaps

#### Scenario: Insertion order does not matter

- **WHEN** a session whose window precedes every existing session is inserted
- **THEN** it is placed at the head of the queue and ordering by window start still holds

### Requirement: A stated session outranks a simulated one

A session the user stated SHALL take precedence over one the simulated usage
schedule produced. The simulated schedule SHALL NOT displace, modify or remove a
user-created session, and SHALL NOT insert a session that would conflict with one.

Only two kinds of session exist: those a user stated, and those the simulated
usage schedule produced to stand in for a user. A VTN SHALL NOT create an EV
charging session - a grid command about an asset's state of charge is an external
constraint, not the driver's intent about their own travel (see R-100 in
`docs/reference/TECHNICAL_DEBTS.md` for the decision this leaves open).

#### Scenario: Simulated schedule yields to a user session

- **WHEN** the simulated usage schedule would insert a session conflicting with a user-created session
- **THEN** no simulated session is inserted and the user session is left unchanged

#### Scenario: Simulated sessions fill the gaps around a user session

- **WHEN** the simulated usage schedule has several sessions to place and one of them conflicts with a user session
- **THEN** the conflicting one is skipped and the non-conflicting ones are inserted

### Requirement: Passed sessions leave the queue

A session whose departure instant has passed SHALL be removed from the queue, so
that a finished or missed session never continues to act as an obligation or to
suppress opportunistic charging.

#### Scenario: A passed session is dropped

- **WHEN** the present instant advances past the head session's departure
- **THEN** that session is no longer in the queue and the next session becomes the head

#### Scenario: Several passed sessions are dropped at once

- **WHEN** the present instant advances past the departure of more than one queued session
- **THEN** all of them are removed and the first session with a future departure becomes the head

#### Scenario: Charging is not suppressed once the queue empties

- **WHEN** the last session's departure passes and no session remains
- **THEN** the EV is reported as having no active session and opportunistic charging is no longer paused

### Requirement: Cancelling a request removes only its own session

Cancelling a user request that owns an EV session SHALL remove exactly that
session from the queue, leaving every other queued session in place.

#### Scenario: One of several sessions is cancelled

- **WHEN** a user request owning the second of three queued sessions is cancelled
- **THEN** that session is removed and the other two remain queued

### Requirement: The queue is visible

Every queued session SHALL be observable through the EV's own session read
surface and, where a session was created by a user request, through that
request's representation. A request SHALL resolve to the session it owns, not to
whichever session is currently at the head of the queue.

#### Scenario: Each request shows its own session

- **WHEN** two user requests each own a different queued EV session
- **THEN** each request's representation carries its own session's target and departure

#### Scenario: The full queue is readable

- **WHEN** the EV session read surface is queried while several sessions are queued
- **THEN** every queued session is returned in window order
