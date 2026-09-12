## ADDED Requirements

### Requirement: Overlapping event intervals resolve by priority per time segment
The VEN SHALL resolve event intervals that overlap in time into non-overlapping segments split at
every interval boundary, and for each segment and payload type SHALL apply the value of the
highest-priority event covering that segment, regardless of whether the overlapping intervals
share the same start and end.

#### Scenario: Intra-hour high-priority price inside a day-ahead hourly price
- **WHEN** a priority-5 PRICE event defines 0.09 €/kWh for 05:00–06:00 and a priority-1 PRICE event defines 0.45 €/kWh for 05:20–05:30
- **THEN** the resolved schedule is 05:00–05:20 at 0.09, 05:20–05:30 at 0.45 and 05:30–06:00 at 0.09

#### Scenario: Lower-priority event starting first no longer wins
- **WHEN** a priority-5 event covers 05:00–06:00 and a priority-1 event covers 05:29–05:59 for the same payload type
- **THEN** every instant in 05:29–05:59 resolves to the priority-1 value

#### Scenario: Short interval does not leak past its end
- **WHEN** a high-priority interval ends inside a longer lower-priority interval
- **THEN** the lower-priority value applies from the end of the high-priority interval until the end of the longer interval

### Requirement: Ranking of conflicting events
The VEN SHALL rank conflicting events by lower `priority` number first, SHALL rank an event without
`priority` below every event with one, SHALL break equal priority in favour of the newer
`createdDateTime` (absent or unparseable counts as oldest), and SHALL break any remaining tie in
favour of the event appearing later in the input.

#### Scenario: Equal priority, newer event wins the overlap
- **WHEN** two equal-priority PRICE events overlap partially and differ in `createdDateTime`
- **THEN** the overlapping segment takes the newer event's value and each non-overlapping remainder keeps its own event's value

#### Scenario: Absent priority ranks lowest across a partial overlap
- **WHEN** an event without `priority` overlaps partially with an event with `priority` 9
- **THEN** the overlapping segment takes the priority-9 event's value

### Requirement: Resolution is per payload type
The VEN SHALL resolve each payload type independently, taking each type's value from the
highest-ranked event covering the segment that carries that type.

#### Scenario: Price and GHG from different events
- **WHEN** a priority-1 event carrying only PRICE overlaps a priority-5 event carrying PRICE and GHG
- **THEN** the overlapping segment has PRICE from the priority-1 event and GHG from the priority-5 event

### Requirement: Non-overlapping input is unchanged
The VEN SHALL produce exactly the input intervals and values when no two event intervals overlap.

#### Scenario: Single event passes through
- **WHEN** only one event with three consecutive hourly intervals is active
- **THEN** the resolved schedule contains exactly those three intervals with their values

### Requirement: All consumers read the same resolved value
Every consumer SHALL derive its value from the same resolved, non-overlapping schedule — the
tick-time cost calculation, the recorded history tariff and limit columns, the planner's tariff
series and the planned capacity limits — so that for any instant they use the same value.

#### Scenario: Planner and recorded tariff agree under overlap
- **WHEN** the resolved schedule contains a 0.45 €/kWh segment from 05:20 to 05:30 inside a 0.09 €/kWh hour
- **THEN** the planner's tariff series value at 05:25 is 0.45 and at 05:35 is 0.09, matching the value the tick-time lookup returns at those instants

### Requirement: The same resolution applies to capacity limits
The VEN SHALL apply the same segment and priority resolution to IMPORT_CAPACITY_LIMIT and
EXPORT_CAPACITY_LIMIT intervals in the planned capacity schedule; the live strictest-limit
capacity state SHALL remain unchanged.

#### Scenario: Short high-priority capacity limit inside a longer one
- **WHEN** a priority-5 IMPORT_CAPACITY_LIMIT of 6 kW covers 18:00–22:00 and a priority-1 limit of 2 kW covers 19:00–19:30
- **THEN** the planned capacity schedule is 18:00–19:00 at 6 kW, 19:00–19:30 at 2 kW and 19:30–22:00 at 6 kW
