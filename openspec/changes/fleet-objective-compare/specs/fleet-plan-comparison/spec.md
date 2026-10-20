## ADDED Requirements

### Requirement: The plan can be read as accumulated energy
The fleet plan chart SHALL offer the planned import and export as accumulated energy (kWh from the start of
the plan, one running sum per direction) as well as power, SHALL open on accumulated energy, and SHALL use the
same step-to-energy conversion as the delta table.

#### Scenario: Accumulated view
- **WHEN** the plan chart is shown with a plan of 24 hourly slots of 10 kW import
- **THEN** the accumulated import reaches 10 kWh after the first slot and 240 kWh after the last

#### Scenario: Reference starts level
- **WHEN** a reference plan is drawn with the current one in the accumulated view
- **THEN** both curves start at zero at the same instant, so their gap is what the change added

### Requirement: An objective switch is compared like a published tariff
The Fleet page SHALL show, for the newest objective switch, how many VENs have replanned since it, the delta
in planned import and export energy and peak over the window both plans cover, and SHALL mark the delta
provisional until every VEN has replanned. The reference SHALL be the plan as of just before the first VEN of
that switch changed.

#### Scenario: A fleet switch in progress
- **WHEN** a fleet switch changed 3 of 5 VENs 4 minutes apart and 2 of them have replanned
- **THEN** the page shows "2 of 5 VENs replanned" and marks the delta provisional

#### Scenario: Reference is before the first VEN
- **WHEN** VENs changed objective at 10:00, 10:04 and 10:08
- **THEN** the reference plan is the one as of 09:59:59, not 10:07:59
