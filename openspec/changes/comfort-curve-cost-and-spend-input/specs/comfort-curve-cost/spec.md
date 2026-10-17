# Spec Delta

## Purpose
Let a resident see what a comfort curve can cost and what the plan actually spends, so a curve is a number they can check rather than an abstract bid.

## ADDED Requirements

### Requirement: Maximum cost of a curve
For an asset that prices energy by a comfort curve, the VEN SHALL report the maximum cost of a given curve: the sum over the energy ahead (from the asset's current charge to its charge limit) of kWh × price bid. CO2 bids SHALL NOT be included, so the figure is in euros that can be compared with a bill. The report SHALL also state the energy it covers.

#### Scenario: Flat curve
- **WHEN** the maximum is requested for an EV at 40 % of a 50 kWh pack with limit 80 %, for a curve bidding 0.20 €/kWh at every fill
- **THEN** the maximum is 0.20 × 20 kWh = €4.00 and the covered energy is 20 kWh

#### Scenario: Declining curve
- **WHEN** the maximum is requested for a straight ramp from 0.40 €/kWh at 0 % to 0.00 at 100 %, EV at 0 %, limit 100 %, 50 kWh
- **THEN** the maximum is the triangle's area, €10.00

#### Scenario: Preview does not store
- **WHEN** the maximum is requested for rows that differ from the stored curve
- **THEN** the answer reflects the rows and the stored curve and the plan are unchanged

#### Scenario: Asset without a priced curve
- **WHEN** the maximum is requested for an asset that does not price by comfort curve
- **THEN** the answer says no maximum is available and the UI shows none for it

### Requirement: Planned spend beside the maximum
The same report SHALL carry the cost the current plan spends on the asset over the plan horizon. The UI SHALL show planned spend and maximum cost next to the curve, labelled so a resident can tell them apart.

#### Scenario: Plan spends less than the maximum
- **WHEN** the plan charges the EV only in cheap hours
- **THEN** the card shows planned spend below the maximum cost

#### Scenario: Plan spends more than the curve allows
- **WHEN** a hard-deadline request forces charging beyond what the curve values
- **THEN** planned spend may exceed the maximum and the card says charging was forced by a deadline, rather than hiding the difference

#### Scenario: No charging planned
- **WHEN** the plan has no allocation for the asset
- **THEN** planned spend is €0.00, not missing
