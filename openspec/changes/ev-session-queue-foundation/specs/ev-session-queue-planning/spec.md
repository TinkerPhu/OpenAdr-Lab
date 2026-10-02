# Spec Delta

## Purpose

How the site planner consumes an EV's queue of charging sessions: each session
becomes its own obligation over its own charging window, and the state of charge
the vehicle loses while away between sessions is accounted for, so the plan can
pre-charge for a deadline that lies behind an intervening departure.

## ADDED Requirements

### Requirement: Every queued session with a firm target becomes its own planning obligation

The planner SHALL receive one charging obligation per queued session that states a
**firm** target and whose departure falls within the planning horizon, carrying
that session's departure and its target state of charge, and identifying the
session it came from.

A session with a **soft** deadline SHALL contribute no obligation: a soft target
is a preference priced per unit of energy by the user's comfort curve, not a
guarantee, so it is expressed through that valuation instead. The same holds for
the free and opportunistic modes, which are gated by available surplus rather than
by a deadline. Those sessions still bound when the vehicle is present, because
availability is fact rather than preference.

#### Scenario: A plan satisfies two sessions in sequence

- **WHEN** two non-overlapping sessions are queued within the horizon, each with a firm target
- **THEN** the plan delivers each session's required energy inside that session's own window

#### Scenario: Each obligation is attributable to its session

- **WHEN** several sessions with firm targets are queued within the horizon
- **THEN** each resulting obligation identifies the session it came from

#### Scenario: A soft session states no obligation

- **WHEN** a queued session has a soft deadline
- **THEN** it contributes no obligation, and how far it charges is decided by its comfort curve

#### Scenario: Sessions beyond the horizon are ignored

- **WHEN** a queued session's charging window opens after the end of the planning horizon
- **THEN** it contributes no obligation to this planning cycle

### Requirement: The vehicle is not chargeable while away

Charging power SHALL be zero in every slot in which the vehicle is predicted or
stated to be absent, including the gaps between consecutive sessions' charging
windows. An obligation SHALL never make an absent slot chargeable.

#### Scenario: The gap between two sessions carries no charging

- **WHEN** the plan spans two queued sessions separated by a departure
- **THEN** no charging is scheduled between the first session's departure and the second session's window start

#### Scenario: A stated target cannot override predicted absence

- **WHEN** a session's window would include a slot the EV's own schedule predicts the vehicle is away for
- **THEN** that slot remains unchargeable

### Requirement: A session's target is reached using the vehicle's whole available history

Each queued session's target SHALL be served by any charging the vehicle can
physically retain until that session's deadline, including charging that happened
before an earlier departure. The charge the vehicle is expected to consume while
away SHALL be accounted for between sessions.

#### Scenario: A later session is served by charging before an earlier departure

- **WHEN** a queued session's own window cannot deliver its target, but charging before the preceding departure can, and that charge survives the trip
- **THEN** the plan charges before the earlier departure and the later session's target is met

#### Scenario: The consumed charge is accounted for

- **WHEN** a session follows a departure expected to consume part of the pack
- **THEN** the plan accounts for that consumption when serving the session's target, rather than assuming the vehicle returns as it left

### Requirement: An unreachable obligation degrades visibly, never infeasibly

Where a queued session's target cannot be reached within its own window, the plan
SHALL charge as far as the window allows and report the shortfall for that
session. A queued session SHALL NOT make the site plan infeasible.

#### Scenario: An impossible queued target still yields a plan

- **WHEN** a queued session demands more energy than its window can physically deliver
- **THEN** a plan is produced, charging is maximised within that window, and the shortfall is reported against that session

#### Scenario: The shortfall names the session

- **WHEN** one of several queued sessions falls short
- **THEN** the reported shortfall identifies which session it belongs to

### Requirement: The head session keeps its stated valuation

The session whose charging window is currently open SHALL continue to be valued
by the comfort curve its request carried, with each unit of energy priced at the
user's stated willingness to pay at that state of charge. A session queued behind
it SHALL carry an obligation only where its target is firm; a soft one behind the
head is still a preference, and contributes neither an obligation nor a second
priced curve.

#### Scenario: The current session's bids still price its energy

- **WHEN** the head session carries a comfort curve and the queue holds further sessions behind it
- **THEN** the head session's energy is valued by that curve exactly as it is with a single session

#### Scenario: A queued session's target is guaranteed, not bid

- **WHEN** a session queued behind the head has a firm target
- **THEN** its required energy is delivered as a guarantee within its window, independent of any bid
