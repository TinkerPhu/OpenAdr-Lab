## ADDED Requirements

### Requirement: The VEN records its nearest- and farthest-lead forecast for each forecastable asset every plan cycle
After each plan cycle resolves, the VEN SHALL record two forecast samples for each of PV,
base_load, and site-residual: one for the plan's second time slot (the nearest genuinely-future
instant) and one for the plan's last time slot (the farthest-lead instant the current horizon
reaches). Each sample SHALL carry the asset id, which of the two points it is, the forecasted
instant, the predicted power value, and when the recording happened. Recording SHALL happen every
plan cycle regardless of whether the predicted value changed from the previous cycle.

#### Scenario: A plan cycle records exactly two points per tracked asset
- **WHEN** a plan cycle resolves with at least two time slots
- **THEN** one new forecast sample is recorded for PV, base_load, and site-residual each, tagged as
  the near point (the plan's second slot), and one more for each tagged as the far point (the
  plan's last slot)

#### Scenario: The near point is never the plan's first slot
- **WHEN** a plan cycle resolves
- **THEN** the near-point sample's forecasted instant is the plan's second slot's start time, never
  the first slot's

#### Scenario: An unchanged predicted value is still recorded
- **WHEN** a plan cycle resolves with a near- or far-point predicted value identical to the
  previous cycle's
- **THEN** a new forecast sample is still recorded for that cycle

#### Scenario: Dispatchable assets are not tracked
- **WHEN** a plan cycle resolves
- **THEN** no forecast sample is recorded for battery, EV, or heater

### Requirement: A recorded forecast is reconciled with the real value once its instant elapses
Once the real measured or simulated value for a forecast sample's asset and instant becomes
available, the VEN SHALL attach that actual value and its own timestamp to the matching still-open
forecast sample(s) for that asset and instant. A forecast sample already carrying an actual value
SHALL NOT be overwritten by a later reconciliation attempt.

#### Scenario: An elapsed forecast is filled in with the real value
- **WHEN** the real value for an asset and instant that a recorded forecast sample predicted
  becomes available
- **THEN** that forecast sample's actual value and actual-recorded-at timestamp are set to it

#### Scenario: Both near and far samples for the same instant are reconciled independently
- **WHEN** a near-point sample and a far-point sample happen to predict the same instant for the
  same asset
- **THEN** both samples receive the actual value once it becomes available

#### Scenario: An already-reconciled sample is not overwritten
- **WHEN** a forecast sample already has an actual value recorded
- **THEN** a subsequent reconciliation pass for the same asset and overlapping time window does not
  change it

### Requirement: Recorded forecasts are queryable by time range, asset, and lead point
The VEN SHALL expose recorded forecast samples — predicted value, actual value once reconciled, and
both associated timestamps — via a query filterable by time range, asset id, and near/far point,
following the same range-validation rules as the existing history query routes.

#### Scenario: A query returns both predicted and actual values once reconciled
- **WHEN** querying forecast samples for a time range that includes an elapsed, reconciled instant
- **THEN** the response includes both the predicted value and the actual value for that sample

#### Scenario: A query for a not-yet-elapsed instant returns the forecast without an actual
- **WHEN** querying forecast samples for a time range that includes an instant that has not yet
  elapsed
- **THEN** the response includes that sample with no actual value

#### Scenario: Filtering by asset and lead point narrows the result
- **WHEN** querying with an asset id and a near/far filter
- **THEN** only samples for that asset and lead point are returned

### Requirement: Recorded forecasts are retained under the same policy as other long-term history
Forecast samples SHALL be subject to the same retention pruning as other long-term history tables,
keyed on the forecasted instant.

#### Scenario: An old forecast sample is pruned regardless of reconciliation state
- **WHEN** retention pruning runs with a cutoff after a forecast sample's forecasted instant
- **THEN** that sample is deleted, whether or not it was ever reconciled with an actual value
