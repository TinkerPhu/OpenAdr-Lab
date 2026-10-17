# Spec Delta

## Purpose
Let a resident express an EV comfort curve as the total they will pay to reach a charge level, while the planner keeps consuming the marginal bids it already understands.

## ADDED Requirements

### Requirement: Cumulative spend becomes marginal bids
The VEN SHALL accept an EV comfort curve as points of (fill, cumulative spend in €) measured from the asset's current charge, and SHALL store it as the marginal-bid curve it already stores. The marginal bid of each interval SHALL be the interval's added spend divided by its energy. Energy per unit of fill SHALL come from the asset, not from the caller.

#### Scenario: Two points
- **WHEN** a 50 kWh EV at 20 % is given "60 % for €2.00, 80 % for €2.50"
- **THEN** the stored bid is €2.00 / 20 kWh = 0.10 €/kWh up to 60 % and €0.50 / 10 kWh = 0.05 €/kWh from 60 % to 80 %, and the curve's source is `override`

#### Scenario: Round trip
- **WHEN** a stored curve is read back as cumulative spend
- **THEN** the spend at each fill is the area under the stored curve from the current charge, within one cent of what was entered

#### Scenario: Planner is unchanged
- **WHEN** a curve entered as cumulative spend is stored
- **THEN** the planner prices the same bands it would for the equivalent marginal curve

### Requirement: Concave spend only
A cumulative curve SHALL be refused when an interval adds more per kWh than the one before it (spend not concave), with HTTP 422 and a message that names the point and the two per-kWh figures in spend terms. The marginal-curve rule that bids do not rise with fill is unchanged and remains the single validator.

#### Scenario: Rising interval refused
- **WHEN** the points are "40 % for €1.00, 60 % for €3.00"
- **THEN** the request is refused with 422, naming the second point

#### Scenario: Spend that falls is refused
- **WHEN** a point carries less cumulative spend than the one before it
- **THEN** the request is refused with 422

### Requirement: Assets that cannot convert keep marginal input
For an asset that declares no energy per fill, the cumulative route SHALL answer 422 (not 404) and the UI SHALL keep the marginal editor for it.

#### Scenario: Heater
- **WHEN** a cumulative curve is posted for the heater
- **THEN** the answer is 422 stating that spend input is not available for that asset
