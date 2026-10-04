# Spec Delta

## Purpose

Make the EV's plugged-in state visible over time: what was measured in the past and what
is predicted for the future. The EV charts on the Control and History pages shade the
periods in which the EV is unplugged, so the charging behaviour can be read against the
car's availability.

## ADDED Requirements

### Requirement: Past EV timeline points carry the measured plugged state
Every past EV point and the now-point that the timeline API returns SHALL carry
`values.plugged` in the range 0..1, where 1 means plugged in. The now-point SHALL carry 1
or 0. A point aggregated over a resampling bucket SHALL carry the time-weighted fraction of
that bucket during which the EV was plugged in.

#### Scenario: Unplugged EV reports zero
- **WHEN** the EV is unplugged (e.g. via sim inject `ev_plugged=false`) and the timeline is read after the next tick
- **THEN** the EV now-point has `values.plugged` equal to 0

#### Scenario: Bucket spanning an unplug
- **WHEN** a resampling bucket contains samples that are half plugged and half unplugged by time
- **THEN** that bucket's `values.plugged` is 0.5 (±0.01)

### Requirement: Future EV timeline points carry the predicted presence
Every future EV point that comes from a solved plan SHALL carry `values.plugged` = 1 when
the EV is predicted to be plugged in during that slot, and 0 when it isn't. The value SHALL
be the planner's own per-slot EV availability, the same one that forbids charging in a slot,
not a value recomputed elsewhere. That availability follows the live plug state, the stated
sessions (present inside a session's window, away after its departure until a stated
return) and the usage forecast where one is configured. A plan solved without an EV SHALL
NOT carry the key.

#### Scenario: Predicted absence is visible in the plan
- **WHEN** the usage forecast predicts the EV away during some slots of the horizon
- **THEN** the future EV points for exactly those slots carry `values.plugged` = 0, and the others carry 1

#### Scenario: A stated departure ends presence
- **WHEN** a plugged EV has a deadline session whose departure lies inside the horizon and no return is stated
- **THEN** the future EV points before the departure carry `values.plugged` = 1 and the points after it carry 0

#### Scenario: Plugged without a session
- **WHEN** the EV is plugged in, has no session and no usage forecast
- **THEN** every future EV point carries `values.plugged` = 1

#### Scenario: No usage forecast, unplugged now
- **WHEN** the EV is unplugged and has no usage forecast
- **THEN** every future EV point carries `values.plugged` = 0

#### Scenario: Future points carry plugged next to SoC
- **WHEN** a plan with an EV has been solved and `/timeline/ev` is read
- **THEN** at least one future point carries both `values.soc` and `values.plugged`

#### Scenario: Stored plans without the field still load
- **WHEN** a plan snapshot persisted before this change is deserialized
- **THEN** it loads without error, and its EV points have no `plugged` key

### Requirement: Persisted history records the plugged fraction
The persisted EV history SHALL record, per sampling window, `plugged` as the mean of the
EV's plugged state over that window (0..1). Rows for assets without a plugged state SHALL
carry `null`. `/history/ticks` SHALL return the field on every row.

#### Scenario: Fully plugged minute
- **WHEN** the EV is plugged in throughout a sampling window
- **THEN** the persisted EV row for that window has `plugged` = 1

#### Scenario: Asset without plugged state
- **WHEN** a PV or battery row is persisted
- **THEN** its `plugged` is `null`

#### Scenario: Pre-existing rows after upgrade
- **WHEN** the history store is opened after the schema migration
- **THEN** rows written before the migration return `plugged: null` and the store keeps working

### Requirement: EV charts shade the unplugged periods
The EV chart on the Control page and on the History page SHALL draw a background band,
behind the chart's lines, across each time range where `plugged` < 1, with opacity
proportional to `1 − plugged`. A range where `plugged` is 1 SHALL have no band. A range
where `plugged` is absent SHALL have no band. Bands for future (predicted) points SHALL be
visually distinct from bands for past (measured) points. A band SHALL NOT extend across a
gap in the data that is much longer than the sample spacing.

#### Scenario: Control chart shows measured and predicted bands
- **WHEN** the Control page renders the EV cell, with past points unplugged and future points predicted away
- **THEN** the EV chart shows an "Unplugged" band before the now line and a visually distinct "Predicted away" band after it

#### Scenario: Plugged EV has no band
- **WHEN** every point of the EV chart carries `plugged` = 1
- **THEN** the chart shows no band

#### Scenario: History chart shades the time away
- **WHEN** the History page renders a day during which the EV was unplugged for a period
- **THEN** the EV chart shows a band during that period and none before and after it

#### Scenario: Partial bucket is lighter
- **WHEN** a point carries `plugged` = 0.5
- **THEN** its band is drawn at half the opacity of a fully unplugged band

#### Scenario: Rows without a value draw nothing
- **WHEN** the History page renders EV rows whose `plugged` is `null`
- **THEN** the chart shows no band for them

#### Scenario: Data gap breaks the band
- **WHEN** two unplugged points are separated by a gap of hours with no rows
- **THEN** the band does not span the gap

### Requirement: PV curtailment shading is unchanged
The PV chart SHALL keep the existing curtailment shading (hardware, planned, unplanned) with
the same classification and colours after the overlay mechanism is generalized.

#### Scenario: PV curtailment still shaded
- **WHEN** the Control page renders a PV cell whose past points show a plan-commanded generation limit
- **THEN** the PV chart shows the planned-curtailment band exactly as before this change
