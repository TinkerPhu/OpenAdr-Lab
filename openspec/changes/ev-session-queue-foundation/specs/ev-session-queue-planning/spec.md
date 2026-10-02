# Spec Delta

## Purpose

How the site planner consumes an EV's queue of charging sessions: each session
becomes its own obligation over its own charging window, and the state of charge
the vehicle loses while away between sessions is accounted for, so the plan can
pre-charge for a deadline that lies behind an intervening departure.

## ADDED Requirements

### Requirement: Every queued session becomes its own planning obligation

The planner SHALL receive one obligation per queued session that falls within the
planning horizon, each bounded by that session's own charging window. Charging
scheduled inside one session's window SHALL NOT count toward another session's
target.

#### Scenario: A plan satisfies two sessions in sequence

- **WHEN** two non-overlapping sessions are queued within the horizon, each with a firm target
- **THEN** the plan delivers each session's required energy inside that session's own window

#### Scenario: Charging after a departure does not satisfy that departure

- **WHEN** a session's window has closed and the plan charges in a later window
- **THEN** that later energy does not count toward the closed session's target

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

### Requirement: State of charge lost while away is carried between sessions

The energy each queued session requires SHALL be derived from the state of charge
the vehicle is expected to have when that session's window opens, which accounts
for the charge consumed during the preceding absence. The plan's projected state
of charge SHALL show each expected drop at the slot the vehicle returns.

#### Scenario: A later session requires the energy the trip consumed

- **WHEN** a session follows a departure that is expected to consume part of the pack
- **THEN** that session's required energy includes the consumed amount, so its target is still reached by its departure

#### Scenario: Pre-charging across an intervening departure

- **WHEN** a queued session's window alone cannot physically deliver its target, but energy is cheap before the preceding departure
- **THEN** the plan charges ahead of the earlier departure only up to what survives the intervening absence, and reports the remaining shortfall rather than promising the unreachable target

#### Scenario: The projected state of charge shows the drop

- **WHEN** the plan spans a predicted return
- **THEN** the projected state of charge falls at the return slot by the predicted amount, and never below the configured floor

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
user's stated willingness to pay at that state of charge. Sessions queued behind
it SHALL carry a firm energy-by-deadline obligation.

#### Scenario: The current session's bids still price its energy

- **WHEN** the head session carries a comfort curve and the queue holds further sessions behind it
- **THEN** the head session's energy is valued by that curve exactly as it is with a single session

#### Scenario: A queued session's target is guaranteed, not bid

- **WHEN** a session queued behind the head has a firm target
- **THEN** its required energy is delivered as a guarantee within its window, independent of any bid
