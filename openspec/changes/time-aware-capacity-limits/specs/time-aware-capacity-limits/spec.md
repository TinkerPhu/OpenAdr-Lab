## ADDED Requirements

### Requirement: One capacity-limit lookup
The VEN SHALL answer "which import (or export) limit applies" through one shared function over the priority-resolved capacity schedule: the tightest limit among the schedule segments overlapping a span, where an instant is a zero-width span covered by segments with start ≤ t < end. The planner, the in-force-now capacity state, the history sampler and the arbiter's limit pass SHALL all use it.

#### Scenario: Instant inside a limit interval
- **WHEN** the schedule has an import limit of 3.0 kW for 10:00–11:00 and the lookup asks for 10:30
- **THEN** it returns 3.0 kW

#### Scenario: Span overlapping two limits
- **WHEN** the schedule has 5.0 kW for 10:00–10:40 and 2.0 kW for 10:40–11:00, and the lookup asks for the span 10:00–11:00
- **THEN** it returns 2.0 kW

#### Scenario: No limit
- **WHEN** no segment overlaps the requested span
- **THEN** it returns no limit

### Requirement: Planner caps each slot by the schedule
The planner SHALL cap each slot's import (and export) at the tightest scheduled limit overlapping that slot, combined with the physical limit and the subscription/reservation allowance, instead of one limit for the whole horizon. SIMPLE level 1 SHALL take its fraction of the slot's cap. Alerts SHALL keep capping overlapping slots at 0.

#### Scenario: A limit announced in advance
- **WHEN** at 14:00 the VEN holds an event with IMPORT_CAPACITY_LIMIT 0.0 kW for 16:00–17:00
- **THEN** the plan's `import_cap_kw` is 0.0 only in slots overlapping 16:00–17:00, and slots before 16:00 and after 17:00 are capped only by the physical limit

#### Scenario: The seed's ev-charge-pause event
- **WHEN** ven-2 holds an event with 0.0 kW for 16:00–17:00 and 7.4 kW for 17:00–18:00
- **THEN** slots in 16:00–17:00 are capped at 0.0 kW, slots in 17:00–18:00 at 7.4 kW, and no other slot is capped by it

### Requirement: Capacity state means "in force now"
`OadrCapacityState.import_limit_kw` and `export_limit_kw` (and their source event ids) SHALL be the scheduled limits in force at the poll's time. A limit that has not started yet or has ended SHALL NOT appear there, and overlapping limits SHALL resolve by event priority, then creation time, like the schedule.

#### Scenario: Future limit not in force
- **WHEN** the VEN holds an event whose only interval starts two hours from now
- **THEN** `GET /capacity` reports no import limit, and the Dashboard shows none

#### Scenario: Limit starts
- **WHEN** the first poll after that interval's start runs
- **THEN** `GET /capacity` reports the limit, and a `CapacityChange` controller event is logged

#### Scenario: Priority, not strictest
- **WHEN** two events overlap now, a priority-1 event with 10.0 kW and a priority-5 event with 3.0 kW
- **THEN** the limit in force is 10.0 kW

### Requirement: Execution never enforces a limit before it starts
The PV generation-limit resolver, the simulated grid asset and the arbiter's limit pass SHALL only apply a capacity limit whose interval covers the current time (the limit pass at tick time, the others at the latest poll).

#### Scenario: Future export limit
- **WHEN** an EXPORT_CAPACITY_LIMIT of 0.5 kW is scheduled from one hour from now
- **THEN** PV generation is not limited by it now
