## ADDED Requirements

### Requirement: The capacity-limit envelope is sampled into long-term history
The VEN SHALL sample the currently-applicable import/export capacity limit (the Dynamic Operating
Envelope, resolved the same way `GET /capacity/schedule` resolves it) into the same 1-minute
downsample window used for tariffs, and persist it into the site-wide grid history record
alongside tariff/CO2 data. "No limit" SHALL be represented as absence (no value), never as a
sentinel number such as zero or infinity.

#### Scenario: A capacity limit active during a window is recorded
- **WHEN** an import or export capacity limit was applicable at any point during a past downsample
  window
- **THEN** querying grid history for that window returns a non-null limit value for the direction
  (import/export) that was constrained

#### Scenario: An unconstrained window has no limit recorded
- **WHEN** no capacity limit was applicable at any point during a downsample window
- **THEN** the persisted grid-history row for that window has an absent (null) limit value for
  both import and export — not zero, not any other sentinel

### Requirement: A brief capacity-limit event within a window is not averaged away
Within a downsample window, the persisted limit value SHALL be the tightest (most restrictive)
value observed at any point during the window, not a mean or a last-value-wins sample — so a
capacity event that starts or ends partway through the window is never masked by the rest of the
window being unconstrained.

#### Scenario: A brief mid-window limit is not diluted by an otherwise-unconstrained window
- **WHEN** a downsample window has no active capacity limit for most of its duration but a
  strictly tighter limit becomes applicable partway through
- **THEN** the window's persisted limit value equals the tighter value observed during the
  constrained portion, not a value averaged with the unconstrained portion

#### Scenario: Multiple distinct limit values within one window keep the tightest
- **WHEN** a downsample window observes more than one applicable capacity-limit value for the same
  direction (e.g. the schedule's applicable interval changes mid-window)
- **THEN** the window's persisted limit value for that direction is the tightest (most
  restrictive, i.e. lowest) of the values observed

### Requirement: Persisted capacity-limit history survives a restart and is queryable by range
The persisted import/export capacity-limit values SHALL be retrievable via the existing grid
history query path (the same endpoint tariff history is already served through), for any past
time range, after a VEN restart.

#### Scenario: A past capacity-limit event survives a restart
- **WHEN** a capacity limit was recorded for a past downsample window, and the VEN is later
  restarted
- **THEN** querying grid history for that window still returns the recorded limit value

#### Scenario: A query spanning a mix of constrained and unconstrained windows returns both correctly
- **WHEN** a grid-history query range includes some windows with a recorded capacity limit and
  some without
- **THEN** the response distinguishes constrained windows (non-null limit) from unconstrained ones
  (null limit) for each window in the range

### Requirement: The History tab renders the persisted envelope with the same chart split as the live Controller view
The History tab SHALL render the persisted capacity-limit envelope using the same direct-signal
chart (tariff + envelope) that the Controller tab uses for the live view, rather than a separate
implementation, and SHALL NOT show the envelope as a hardcoded absent value once real data exists
for the queried range.

#### Scenario: A past capacity-limit event is visible on the History tab
- **WHEN** a user views the History tab for a date range that includes a recorded capacity-limit
  event
- **THEN** the tariff/envelope diagram shows the import or export limit for the affected interval,
  not a flat absent value

#### Scenario: A date range with no capacity-limit events shows none, not an error
- **WHEN** a user views the History tab for a date range with no recorded capacity-limit events
- **THEN** the envelope series renders as absent for that range without an error or placeholder
  value standing in for real data
