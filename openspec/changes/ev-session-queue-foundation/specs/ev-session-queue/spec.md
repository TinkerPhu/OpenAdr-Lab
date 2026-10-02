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

### Requirement: A session declares what the following trip is expected to consume

A charging session SHALL carry the distance the vehicle is expected to travel after
its departure, so that the charge consumed between one session and the next is
stated rather than assumed.

Where the user does not state a distance, the EV's own configured default SHALL be
used, and the plan SHALL make clear that a default was applied rather than a stated
value. The vehicle itself SHALL be the only authority that converts a distance into
a state-of-charge drop: no route, interface or planner may do that arithmetic
independently.

#### Scenario: A stated distance determines the expected drop

- **WHEN** a user states a session whose following trip is 120 km, for a vehicle configured at 0.2 kWh/km with a 60 kWh pack
- **THEN** the expected drop after that departure is 40 % of the pack

#### Scenario: An unstated distance falls back to the vehicle's default, visibly

- **WHEN** a user states a session without a distance
- **THEN** the EV's configured default distance is used, and the plan reports that the value was defaulted rather than stated

#### Scenario: A simulated session carries the trip it was generated from

- **WHEN** the simulated usage schedule produces a session
- **THEN** that session's expected consumption is the one its own generated trip already describes, not the configured default

#### Scenario: Two sessions with no trip between them consume nothing

- **WHEN** a session's departure is immediately followed by the next session's window opening
- **THEN** no consumption is expected between them, whatever default is configured

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

### Requirement: A user states when the vehicle becomes available

The EV request surface SHALL let the user state when the vehicle becomes available
for the requested session, which is that session's charging-window start. Where the
user does not state it, the window SHALL open at the moment of submission.

#### Scenario: A stated availability time becomes the window start

- **WHEN** a user submits an EV session stating the vehicle is available from a future instant
- **THEN** the queued session's charging window opens at that instant

#### Scenario: An unstated availability time means now

- **WHEN** a user submits an EV session without stating an availability time
- **THEN** the queued session's charging window opens at the submission instant

#### Scenario: Stating availability after the deadline is refused

- **WHEN** a user submits an EV session whose stated availability time is at or after its departure
- **THEN** the submission is refused as an empty charging window and nothing is queued

### Requirement: A conflicting user submission is refused, and changes nothing

Where a submitted session's window overlaps one already queued, the submission SHALL
be refused, no session SHALL be created, and every existing session SHALL be left
unchanged. The refusal SHALL identify each session it overlaps, so that a caller can
describe the clash without inspecting the queue separately.

Nothing SHALL be removed to make room for a submission. Displacing a plan the user
previously committed to, without asking, is the failure this queue exists to prevent
- a standing weekly plan silently losing to a spontaneous one. Offering the
replacement instead is `ev-session-user-conflict-resolution`'s job; refusing is this
change's.

#### Scenario: An overlapping submission leaves the queue untouched

- **WHEN** a user submits a session whose window overlaps an existing queued session
- **THEN** the submission is refused, nothing is queued, and the existing session is unchanged

#### Scenario: The refusal names every clashing session

- **WHEN** a submission overlaps two queued sessions
- **THEN** the refusal identifies both

#### Scenario: Non-overlapping submissions are both kept

- **WHEN** a user submits a second session whose window does not overlap the first
- **THEN** both are queued, which is what lets a user hold several sessions at once

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
