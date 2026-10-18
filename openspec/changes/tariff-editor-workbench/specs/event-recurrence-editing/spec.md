## ADDED Requirements

### Requirement: Recurrence is the event's duration and nothing else
The workbench SHALL express how a series repeats only through `event.duration`, and SHALL NOT
introduce a private field, a changed `intervalPeriod.duration`, or a client-side expansion that
is sent instead of the pattern.

#### Scenario: Forever
- **WHEN** the user sets recurrence to "forever" on a 24-interval daily series and publishes
- **THEN** the event carries the 24 intervals once and `duration` equal to `P9999Y`

#### Scenario: Repeat for a bounded span
- **WHEN** the user sets recurrence to "repeat for 7 days" and publishes
- **THEN** the event carries the pattern once and `duration` equal to `P7D`

#### Scenario: Once
- **WHEN** the user sets recurrence to "once"
- **THEN** the published event has no `duration`

### Requirement: An existing event's recurrence is read back
Loading an event into the draft SHALL map its `duration` to once, repeat-for-span or forever, so
that load followed by publish with no edit reproduces the same `duration`.

#### Scenario: Round trip of the seeded tariff
- **WHEN** the event `tou-pricing-day-ahead` (`duration = P9999Y`, 24 hourly intervals) is loaded
- **THEN** the recurrence control shows "forever" and the grid shows 24 rows, not an expansion

#### Scenario: Duration shorter than the pattern
- **WHEN** a loaded event's `duration` is shorter than its interval sequence
- **THEN** the draft shows "truncated to <duration>" and says that the surplus intervals are
  ignored by receivers

### Requirement: The pattern period is visible and valid
For a repeating series the draft SHALL show the pattern period (first interval start to last
interval end) and SHALL refuse to publish a repeating series whose intervals are not contiguous
or whose period is not positive, because receivers shift the whole pattern by that period.

#### Scenario: Gap in a repeating pattern
- **WHEN** a "forever" series has no interval covering 03:00–04:00 between two intervals
- **THEN** publishing is blocked with a message naming the gap

#### Scenario: Daily pattern
- **WHEN** a pattern starts at midnight and ends 24 h later
- **THEN** the draft states "repeats every 24 h"

### Requirement: Preview of a repeating series is bounded and labelled
The preview SHALL draw the repetition within the chart window and mark that the series repeats,
rather than drawing an unbounded number of intervals. The expansion shown SHALL be the one
receivers compute.

#### Scenario: Forever series in a 48 h window
- **WHEN** a "forever" daily series is previewed over a 48 h chart window
- **THEN** both days are drawn with the same prices and the series is labelled "repeats forever"

### Requirement: Fixed-period repetition is declared against local time
Because receivers repeat the pattern by a fixed elapsed period, a preview window that crosses a
daylight-saving change SHALL carry a visible note that the repetition does not follow local
clock time.

#### Scenario: Window across the October clock change
- **WHEN** a "forever" daily series authored at Europe/Zurich midnight is previewed across the
  autumn DST change
- **THEN** the preview shows the pattern shifted by one local hour after the change and the note
  is displayed
