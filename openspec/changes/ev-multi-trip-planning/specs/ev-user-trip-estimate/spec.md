# Spec Delta

## Purpose

Lets a user tell the planner what an upcoming trip will cost the battery and when the vehicle
will be back, so the plan can account for that consumption — while guaranteeing that a user who
says nothing has nothing invented on their behalf.

## ADDED Requirements

### Requirement: A trip estimate is optional

A user stating a charging session SHALL NOT be required to estimate the trip that follows it. A
session carrying no trip estimate SHALL be accepted.

#### Scenario: A session with no estimate is accepted

- **WHEN** a user submits a charging session stating only an availability window, a departure
  time and a target state of charge
- **THEN** the session is accepted and queued

### Requirement: Without an estimate, no consumption is projected

When a session carries no trip estimate, the planner SHALL project no state-of-charge drop for
the trip that follows it. The planned state of charge SHALL hold at its last planned value until
the vehicle's real return is measured. The planner SHALL NOT substitute a configured or inferred
distance, duration or consumption.

#### Scenario: The plan holds flat across an unestimated trip

- **WHEN** a session with no trip estimate departs inside the planning horizon
- **THEN** the planned state-of-charge curve shows no drop after that departure
- **AND** the plan reports no assumed trip consumption

#### Scenario: The measured return corrects the plan

- **WHEN** the vehicle returns from an unestimated trip and its measured state of charge is lower
  than the plan projected
- **THEN** the next planning cycle starts from the measured state of charge

### Requirement: An estimate is distance and return time together

A trip estimate SHALL consist of both an expected distance and an expected return time. A session
stating one without the other SHALL be refused, naming which part is missing. A distance without
a return time carries consumption with no instant to apply it to, and a return time without a
distance carries an instant with no consumption.

#### Scenario: A complete estimate is accepted

- **WHEN** a user submits a session stating both an expected trip distance and an expected return
  time
- **THEN** the session is accepted
- **AND** the plan projects that trip's consumption at the stated return time

#### Scenario: A distance with no return time is refused

- **WHEN** a user submits a session stating an expected trip distance but no expected return time
- **THEN** the submission is refused with an error naming the missing return time
- **AND** no session is queued

#### Scenario: A return time with no distance is refused

- **WHEN** a user submits a session stating an expected return time but no expected trip distance
- **THEN** the submission is refused with an error naming the missing distance
- **AND** no session is queued

### Requirement: Each session's estimate stands alone

A session's trip estimate SHALL be interpreted using only that session's own stated values. It
SHALL NOT depend on the presence, absence or content of any other session. A single queued
session's estimate SHALL be honoured, as SHALL the last session's in a queue of any length.

#### Scenario: A lone session's estimate is honoured

- **WHEN** exactly one session is queued and it states a complete trip estimate
- **THEN** the plan projects that trip's consumption at the stated return time

#### Scenario: The final session's estimate is honoured

- **WHEN** several sessions are queued and the last of them states a complete trip estimate
- **THEN** the plan projects that trip's consumption at the stated return time

### Requirement: The vehicle converts distance to energy

Converting an estimated distance into an expected state-of-charge drop SHALL be performed by the
EV asset alone, from its own configured consumption rate and battery capacity. No route,
interface, planner or user-facing surface SHALL compute that conversion.

#### Scenario: The same distance costs differently on different vehicles

- **WHEN** the same estimated distance is stated for two EVs with different consumption rates or
  battery capacities
- **THEN** the projected state-of-charge drops differ accordingly

## REMOVED Requirements

### Requirement: A missing trip distance falls back to a configured default

**Reason**: The fallback invented consumption the user never stated, and surfacing it as
"assumed" did not make it true — a plan is better holding flat and being corrected by the real
return than projecting a trip nobody described. With the fallback gone, there is nothing for a
default distance to serve.

**Migration**: Remove `default_trip_distance_km` from EV profile configuration; it is no longer
accepted. `consumption_kwh_per_km` is unchanged and still required. Users who relied on the
default to see a drop in their plan should state an expected distance and return time on the
session.
