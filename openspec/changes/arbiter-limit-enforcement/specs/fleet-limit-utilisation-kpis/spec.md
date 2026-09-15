## ADDED Requirements

### Requirement: Utilisation next to the pass bar
For each VEN and engaged hard-limit window with a paired baseline, the KPI tooling SHALL report
utilisation as the sum of actual import divided by the sum of the lower of baseline import and the
limit, and unused headroom as the energy the VEN could have imported within the limit but didn't,
without changing the pass bar.

#### Scenario: Over-curtailment visible
- **WHEN** a VEN's baseline import is 2.0 kW under a 1.5 kW limit and it imports 0.5 kW throughout
- **THEN** utilisation is 0.33 and unused headroom is 1.0 kW times the window duration, while pass is still true

#### Scenario: No paired baseline
- **WHEN** the run has no paired baseline
- **THEN** utilisation and unused headroom are reported as null

### Requirement: Comfort next to the pass bar
For each VEN and limit window the KPI tooling SHALL report heater temperature at window end versus
the paired baseline, minutes the heater spent below its minimum temperature, and EV energy charged
versus the paired baseline.

#### Scenario: Heater paused under a cap
- **WHEN** a heater was paused for the window and ends 0.4 °C cooler than in the baseline
- **THEN** the comfort block reports a −0.4 °C heater temperature delta and 0 minutes below minimum
