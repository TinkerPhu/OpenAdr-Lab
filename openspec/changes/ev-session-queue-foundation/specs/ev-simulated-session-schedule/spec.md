# Spec Delta

## Purpose

The simulated usage schedule maintains a rolling week of simulated charging
sessions derived from the EV's own trip pattern, so the planner sees the whole
upcoming week of departures rather than only the next one.

## ADDED Requirements

### Requirement: A rolling seven-day schedule of simulated sessions

While the simulated usage class is active and charge planning is engaged, the
system SHALL maintain simulated charging sessions covering every predicted trip
whose departure falls within seven days of the present instant. The schedule
SHALL be topped up as time advances, so that the covered span stays a rolling
seven days rather than a one-off fill.

#### Scenario: A week of trips is queued

- **WHEN** the simulated usage class is active with charge planning engaged and the EV's pattern predicts a departure on each of the next seven days
- **THEN** a simulated session is queued for each of those departures

#### Scenario: The window advances with time

- **WHEN** the present instant advances by a day and the head session's departure has passed
- **THEN** the passed session is gone and a session for the newly-in-range eighth day is queued

#### Scenario: A day without a predicted trip is skipped

- **WHEN** the EV's pattern predicts no departure on a given day
- **THEN** no simulated session exists for that day and the surrounding days' sessions are unaffected

#### Scenario: Repeated evaluation is idempotent

- **WHEN** the schedule is evaluated repeatedly at the same instant
- **THEN** the queued simulated sessions are unchanged, with no duplicates added

### Requirement: Each simulated session spans its own availability window

A simulated session's charging window SHALL open when the vehicle is predicted to
be home and available — at the return from the preceding trip, or at the present
instant for the session covering the imminent departure — and SHALL close at the
predicted departure.

#### Scenario: A session's window starts at the previous return

- **WHEN** two consecutive predicted trips are queued as sessions
- **THEN** the later session's charging window opens at the earlier trip's predicted return

#### Scenario: The imminent session starts now

- **WHEN** the vehicle is home and the next predicted departure is the first in range
- **THEN** that session's charging window opens at the present instant

### Requirement: The simulated schedule remains deterministic

For a given EV configuration and seed, the simulated schedule SHALL be
reproducible: evaluating it at the same instant SHALL always yield the same set
of session windows and targets.

#### Scenario: Same instant, same schedule

- **WHEN** the schedule is built twice for the same configuration, seed and instant
- **THEN** the resulting session windows, departures and targets are identical

### Requirement: The simulated schedule writes no sessions under the forecast class

Where the EV declares the forecast usage class, the simulated schedule SHALL
write no sessions at all; predicted departures reach the planner directly without
becoming stored sessions.

#### Scenario: Forecast class leaves the queue empty

- **WHEN** the EV declares the forecast usage class and charge planning is engaged
- **THEN** no simulated sessions are queued, and the plan still avoids charging during predicted trips

### Requirement: The simulated schedule never conflicts with stated sessions

The simulated schedule SHALL only add sessions that do not conflict with any
user- or VTN-created session, and SHALL leave such sessions untouched.

#### Scenario: A user session blocks one simulated day

- **WHEN** a user-created session covers a window a predicted trip would also claim
- **THEN** no simulated session is queued for that trip, the user session is unchanged, and other days' simulated sessions are still queued
