## ADDED Requirements

### Requirement: Limit windows are derived from the action log
The KPI tooling SHALL derive limit windows from `run.json["actions"]`: `capacity_limit` as an
import limit of `import_kw`, `export_capacity_limit` as an export limit of `export_kw`, and
`alert` as an import limit of 0 kW, each over `[started_at, started_at + duration_minutes)`;
where windows overlap in one direction, the strictest limit SHALL apply.

#### Scenario: Alert inside a capacity limit
- **WHEN** a 1.5 kW capacity limit runs from minute 5 to 25 and an alert from minute 10 to 20
- **THEN** the import target is 1.5 kW for minutes 5–9 and 21–24 and 0 kW for minutes 10–19

### Requirement: Compliance is judged against the physical floor
The KPI tooling SHALL compute each VEN's per-minute import floor as base load plus shiftable-load
power plus heater power only while the heater is at or below its own minimum temperature, plus
PV power, minus available battery discharge, never below zero; the export floor SHALL be zero;
and the per-minute target SHALL be the higher of the limit and the floor.

#### Scenario: Storage-less VEN under a 0 kW alert
- **WHEN** a VEN with only base load (0.5 kW) is under an alert
- **THEN** its target is 0.5 kW and importing 0.5 kW counts as compliant

#### Scenario: Heater running in the comfort band is controllable
- **WHEN** a heater runs at full power at 20 °C with a minimum temperature of 18 °C under a capacity limit
- **THEN** the heater's power is not part of the floor

### Requirement: Pass bar of five minutes, sustained
For each VEN and limit window the KPI tooling SHALL report `time_to_comply_s` as the time from the
window start to the first minute from which the VEN stays at or below its target (+0.05 kW) for
the rest of the window, SHALL report `pass` when that time is at most 300 s, and SHALL report a
window where compliance is never sustained as not passing.

#### Scenario: Late but sustained compliance
- **WHEN** a VEN exceeds its target for the first 6 minutes of a limit window and complies afterwards
- **THEN** `time_to_comply_s` is 360 and `pass` is false

#### Scenario: Complies then relapses
- **WHEN** a VEN complies at minute 1 but exceeds its target again at minute 12 and stays over
- **THEN** compliance is not sustained and `pass` is false

### Requirement: Engagement and fleet summary
The KPI tooling SHALL mark a VEN × window as engaged when the VEN's actual reached at least 80% of
the limit during the window, its floor exceeded the limit, or it was above the limit at window
start, and SHALL report per window the count of engaged VENs, the count of engaged VENs that
passed, and the failing VENs.

#### Scenario: Limit far above a VEN's level
- **WHEN** a VEN's import stays below 50% of a 4 kW limit for the whole window
- **THEN** that VEN is not engaged and does not count toward the window's pass rate

### Requirement: Signal integrity check
The KPI tooling SHALL compare, minute by minute within the scenario window, the VEN's recorded
import tariff with the value the scenario's `price_series` defines for that minute, and SHALL
report the number of mismatching minutes per VEN.

#### Scenario: Tariff overridden
- **WHEN** a VEN recorded 0.09 €/kWh throughout a window whose scenario price was 0.10/0.45/0.10
- **THEN** every minute of that window is reported as a mismatch for that VEN

### Requirement: Fleet coincident peak
The KPI tooling SHALL report the fleet's coincident peak import as the maximum over minutes of
the sum of all VENs' import in that minute, separately from each VEN's own peak.

#### Scenario: Peaks at different minutes
- **WHEN** VEN A peaks at 3 kW in minute 2 and VEN B peaks at 3 kW in minute 5, each importing 1 kW otherwise
- **THEN** the fleet coincident peak is 4 kW, not 6 kW
