## ADDED Requirements

### Requirement: One interval-timing rule for every event parser
The VEN SHALL resolve the absolute start and end of every interval of an OpenADR event through a single shared function, and every event parser (price/capacity schedules, alert, SIMPLE, dispatch and charge-state windows, report activity) SHALL use it. No parser SHALL compute interval timing on its own.

#### Scenario: Parsers agree on an event's windows
- **WHEN** one event carries both an IMPORT_CAPACITY_LIMIT and a SIMPLE payload in the same intervals
- **THEN** the capacity schedule and the SIMPLE windows cover exactly the same start and end times

### Requirement: Spec-conformant interval timing
An interval's own `intervalPeriod` SHALL take precedence. An interval without its own start SHALL start where the previous interval ended, and the first interval without its own start SHALL start at `event.intervalPeriod.start`. An interval without its own duration SHALL last the event-level duration (OpenADR 3.1 User Guide §7.3). A start of `0001-01-01` SHALL mean "now" and a duration of `P9999Y` SHALL mean open-ended.

#### Scenario: Contiguous DOE intervals from an event-level period
- **WHEN** an event has `intervalPeriod {start: 2023-02-10T00:00Z, duration: PT30M}` and two intervals without their own periods carrying IMPORT_CAPACITY_LIMIT 10.0 and 8.0 (User Guide Example 8.10.1-1)
- **THEN** the first interval covers 00:00–00:30 with 10.0 kW and the second 00:30–01:00 with 8.0 kW

#### Scenario: A 48-interval day
- **WHEN** such an event has 48 intervals without their own periods
- **THEN** they cover 48 contiguous 30-minute intervals, one day

#### Scenario: Own period overrides, later intervals follow it
- **WHEN** an event with an event-level start of 10:00 and duration PT1H has interval 0 with its own period starting 12:00 for PT30M, followed by interval 1 without a period
- **THEN** interval 0 covers 12:00–12:30 and interval 1 covers 12:30–13:30

#### Scenario: Single interval with event-level period (unchanged)
- **WHEN** an event has an event-level period 10:00, PT10M and one interval without its own period
- **THEN** the interval covers 10:00–10:10

### Requirement: One rule for missing timing
When neither the interval nor the event gives a duration, the interval SHALL be open-ended. When no start can be derived, the interval SHALL be in force for as long as the VTN lists the event. Both rules SHALL be the same for every event type.

#### Scenario: Missing duration
- **WHEN** an event has an event-level start and no duration anywhere
- **THEN** its interval is in force from that start for as long as the VTN lists the event

#### Scenario: No timing at all
- **WHEN** an event carries an IMPORT_CAPACITY_LIMIT and no `intervalPeriod` at event or interval level
- **THEN** the limit is in force while the VTN lists the event
